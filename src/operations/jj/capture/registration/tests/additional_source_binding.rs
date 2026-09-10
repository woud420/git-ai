use super::*;

#[test]
fn created_inode_at_another_valid_source_does_not_transfer_source_binding() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let mut budget = RegistrationCaptureBudget::new(deadline());
    let mut initial = budget.open_initial(&fixture.context()).unwrap();
    canonical_policy(&mut initial);
    let created = initial.publish_new(SOURCE).unwrap();
    let original = occupied_namespace(&fixture);
    let destination = occupied_namespace(&other);
    let before = fs::metadata(original.join("registration")).unwrap();
    assert_eq!(before.dev(), fs::metadata(&other.repo).unwrap().dev());
    fs::rename(&original, &destination).unwrap();
    let after = fs::metadata(destination.join("registration")).unwrap();
    assert_eq!(
        (
            before.dev(),
            before.ino(),
            before.mode(),
            before.nlink(),
            before.len()
        ),
        (
            after.dev(),
            after.ino(),
            after.mode(),
            after.nlink(),
            after.len()
        ),
    );
    assert_eq!(
        fs::read(destination.join("registration")).unwrap(),
        expected_seal(SOURCE)
    );
    let error = require_error(budget.open_created(&other.context(), created));
    assert!(error.to_string().contains("binding"), "{error}");
    assert_eq!(budget.session_counters(1).live_directory_descriptors, 0);
    assert!(destination.join("registration").exists());
}
