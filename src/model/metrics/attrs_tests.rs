use super::*;
use serde_json::Value;

#[test]
fn test_event_attributes_builder() {
    let attrs = EventAttributes::with_version("1.0.0")
        .repo_url("https://github.com/user/repo")
        .author("user@example.com")
        .commit_sha("commit-123")
        .base_commit_sha("base-commit-123")
        .branch("main")
        .tool("claude-code")
        .model_null();

    assert_eq!(attrs.git_ai_version, Some(Some("1.0.0".to_string())));
    assert_eq!(
        attrs.repo_url,
        Some(Some("https://github.com/user/repo".to_string()))
    );
    assert_eq!(attrs.author, Some(Some("user@example.com".to_string())));
    assert_eq!(attrs.commit_sha, Some(Some("commit-123".to_string())));
    assert_eq!(
        attrs.base_commit_sha,
        Some(Some("base-commit-123".to_string()))
    );
    assert_eq!(attrs.branch, Some(Some("main".to_string())));
    assert_eq!(attrs.tool, Some(Some("claude-code".to_string())));
    assert_eq!(attrs.model, Some(None)); // explicitly null
    assert_eq!(attrs.prompt_id, None); // tombstoned - never written
}

#[test]
fn test_event_attributes_to_sparse() {
    let attrs = EventAttributes::with_version("1.0.0")
        .tool("test-tool")
        .model_null();

    let sparse = attrs.to_sparse();

    assert_eq!(sparse.get("0"), Some(&Value::String("1.0.0".to_string())));
    assert_eq!(sparse.get("1"), None); // not set
    assert_eq!(sparse.get("2"), None); // not set
    assert_eq!(sparse.get("3"), None); // not set
    assert_eq!(sparse.get("4"), None); // not set
    assert_eq!(sparse.get("5"), None); // not set
    assert_eq!(
        sparse.get("20"),
        Some(&Value::String("test-tool".to_string()))
    );
    assert_eq!(sparse.get("21"), Some(&Value::Null)); // explicitly null
    assert_eq!(sparse.get("22"), None); // tombstoned - never written
}

#[test]
fn test_event_attributes_from_sparse() {
    let mut sparse = SparseArray::new();
    sparse.insert("0".to_string(), Value::String("2.0.0".to_string()));
    sparse.insert("1".to_string(), Value::Null);
    sparse.insert("20".to_string(), Value::String("my-tool".to_string()));
    sparse.insert("22".to_string(), Value::String("prompt-123".to_string()));

    let attrs = EventAttributes::from_sparse(&sparse);

    assert_eq!(attrs.git_ai_version, Some(Some("2.0.0".to_string())));
    assert_eq!(attrs.repo_url, Some(None)); // null
    assert_eq!(attrs.author, None); // not set
    assert_eq!(attrs.tool, Some(Some("my-tool".to_string())));
    assert_eq!(attrs.model, None); // not set
    assert_eq!(attrs.prompt_id, Some(Some("prompt-123".to_string())));
}

#[test]
fn test_event_attributes_all_fields() {
    let attrs = EventAttributes::with_version("1.2.3")
        .repo_url("https://github.com/user/repo")
        .author("dev@example.com")
        .commit_sha("abc123")
        .base_commit_sha("def456")
        .branch("feature-branch")
        .tool("cursor")
        .model("gpt-4")
        .external_session_id("ext-789");

    assert_eq!(attrs.git_ai_version, Some(Some("1.2.3".to_string())));
    assert_eq!(
        attrs.repo_url,
        Some(Some("https://github.com/user/repo".to_string()))
    );
    assert_eq!(attrs.author, Some(Some("dev@example.com".to_string())));
    assert_eq!(attrs.commit_sha, Some(Some("abc123".to_string())));
    assert_eq!(attrs.base_commit_sha, Some(Some("def456".to_string())));
    assert_eq!(attrs.branch, Some(Some("feature-branch".to_string())));
    assert_eq!(attrs.tool, Some(Some("cursor".to_string())));
    assert_eq!(attrs.model, Some(Some("gpt-4".to_string())));
    assert_eq!(attrs.prompt_id, None); // tombstoned
    assert_eq!(attrs.external_session_id, Some(Some("ext-789".to_string())));
}

#[test]
fn test_event_attributes_all_nulls() {
    let attrs = EventAttributes::new()
        .git_ai_version_null()
        .repo_url_null()
        .author_null()
        .commit_sha_null()
        .base_commit_sha_null()
        .branch_null()
        .tool_null()
        .model_null()
        .external_session_id_null();

    assert_eq!(attrs.git_ai_version, Some(None));
    assert_eq!(attrs.repo_url, Some(None));
    assert_eq!(attrs.author, Some(None));
    assert_eq!(attrs.commit_sha, Some(None));
    assert_eq!(attrs.base_commit_sha, Some(None));
    assert_eq!(attrs.branch, Some(None));
    assert_eq!(attrs.tool, Some(None));
    assert_eq!(attrs.model, Some(None));
    assert_eq!(attrs.prompt_id, None); // tombstoned - no setter available
    assert_eq!(attrs.external_session_id, Some(None));
}

#[test]
fn test_event_attributes_to_sparse_all_fields() {
    let attrs = EventAttributes::with_version("1.0.0")
        .repo_url("https://github.com/test/repo")
        .author("author@test.com")
        .commit_sha("commit-sha")
        .base_commit_sha("base-sha")
        .branch("main")
        .tool("test-tool")
        .model("test-model")
        .external_session_id("ext-id");

    let sparse = attrs.to_sparse();

    assert_eq!(sparse.get("0"), Some(&Value::String("1.0.0".to_string())));
    assert_eq!(
        sparse.get("1"),
        Some(&Value::String("https://github.com/test/repo".to_string()))
    );
    assert_eq!(
        sparse.get("2"),
        Some(&Value::String("author@test.com".to_string()))
    );
    assert_eq!(
        sparse.get("3"),
        Some(&Value::String("commit-sha".to_string()))
    );
    assert_eq!(
        sparse.get("4"),
        Some(&Value::String("base-sha".to_string()))
    );
    assert_eq!(sparse.get("5"), Some(&Value::String("main".to_string())));
    assert_eq!(
        sparse.get("20"),
        Some(&Value::String("test-tool".to_string()))
    );
    assert_eq!(
        sparse.get("21"),
        Some(&Value::String("test-model".to_string()))
    );
    assert_eq!(sparse.get("22"), None); // tombstoned - never written
    assert_eq!(sparse.get("23"), Some(&Value::String("ext-id".to_string())));
}

#[test]
fn test_event_attributes_roundtrip() {
    let original = EventAttributes::with_version("2.5.0")
        .repo_url("https://gitlab.com/org/repo")
        .author_null()
        .commit_sha("sha123")
        .tool("copilot");

    let sparse = original.to_sparse();
    let restored = EventAttributes::from_sparse(&sparse);

    assert_eq!(restored.git_ai_version, Some(Some("2.5.0".to_string())));
    assert_eq!(
        restored.repo_url,
        Some(Some("https://gitlab.com/org/repo".to_string()))
    );
    assert_eq!(restored.author, Some(None)); // explicitly null
    assert_eq!(restored.commit_sha, Some(Some("sha123".to_string())));
    assert_eq!(restored.tool, Some(Some("copilot".to_string())));
    assert_eq!(restored.base_commit_sha, None); // not set
    assert_eq!(restored.model, None); // not set
}

#[test]
fn test_event_attributes_partial_sparse() {
    let mut sparse = SparseArray::new();
    sparse.insert("0".to_string(), Value::String("3.0.0".to_string()));
    sparse.insert("20".to_string(), Value::String("windsurf".to_string()));

    let attrs = EventAttributes::from_sparse(&sparse);

    assert_eq!(attrs.git_ai_version, Some(Some("3.0.0".to_string())));
    assert_eq!(attrs.repo_url, None); // not set
    assert_eq!(attrs.author, None); // not set
    assert_eq!(attrs.tool, Some(Some("windsurf".to_string())));
    assert_eq!(attrs.branch, None); // not set
}

#[test]
fn test_event_attributes_default() {
    let attrs = EventAttributes::default();

    assert_eq!(attrs.git_ai_version, None);
    assert_eq!(attrs.repo_url, None);
    assert_eq!(attrs.author, None);
    assert_eq!(attrs.commit_sha, None);
    assert_eq!(attrs.base_commit_sha, None);
    assert_eq!(attrs.branch, None);
    assert_eq!(attrs.tool, None);
    assert_eq!(attrs.model, None);
    assert_eq!(attrs.prompt_id, None);
    assert_eq!(attrs.external_session_id, None);
}

#[test]
fn test_event_attributes_git_ai_version_builder() {
    let attrs = EventAttributes::new().git_ai_version("4.0.0");
    assert_eq!(attrs.git_ai_version, Some(Some("4.0.0".to_string())));
}

#[test]
fn test_event_attributes_sparse_positions() {
    // Verify the position constants match expected values
    use super::attr_pos::*;

    assert_eq!(GIT_AI_VERSION, 0);
    assert_eq!(REPO_URL, 1);
    assert_eq!(AUTHOR, 2);
    assert_eq!(COMMIT_SHA, 3);
    assert_eq!(BASE_COMMIT_SHA, 4);
    assert_eq!(BRANCH, 5);
    assert_eq!(TOOL, 20);
    assert_eq!(MODEL, 21);
    assert_eq!(PROMPT_ID, 22);
    assert_eq!(EXTERNAL_SESSION_ID, 23);
    assert_eq!(SESSION_ID, 24);
    assert_eq!(TRACE_ID, 25);
}

#[test]
fn test_event_attributes_session_id_builder() {
    let attrs = EventAttributes::with_version("1.0.0")
        .session_id("session-123")
        .trace_id("trace-456");

    assert_eq!(attrs.session_id, Some(Some("session-123".to_string())));
    assert_eq!(attrs.trace_id, Some(Some("trace-456".to_string())));
}

#[test]
fn test_event_attributes_session_id_null() {
    let attrs = EventAttributes::with_version("1.0.0")
        .session_id_null()
        .trace_id_null();

    assert_eq!(attrs.session_id, Some(None));
    assert_eq!(attrs.trace_id, Some(None));
}

#[test]
fn test_event_attributes_to_sparse_with_session_fields() {
    let attrs = EventAttributes::with_version("1.0.0")
        .session_id("session-abc")
        .trace_id("trace-xyz")
        .tool("test-tool");

    let sparse = attrs.to_sparse();

    assert_eq!(sparse.get("0"), Some(&Value::String("1.0.0".to_string())));
    assert_eq!(
        sparse.get("20"),
        Some(&Value::String("test-tool".to_string()))
    );
    assert_eq!(
        sparse.get("24"),
        Some(&Value::String("session-abc".to_string()))
    );
    assert_eq!(
        sparse.get("25"),
        Some(&Value::String("trace-xyz".to_string()))
    );
}

#[test]
fn test_event_attributes_from_sparse_with_session_fields() {
    let mut sparse = SparseArray::new();
    sparse.insert("0".to_string(), Value::String("2.0.0".to_string()));
    sparse.insert("24".to_string(), Value::String("session-123".to_string()));
    sparse.insert("25".to_string(), Value::Null);

    let attrs = EventAttributes::from_sparse(&sparse);

    assert_eq!(attrs.git_ai_version, Some(Some("2.0.0".to_string())));
    assert_eq!(attrs.session_id, Some(Some("session-123".to_string())));
    assert_eq!(attrs.trace_id, Some(None)); // null
}

#[test]
fn test_event_attributes_roundtrip_with_session_fields() {
    let original = EventAttributes::with_version("2.5.0")
        .session_id("session-roundtrip")
        .trace_id_null()
        .tool("copilot");

    let sparse = original.to_sparse();
    let restored = EventAttributes::from_sparse(&sparse);

    assert_eq!(restored.git_ai_version, Some(Some("2.5.0".to_string())));
    assert_eq!(
        restored.session_id,
        Some(Some("session-roundtrip".to_string()))
    );
    assert_eq!(restored.trace_id, Some(None)); // explicitly null
    assert_eq!(restored.tool, Some(Some("copilot".to_string())));
}

#[test]
fn test_event_attributes_prompt_id_backward_compat() {
    // Test that tombstoned prompt_id still works for deserialization
    let mut sparse = SparseArray::new();
    sparse.insert("0".to_string(), Value::String("1.0.0".to_string()));
    sparse.insert("22".to_string(), Value::String("old-prompt-id".to_string()));
    sparse.insert("24".to_string(), Value::String("new-session".to_string()));

    let attrs = EventAttributes::from_sparse(&sparse);

    assert_eq!(attrs.prompt_id, Some(Some("old-prompt-id".to_string())));
    assert_eq!(attrs.session_id, Some(Some("new-session".to_string())));
}

#[test]
fn test_event_attributes_external_session_ids() {
    let attrs = EventAttributes::with_version("1.0.0")
        .session_id("internal-session")
        .external_session_id("agent-uuid-123")
        .external_parent_session_id("parent-uuid-456");

    assert_eq!(
        attrs.external_session_id,
        Some(Some("agent-uuid-123".to_string()))
    );
    assert_eq!(
        attrs.external_parent_session_id,
        Some(Some("parent-uuid-456".to_string()))
    );

    let sparse = attrs.to_sparse();
    assert_eq!(
        sparse.get("23"),
        Some(&Value::String("agent-uuid-123".to_string()))
    );
    assert_eq!(
        sparse.get("27"),
        Some(&Value::String("parent-uuid-456".to_string()))
    );

    let restored = EventAttributes::from_sparse(&sparse);
    assert_eq!(
        restored.external_session_id,
        Some(Some("agent-uuid-123".to_string()))
    );
    assert_eq!(
        restored.external_parent_session_id,
        Some(Some("parent-uuid-456".to_string()))
    );
}

#[test]
fn test_event_attributes_external_session_id_opt() {
    let attrs = EventAttributes::with_version("1.0.0")
        .external_session_id_opt(Some("has-value".to_string()))
        .external_parent_session_id_opt(None);

    assert_eq!(
        attrs.external_session_id,
        Some(Some("has-value".to_string()))
    );
    assert_eq!(attrs.external_parent_session_id, None);
}
