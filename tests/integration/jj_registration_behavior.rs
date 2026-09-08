use super::*;
use ciborium::Value;
use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

pub fn assert_registration(
    case: &Case,
    journal: &JjObservationJournal,
    registered: &RegisteredJjCurrentState,
    anchors: &[JjOperationEvidence],
    checkout: &str,
    outside: bool,
) {
    hex_id(registered.source_id());
    hex_id(registered.initialization_receipt_id());
    hex_id(registered.attachment_id());
    assert_ne!(registered.source_id(), registered.attachment_id());
    assert_eq!(registered.workspace_name(), "default");
    assert_eq!(registered.checkout().workspace_name, "default");
    assert_eq!(registered.checkout().operation_id, checkout);
    assert!(matches!(
        (registered.checkout_relation(), outside),
        (JjRegisteredCheckoutRelation::OutsideBaseline, true)
            | (JjRegisteredCheckoutRelation::BaselineAnchor, false)
    ));
    let baseline = registered.baseline();
    let receipt = baseline.receipt();
    assert_eq!(receipt.source_id(), registered.source_id());
    assert_eq!(receipt.reader_profile(), JJ_OBSERVATION_READER_PROFILE);
    assert_eq!(receipt.expected_native_generation(), 0);
    assert_eq!(receipt.generation(), 1);
    assert_eq!(receipt.boundary(), JjBaselineBoundary::CurrentState);
    assert_eq!(baseline.anchors(), anchors);
    let expected_heads: Vec<_> = anchors.iter().map(|e| e.operation_id.clone()).collect();
    assert_eq!(receipt.captured_head_ids(), expected_heads);
    let raw = journal
        .read_native_registration_record(registered.source_id(), &mut budget())
        .unwrap()
        .unwrap();
    assert_eq!(hash(&raw), registered.initialization_receipt_id());
    let registration: Value = ciborium::from_reader(raw.as_slice()).unwrap();
    assert_eq!(
        text_field(&registration, "source_id"),
        registered.source_id()
    );
    assert_eq!(
        text_field(&registration, "baseline_id"),
        receipt.baseline_id()
    );
    assert_eq!(
        text_field(&registration, "initial_workspace_name"),
        registered.workspace_name()
    );
    assert_eq!(
        text_field(&registration, "initial_attachment_id"),
        registered.attachment_id()
    );
    let raw = journal
        .read_native_workspace_record(
            registered.source_id(),
            registered.workspace_name(),
            &mut budget(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        hash(&raw),
        text_field(&registration, "initial_workspace_record_id")
    );
    let workspace: Value = ciborium::from_reader(raw.as_slice()).unwrap();
    assert_eq!(
        text_field(&workspace, "attachment_id"),
        registered.attachment_id()
    );
    assert_eq!(text_field(&workspace, "baseline_id"), receipt.baseline_id());
    assert_eq!(
        fs::read(case.seal()).unwrap(),
        seal_bytes(registered.source_id())
    );
    assert_eq!(counts(case), [1, 1, 1, 1]);
    let status = journal.status(registered.source_id()).unwrap();
    assert_eq!(status.generation, 0);
    assert!(status.observed_heads.is_empty());
    assert!(status.applied_heads.is_empty());
    assert_eq!(status.pending_operations, 0);
    assert!(
        journal
            .pending(registered.source_id(), 8)
            .unwrap()
            .is_empty()
    );
}

pub(super) fn assert_only_publication(
    case: &Case,
    mut before: BTreeMap<PathBuf, Entry>,
    source: &str,
) {
    let namespace = case.namespace();
    let namespace_metadata = fs::symlink_metadata(&namespace).unwrap();
    let seal_metadata = fs::symlink_metadata(case.seal()).unwrap();
    assert!(namespace_metadata.is_dir());
    assert!(!namespace_metadata.file_type().is_symlink());
    assert!(seal_metadata.is_file());
    assert!(!seal_metadata.file_type().is_symlink());
    assert_eq!(namespace_metadata.permissions().mode() & 0o777, 0o700);
    assert_eq!(seal_metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(seal_metadata.nlink(), 1);
    assert_eq!(namespace_metadata.uid(), unsafe { libc::geteuid() });
    assert_eq!(seal_metadata.uid(), namespace_metadata.uid());
    before.insert(
        namespace.strip_prefix(&case.repository).unwrap().to_owned(),
        Entry::Directory,
    );
    before.insert(
        case.seal()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::File(seal_bytes(source)),
    );
    assert_eq!(manifest(&case.repository), before);
}

pub fn same_receipt(actual: &RegisteredJjCurrentState, expected: &RegisteredJjCurrentState) {
    assert_eq!(actual.source_id(), expected.source_id());
    assert_eq!(
        actual.initialization_receipt_id(),
        expected.initialization_receipt_id()
    );
    assert_eq!(actual.workspace_name(), expected.workspace_name());
    assert_eq!(actual.attachment_id(), expected.attachment_id());
    assert_eq!(actual.baseline().receipt(), expected.baseline().receipt());
    assert_eq!(actual.baseline().anchors(), expected.baseline().anchors());
}

pub fn install(case: &Case, config: &Config) {
    let mut journal = case.open();
    assert_eq!(counts(case), [0; 4]);
    let before = manifest(&case.repository);
    let registered = installed(case.register(&mut journal, config).unwrap());
    assert_registration(case, &journal, &registered, &[merge()], MERGE_ID, false);
    assert_only_publication(case, before, registered.source_id());
    drop(journal);
    let journal = case.open();
    let before = state(case);
    let reopened = case.reopen(&journal, config).unwrap().unwrap();
    same_receipt(&reopened, &registered);
    assert_registration(case, &journal, &reopened, &[merge()], MERGE_ID, false);
    assert_eq!(state(case), before);
}

pub fn absent(case: &Case, config: &Config) {
    let journal = case.open();
    let before = state(case);
    assert!(case.reopen(&journal, config).unwrap().is_none());
    assert_eq!(state(case), before);
    assert_eq!(counts(case), [0; 4]);
    assert!(!case.namespace().exists());
}

pub fn retry(case: &Case, config: &Config) {
    let mut journal = case.open();
    let first = installed(case.register(&mut journal, config).unwrap());
    let before = state(case);
    let retry = already(case.register(&mut journal, config).unwrap());
    same_receipt(&retry, &first);
    assert_registration(case, &journal, &retry, &[merge()], MERGE_ID, false);
    assert_eq!(state(case), before);
}

pub fn advance(case: &Case, config: &Config) {
    let mut journal = case.open();
    let first = installed(case.register(&mut journal, config).unwrap());
    let successor = vectors::successor();
    let verified = verify_evidence(JJ_OBSERVATION_READER_PROFILE, &successor).unwrap();
    assert_eq!(verified.operation().parent_ids, [MERGE_ID]);
    assert!(verified.view().wc_commit_ids.contains_key("default"));
    case.write_evidence(&successor);
    case.heads(&[&successor.operation_id]);
    case.write_checkout(&successor.operation_id);
    let captured = capture_current_state(&case.context(), deadline()).unwrap();
    assert_eq!(
        captured.head_ids(),
        std::slice::from_ref(&successor.operation_id)
    );
    assert_eq!(captured.checkout().operation_id, successor.operation_id);
    assert_eq!(
        captured.checkout_relation(),
        JjCheckoutRelation::OperationIsCapturedHead
    );
    let before = state(case);
    drop(journal);
    let mut journal = case.open();
    let reopened = case.reopen(&journal, config).unwrap().unwrap();
    let retry = already(case.register(&mut journal, config).unwrap());
    for current in [&reopened, &retry] {
        same_receipt(current, &first);
        assert_registration(
            case,
            &journal,
            current,
            &[merge()],
            &successor.operation_id,
            true,
        );
    }
    assert_eq!(state(case), before);
}

pub fn outside(case: &Case, config: &Config) {
    case.write_evidence(&left());
    case.heads(&[LEFT_ID]);
    let mut journal = case.open();
    let before = manifest(&case.repository);
    let registered = installed(case.register(&mut journal, config).unwrap());
    assert_registration(case, &journal, &registered, &[left()], MERGE_ID, true);
    assert_only_publication(case, before, registered.source_id());
    let before = state(case);
    let reopened = case.reopen(&journal, config).unwrap().unwrap();
    same_receipt(&reopened, &registered);
    assert_registration(case, &journal, &reopened, &[left()], MERGE_ID, true);
    assert_eq!(state(case), before);
}
