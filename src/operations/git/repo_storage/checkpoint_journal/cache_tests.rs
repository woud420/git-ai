use super::cache::{
    JournalCache, decode_count, read_cached_with_cache, read_cached_with_cache_after_decode,
    reset_decode_count, retained_capacity_bytes,
};
use super::*;
use std::sync::Mutex;

#[test]
fn journal_lease_does_not_expose_general_mutable_deref() {
    let source = include_str!("cache.rs");
    assert!(!source.contains("impl DerefMut for JournalLease"));
}

fn checkpoint(author: &str) -> Checkpoint {
    let mut checkpoint = Checkpoint::new(
        crate::model::working_log::CheckpointKind::AiAgent,
        format!("diff-{author}"),
        author.to_string(),
        Vec::new(),
    );
    checkpoint.timestamp = 1;
    checkpoint
}

fn checkpoint_with_missing_blob() -> Checkpoint {
    let entry = crate::model::working_log::WorkingLogEntry::new(
        "src/missing.rs".to_string(),
        "0".repeat(64),
        Vec::new(),
        Vec::new(),
    );
    Checkpoint::new(
        crate::model::working_log::CheckpointKind::AiAgent,
        "missing-diff".to_string(),
        "missing".to_string(),
        vec![entry],
    )
}

fn location<'a>(directory: &'a Path) -> JournalLocation<'a> {
    fs::create_dir_all(directory).unwrap();
    JournalLocation::new(directory, "base")
}

fn write_records(location: &JournalLocation<'_>, checkpoints: &[Checkpoint]) {
    let mut bytes = Vec::new();
    for checkpoint in checkpoints {
        bytes.extend(wire_v2::encode(checkpoint).unwrap());
        bytes.push(b'\n');
    }
    fs::write(location.checkpoints_file(), bytes).unwrap();
}

fn append_record_ignoring_lock(location: &JournalLocation<'_>, checkpoint: &Checkpoint) {
    let mut bytes = wire_v2::encode(checkpoint).unwrap();
    bytes.push(b'\n');
    OpenOptions::new()
        .append(true)
        .open(location.checkpoints_file())
        .unwrap()
        .write_all(&bytes)
        .unwrap();
}

#[test]
fn unchanged_warm_checkout_decodes_no_historical_records() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first"), checkpoint("second")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));

    drop(read_cached_with_cache(&location, u64::MAX, &cache).unwrap());
    reset_decode_count();
    let journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    assert_eq!(journal.len(), 2);
    assert_eq!(decode_count(), 0);
}

#[test]
fn accepted_revision_includes_skipped_blank_and_crlf_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    let mut unsupported = checkpoint("unsupported");
    unsupported.api_version = "checkpoint/unsupported".to_string();
    let supported = checkpoint("supported");
    let mut bytes = serde_json::to_vec(&unsupported).unwrap();
    bytes.extend_from_slice(b"\n \t\n");
    bytes.extend_from_slice(&wire_v2::encode(&supported).unwrap());
    bytes.extend_from_slice(b"\r\n");
    fs::write(location.checkpoints_file(), bytes).unwrap();
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));

    let cold = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(cold.len(), 1);
    assert_eq!(cold[0].author, "supported");
    drop(cold);
    reset_decode_count();

    let warm = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(warm.len(), 1);
    assert_eq!(decode_count(), 0);
}

#[test]
fn same_size_tamper_invalidates_cache_and_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    drop(read_cached_with_cache(&location, u64::MAX, &cache).unwrap());

    let path = location.checkpoints_file();
    let mut bytes = fs::read(&path).unwrap();
    let offset = bytes
        .windows(b"first".len())
        .position(|window| window == b"first")
        .unwrap();
    bytes[offset] = b'F';
    fs::write(&path, bytes).unwrap();

    let error = read_cached_with_cache(&location, u64::MAX, &cache)
        .err()
        .expect("same-size tampering must not reuse decoded state");
    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn external_valid_append_reloads_the_complete_journal() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    drop(read_cached_with_cache(&location, u64::MAX, &cache).unwrap());

    append_record_ignoring_lock(&location, &checkpoint("external"));
    let journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    assert_eq!(journal.len(), 2);
    assert_eq!(journal[1].author, "external");
}

#[test]
fn cold_decode_cannot_adopt_a_newer_external_revision() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut injected = false;

    let journal = read_cached_with_cache_after_decode(&location, u64::MAX, &cache, &mut || {
        if !injected {
            append_record_ignoring_lock(&location, &checkpoint("external"));
            injected = true;
        }
    })
    .unwrap();
    assert_eq!(
        journal
            .iter()
            .map(|checkpoint| checkpoint.author.as_str())
            .collect::<Vec<_>>(),
        ["first", "external"]
    );
    drop(journal);

    reset_decode_count();
    let warm = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(warm.len(), 2);
    assert_eq!(decode_count(), 0);
}

#[test]
fn torn_tail_repair_does_not_poison_the_next_warm_checkout() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    drop(read_cached_with_cache(&location, u64::MAX, &cache).unwrap());
    OpenOptions::new()
        .append(true)
        .open(location.checkpoints_file())
        .unwrap()
        .write_all(br#"{"v":2"#)
        .unwrap();

    let repaired = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(repaired.len(), 1);
    drop(repaired);
    reset_decode_count();

    let warm = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(warm.len(), 1);
    assert_eq!(decode_count(), 0);
}

#[test]
fn failed_append_invalidates_the_checked_out_state() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    let missing_blob = checkpoint_with_missing_blob();

    assert!(
        journal
            .append_checkpoint(missing_blob, COMPACTION_INTERVAL)
            .is_err()
    );
    drop(journal);
    reset_decode_count();
    let reloaded = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    assert_eq!(reloaded.len(), 1);
    assert_eq!(decode_count(), 1);
}

#[test]
fn unpublished_mutation_is_not_reinserted_during_unwind() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
        let _ = journal.append_checkpoint_after_mutation(
            checkpoint("phantom"),
            COMPACTION_INTERVAL,
            &mut || panic!("stop before publication"),
        );
    }));
    assert!(result.is_err());

    reset_decode_count();
    let reloaded = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].author, "first");
    assert_eq!(decode_count(), 1);
}

#[test]
fn ordinary_cached_append_preserves_blob_verification_policy() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    let blobs = location.blobs_directory();
    fs::create_dir_all(&blobs).unwrap();
    fs::write(location.checkpoints_file(), b"").unwrap();
    let checkpoint = checkpoint_with_missing_blob();
    fs::write(
        blobs.join(&checkpoint.entries[0].blob_sha),
        b"wrong contents",
    )
    .unwrap();
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    journal
        .append_checkpoint(checkpoint, COMPACTION_INTERVAL)
        .expect("ordinary append syncs a fresh blob without rehashing it");
    let error = journal
        .ensure_durable(0)
        .expect_err("deduplicated replay must still verify blob contents");

    assert!(error.to_string().contains("blob hash mismatch"), "{error}");
}

#[test]
fn successful_append_cannot_mutate_cached_history() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    journal
        .append_checkpoint(checkpoint("second"), COMPACTION_INTERVAL)
        .unwrap();
    drop(journal);
    let warm = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    assert_eq!(warm[0].author, "first");
    assert_eq!(warm[1].author, "second");
}

#[test]
fn cached_interval_compaction_rewrites_and_stays_warm() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    journal.append_checkpoint(checkpoint("second"), 2).unwrap();
    drop(journal);
    reset_decode_count();
    let warm = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(warm.len(), 2);
    assert_eq!(decode_count(), 0);
    drop(warm);

    let disk = read(&location, u64::MAX).unwrap();
    assert_eq!(
        disk.iter()
            .map(|checkpoint| checkpoint.author.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(
        !location
            .checkpoints_file()
            .with_extension("jsonl.tmp")
            .exists()
    );
}

#[test]
fn pruning_releases_stale_attribution_capacity() {
    let mut old = checkpoint("old");
    let mut attributions = Vec::with_capacity(4096);
    attributions.push(crate::model::attribution::Attribution {
        start: 0,
        end: 1,
        author_id: "author".to_string(),
        ts: 1,
    });
    old.entries
        .push(crate::model::working_log::WorkingLogEntry::new(
            "same.rs".to_string(),
            "old".to_string(),
            attributions,
            Vec::new(),
        ));
    let mut new = checkpoint("new");
    new.entries
        .push(crate::model::working_log::WorkingLogEntry::new(
            "same.rs".to_string(),
            "new".to_string(),
            Vec::new(),
            Vec::new(),
        ));
    let mut checkpoints = vec![old, new];

    prune_old_char_attributions(&mut checkpoints);

    assert_eq!(checkpoints[0].entries[0].attributions.capacity(), 0);
}

#[test]
fn generation_conflict_cannot_overwrite_an_external_append() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    append_record_ignoring_lock(&location, &checkpoint("external"));

    let error = journal
        .append_checkpoint(checkpoint("candidate"), COMPACTION_INTERVAL)
        .expect_err("a stale generation must fail before append");
    assert!(
        error.to_string().contains("changed after checkout"),
        "{error}"
    );
    drop(journal);

    let reloaded = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert_eq!(
        reloaded
            .iter()
            .map(|checkpoint| checkpoint.author.as_str())
            .collect::<Vec<_>>(),
        ["first", "external"]
    );
}

#[test]
fn cache_evicts_oldest_journal_at_its_bound() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let directories = [
        directory.path().join("one"),
        directory.path().join("two"),
        directory.path().join("three"),
    ];
    for (index, directory) in directories.iter().enumerate() {
        let location = location(directory);
        write_records(&location, &[checkpoint(&format!("record-{index}"))]);
        drop(read_cached_with_cache(&location, u64::MAX, &cache).unwrap());
    }

    reset_decode_count();
    let first = location(&directories[0]);
    drop(read_cached_with_cache(&first, u64::MAX, &cache).unwrap());

    assert_eq!(decode_count(), 1);
    assert_eq!(cache.lock().unwrap().len(), 2);
}

#[test]
fn cache_evicts_to_its_retained_byte_budget() {
    let directory = tempfile::tempdir().unwrap();
    let first_directory = directory.path().join("first");
    let second_directory = directory.path().join("second");
    let first = location(&first_directory);
    let second = location(&second_directory);
    write_records(&first, &[checkpoint("first")]);
    write_records(&second, &[checkpoint("other")]);
    let retained_bytes = retained_capacity_bytes(&read(&first, u64::MAX).unwrap());
    let cache = Mutex::new(JournalCache::new(8, retained_bytes * 2 - 1));

    drop(read_cached_with_cache(&first, u64::MAX, &cache).unwrap());
    drop(read_cached_with_cache(&second, u64::MAX, &cache).unwrap());
    reset_decode_count();
    drop(read_cached_with_cache(&first, u64::MAX, &cache).unwrap());

    assert_eq!(decode_count(), 1);
    assert_eq!(cache.lock().unwrap().len(), 1);
}

#[test]
fn external_rewrite_invalidates_cached_history() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    drop(read_cached_with_cache(&location, u64::MAX, &cache).unwrap());

    rewrite(&location, &[checkpoint("replacement")]).unwrap();
    let reloaded = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].author, "replacement");
}

#[test]
fn reset_generation_conflict_cannot_resurrect_cached_records() {
    let directory = tempfile::tempdir().unwrap();
    let location = location(directory.path());
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let mut journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();

    reset_file_durably(&location.checkpoints_file()).unwrap();
    assert!(
        journal
            .append_checkpoint(checkpoint("stale"), COMPACTION_INTERVAL)
            .is_err()
    );
    drop(journal);

    let reloaded = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    assert!(reloaded.is_empty());
}

#[test]
fn reset_obeys_journal_then_cache_lock_order() {
    let directory = tempfile::tempdir().unwrap();
    let log_directory = directory.path().to_path_buf();
    let location = location(&log_directory);
    write_records(&location, &[checkpoint("first")]);
    let cache = Mutex::new(JournalCache::new(2, u64::MAX));
    let journal = read_cached_with_cache(&location, u64::MAX, &cache).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(0);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            let location = JournalLocation::new(&log_directory, "base");
            started_tx.send(()).unwrap();
            reset(&location).unwrap();
            done_tx.send(()).unwrap();
        });
        started_rx.recv().unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(25)).is_err(),
            "reset must wait for the checked-out journal lease"
        );
        drop(journal);
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    });
}
