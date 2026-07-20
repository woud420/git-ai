//! The built-in prompt_storage default is "local": prompt/transcript content
//! stays on this machine unless the user explicitly opts into "notes" or
//! "default" (CAS). TestRepo's default patch pins "notes" for attribution
//! tests, so these tests drop that override to observe the real default.

use crate::repos::test_repo::TestRepo;

#[test]
fn test_prompt_storage_defaults_to_local() {
    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.prompt_storage = None;
    });

    let value = repo
        .git_ai(&["config", "prompt_storage"])
        .expect("prompt_storage should be readable");
    assert!(
        value.contains("local"),
        "expected the built-in default to be local, got: {value}"
    );
}
