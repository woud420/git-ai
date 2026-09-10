use super::*;

pub fn durable(fixture: &Fixture, anchors: &[JjOperationEvidence]) -> DurableCurrentStateBaseline {
    let mut journal = fixture.open();
    baseline_support::installed(
        baseline_support::install(&mut journal, &fixture.source, anchors).unwrap(),
    );
    drop(journal);
    let journal = fixture.open();
    baseline_support::reopen(&journal, &fixture.source)
        .unwrap()
        .unwrap()
}

pub fn input<'a>(
    baseline: &'a DurableCurrentStateBaseline,
    heads: &'a [String],
    operations: &'a [&'a JjOperationEvidence],
) -> JjAncestryInput<'a> {
    let receipt = baseline.receipt();
    JjAncestryInput {
        source_id: receipt.source_id(),
        reader_profile: receipt.reader_profile(),
        baseline_id: receipt.baseline_id(),
        expected_native_generation: receipt.generation(),
        head_ids: heads,
        operations,
    }
}

pub fn checked<'a>(
    fixture: &Fixture,
    baseline: &'a DurableCurrentStateBaseline,
    input: JjAncestryInput<'a>,
) -> Result<VerifiedJjAncestry<'a>, JjAncestryError> {
    let before_repo = snapshot(fixture.repo.path());
    let before_home = snapshot(fixture.repo.test_home_path());
    let result = verify_ancestry_to_baseline(baseline, input);
    assert_eq!(snapshot(fixture.repo.path()), before_repo);
    assert_eq!(snapshot(fixture.repo.test_home_path()), before_home);
    result
}

pub fn rejected<'a>(
    fixture: &Fixture,
    baseline: &'a DurableCurrentStateBaseline,
    input: JjAncestryInput<'a>,
    category: Option<&str>,
) -> JjAncestryError {
    let error = match checked(fixture, baseline, input) {
        Ok(_) => panic!("invalid graph returned an ancestry proof"),
        Err(error) => error,
    };
    let standard: &dyn std::error::Error = &error;
    assert!(!standard.to_string().is_empty());
    if let Some(category) = category {
        assert!(
            standard.to_string().to_ascii_lowercase().contains(category),
            "{standard}"
        );
    }
    error
}

pub fn ordered_ids(proof: &VerifiedJjAncestry<'_>) -> Vec<String> {
    proof
        .ordered_operations()
        .iter()
        .map(|operation| operation.operation().operation_id.clone())
        .collect()
}

pub fn unrecorded() -> JjOperationEvidence {
    paired(
        UNRECORDED_ID,
        UNRECORDED_HEX,
        &[&"00".repeat(64)],
        MINIMAL_ID,
        MINIMAL_HEX,
    )
}
