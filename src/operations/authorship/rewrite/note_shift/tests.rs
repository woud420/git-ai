use crate::model::authorship_log::LineRange;
use crate::model::authorship_log_serialization::AttestationEntry;

use super::*;
use crate::operations::authorship::rewrite::DiffTreeResult;

fn log_for(file: &str, hash: &str) -> AuthorshipLog {
    let mut log = AuthorshipLog::new();
    log.get_or_create_file(file)
        .add_entry(AttestationEntry::new(
            hash.to_string(),
            vec![LineRange::Single(1)],
        ));
    log
}

#[test]
fn apply_chunk_shifts_merges_sources_for_one_target_across_chunks() {
    let target = "b".repeat(40);
    let first = log_for("first.rs", "1111111111111111")
        .serialize_to_string()
        .unwrap();
    let second = log_for("second.rs", "2222222222222222")
        .serialize_to_string()
        .unwrap();
    let pending = vec![
        PendingShift {
            new_sha: target.clone(),
            raw_note: &first,
        },
        PendingShift {
            new_sha: target.clone(),
            raw_note: &second,
        },
    ];
    let mut pending_iter = pending.into_iter();
    let mut merged_by_target = HashMap::new();

    apply_chunk_shifts(
        &mut merged_by_target,
        vec![DiffTreeResult::default()],
        &mut pending_iter,
    );
    apply_chunk_shifts(
        &mut merged_by_target,
        vec![DiffTreeResult::default()],
        &mut pending_iter,
    );

    assert_eq!(pending_iter.count(), 0);
    let merged = merged_by_target.get(&target).expect("merged target");
    assert_eq!(merged.metadata.base_commit_sha, target);
    assert!(
        merged
            .attestations
            .iter()
            .any(|attestation| attestation.file_path == "first.rs")
    );
    assert!(
        merged
            .attestations
            .iter()
            .any(|attestation| attestation.file_path == "second.rs")
    );
}

#[test]
fn serialization_errors_preserve_diagnostics_and_structured_kind() {
    for context in ["rewrite", "shifted"] {
        let error = serialization_error(context, "invalid data");
        assert_eq!(
            error.to_string(),
            format!("Generic error: failed to serialize {context} authorship log: invalid data")
        );
        assert!(matches!(
            error,
            GitAiError::Persistence(PersistenceError::Io {
                kind: std::io::ErrorKind::InvalidData,
                ..
            })
        ));
    }
}
