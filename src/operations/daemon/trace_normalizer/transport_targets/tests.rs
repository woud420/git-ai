use super::*;
use serde_json::json;

#[test]
fn ssh_destination_keeps_host_path_and_explicit_port() {
    for (argv, expected) in [
        (
            vec![
                "ssh",
                "git@example.test",
                "git-receive-pack 'team/repo.git'",
            ],
            "git@example.test:team/repo.git",
        ),
        (
            vec![
                "ssh",
                "-p",
                "2222",
                "git@example.test",
                "git-receive-pack '/srv/repo.git'",
            ],
            "ssh://git@example.test:2222/srv/repo.git",
        ),
        (
            vec![
                "ssh",
                "-o",
                "SendEnv=GIT_PROTOCOL",
                "git@example.test",
                "git-receive-pack 'team/repo.git'",
            ],
            "git@example.test:team/repo.git",
        ),
    ] {
        assert_eq!(
            target_from_frame(&json!({"argv":argv}), "transport/ssh", None).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn capture_refuses_ambiguous_shell_or_helper_destinations() {
    for (class, argv) in [
        ("transport/file", vec!["custom-receive-pack '/remote'"]),
        (
            "transport/file",
            vec!["git-receive-pack '/remote'; echo extra"],
        ),
        (
            "transport/ssh",
            vec!["custom-ssh", "example.test", "git-receive-pack '/remote'"],
        ),
        (
            "transport/ssh",
            vec![
                "ssh",
                "-o",
                "Port=2222",
                "example.test",
                "git-receive-pack '/remote'",
            ],
        ),
        (
            "remote-ext",
            vec!["git", "remote-ext", "origin", "ext::arbitrary"],
        ),
    ] {
        assert_eq!(target_from_frame(&json!({"argv":argv}), class, None), None);
    }
}

#[test]
fn file_destination_decodes_git_quotes_without_collapsing_parent_components() {
    let frame = json!({"argv":["git-receive-pack '../remote'\\''s space.git'"]});
    let root = std::env::temp_dir().join("worktree");
    let expected = root
        .join("../remote's space.git")
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        target_from_frame(&frame, "transport/file", Some(&root)),
        Some(expected)
    );
}

#[test]
fn http_destination_is_taken_from_the_matching_helper_argument() {
    assert_eq!(
        target_from_frame(
            &json!({"argv":["git","remote-https","origin","https://example.test/repo.git"]}),
            "remote-https",
            None
        )
        .as_deref(),
        Some("https://example.test/repo.git")
    );
    assert_eq!(
        target_from_frame(
            &json!({"argv":["git","remote-http","origin","https://example.test/repo.git"]}),
            "remote-https",
            None
        ),
        None
    );
}

fn captured_targets(frames: &[Value], command: &str) -> Option<Vec<String>> {
    use crate::operations::daemon::trace_normalizer::tests_lifecycle::MockBackend;
    let backend = std::sync::Arc::new(MockBackend::default());
    let mut normalizer = TraceNormalizer::new(backend);
    let root = std::env::temp_dir().join("capture-root");
    normalizer
        .ingest_payload(
            &json!({"event":"start","sid":"root","argv":["git",command],"worktree":root}),
        )
        .unwrap();
    for frame in frames {
        normalizer.ingest_payload(frame).unwrap();
    }
    normalizer.state().pending["root"].transport_targets.clone()
}

fn http_frame(target: &str) -> Value {
    json!({"event":"child_start","sid":"root","child_class":"remote-https","argv":["git","remote-https","origin",target]})
}

#[test]
fn capture_deduplicates_within_the_fixed_destination_limit() {
    let mut frames: Vec<_> = (0..MAX_TARGETS)
        .map(|n| http_frame(&format!("https://host.test/repo-{n}")))
        .collect();
    frames.push(frames[0].clone());
    assert_eq!(
        captured_targets(&frames, "push").unwrap().len(),
        MAX_TARGETS
    );
    frames.push(http_frame("https://host.test/ninth"));
    assert_eq!(
        captured_targets(&frames, "push"),
        None,
        "overflow must not deliver only a prefix of the user's destinations"
    );
}

#[test]
fn capture_invalidates_the_whole_set_after_an_oversized_destination() {
    let target = format!(
        "https://host.test/{}",
        "x".repeat(MAX_TARGET_BYTES - "https://host.test/".len())
    );
    assert_eq!(target.len(), MAX_TARGET_BYTES);
    assert!(captured_targets(&[http_frame(&target)], "push").is_some());
    assert_eq!(
        captured_targets(
            &[
                http_frame("https://host.test/first"),
                http_frame(&(target + "x"))
            ],
            "push"
        ),
        None
    );
}

#[test]
fn capture_ignores_nested_transport_and_non_push_commands() {
    let mut child = http_frame("https://host.test/nested");
    child["sid"] = json!("root/child");
    assert_eq!(captured_targets(&[child], "push"), Some(Vec::new()));
    assert_eq!(
        captured_targets(&[http_frame("https://host.test/fetch")], "fetch"),
        Some(Vec::new())
    );
}

#[test]
fn capture_uses_gits_resolved_push_name_for_aliases() {
    let target = "https://host.test/repo";
    assert_eq!(
        captured_targets(
            &[
                json!({"event":"cmd_name","sid":"root","name":"push"}),
                http_frame(target)
            ],
            "publish"
        ),
        Some(vec![target.to_owned()])
    );
}

#[test]
fn capture_accepts_only_the_direct_push_child_of_a_git_alias() {
    let alias = json!({"event":"child_start","sid":"root","child_class":"git_alias","use_shell":false,"argv":["git","push","origin"]});
    let name = json!({"event":"cmd_name","sid":"root/push","name":"push"});
    let mut transport = http_frame("https://host.test/alias");
    transport["sid"] = json!("root/push");
    assert_eq!(
        captured_targets(&[alias.clone(), name.clone(), transport.clone()], "publish"),
        Some(vec!["https://host.test/alias".to_owned()])
    );
    assert_eq!(
        captured_targets(&[name, transport.clone()], "publish"),
        Some(Vec::new())
    );
    transport["sid"] = json!("root/push/nested");
    assert_eq!(
        captured_targets(&[alias, transport], "publish"),
        Some(Vec::new())
    );
}

#[test]
fn capture_does_not_guess_the_worktree_of_an_alias_with_extra_global_options() {
    let alias = json!({"event":"child_start","sid":"root","child_class":"git_alias","use_shell":false,"argv":["git","-C","elsewhere","push","origin"]});
    let name = json!({"event":"cmd_name","sid":"root/push","name":"push"});
    let mut transport = http_frame("https://host.test/alias");
    transport["sid"] = json!("root/push");
    assert_eq!(
        captured_targets(&[alias, name, transport], "publish"),
        Some(Vec::new())
    );
}
