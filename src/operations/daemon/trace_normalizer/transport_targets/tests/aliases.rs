use super::*;

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
fn alias_transport_survives_child_frames_arriving_before_the_parent_alias_frame() {
    let alias = json!({"event":"child_start","sid":"root","child_class":"git_alias","use_shell":false,"argv":["git","push","origin"]});
    let name = json!({"event":"cmd_name","sid":"root/push","name":"push"});
    let mut transport = http_frame("https://host.test/alias");
    transport["sid"] = json!("root/push");
    for frames in [
        vec![name.clone(), alias.clone(), transport.clone()],
        vec![name, transport, alias],
    ] {
        assert_eq!(
            captured_targets(&frames, "publish"),
            Some(vec!["https://host.test/alias".to_owned()])
        );
    }
}

#[test]
fn delayed_alias_authorization_rejects_ambiguous_children_and_capture_overflow() {
    let alias = json!({"event":"child_start","sid":"root","child_class":"git_alias","use_shell":false,"argv":["git","push","origin"]});
    let name = json!({"event":"cmd_name","sid":"root/push","name":"push"});
    let mut frames = vec![name];
    for index in 0..MAX_TARGETS {
        let mut frame = http_frame(&format!("https://host.test/repo-{index}"));
        frame["sid"] = json!("root/push");
        frames.push(frame);
    }
    let mut authorized = frames.clone();
    authorized.push(alias.clone());
    assert_eq!(
        captured_targets(&authorized, "publish").unwrap().len(),
        MAX_TARGETS
    );
    authorized.push(alias.clone());
    assert_eq!(captured_targets(&authorized, "publish"), None);
    let mut overflow = http_frame("https://host.test/ninth");
    overflow["sid"] = json!("root/push");
    let mut over_capacity = frames.clone();
    over_capacity.extend([overflow, alias.clone()]);
    assert_eq!(captured_targets(&over_capacity, "publish"), None);
    frames.push(json!({"event":"cmd_name","sid":"root/other","name":"push"}));
    frames.push(alias);
    assert_eq!(captured_targets(&frames, "publish"), None);
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

#[test]
fn alias_candidates_cannot_authorize_shell_aliases_or_mix_with_root_transport() {
    let name = json!({"event":"cmd_name","sid":"root/push","name":"push"});
    let mut child = http_frame("https://host.test/child");
    child["sid"] = json!("root/push");
    let mut alias = json!({"event":"child_start","sid":"root","child_class":"git_alias","use_shell":true,"argv":["git","push","origin"]});
    assert_eq!(
        captured_targets(&[name.clone(), child.clone(), alias.clone()], "publish"),
        Some(Vec::new())
    );
    alias["use_shell"] = json!(false);
    assert_eq!(
        captured_targets(
            &[name, child, http_frame("https://host.test/root"), alias],
            "push"
        ),
        None
    );
}
