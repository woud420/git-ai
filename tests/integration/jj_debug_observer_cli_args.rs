use super::{ACTIONS, Fixture};

#[test]
fn jj_debug_observer_cli_requires_known_action_and_mandatory_options() {
    let fixture = Fixture::new();
    let journal = fixture.journal();
    for args in [
        vec!["observer"],
        vec!["observer", "unknown", "--json"],
        vec!["observer", "--json"],
        vec!["observer", "enable", "--json"],
        vec!["observer", "enable", "--journal"],
        vec!["observer", "enable", "--journal", &journal],
        vec!["observer", "enable", "--journal", "--json"],
        vec!["observer", "enable", "--journal", "", "--json"],
        vec!["observer", "enable", "--journal", "-missing", "--json"],
    ] {
        fixture.rejected(&args);
    }
    for action in ACTIONS {
        fixture.rejected(&["observer", action]);
    }
}

#[test]
fn jj_debug_observer_cli_rejects_duplicates_unowned_flags_and_extra_arguments() {
    let fixture = Fixture::new();
    let journal = fixture.journal();
    for action in ACTIONS {
        let valid = fixture.args(action);
        for extra in [
            vec!["--json"],
            vec!["extra"],
            vec!["--unknown"],
            vec!["--journal", &journal],
            vec!["--source", "11"],
            vec!["--expect-source", "11"],
            vec!["--expect-generation", "0"],
            vec!["--expect-head", "11"],
            vec!["--expect-workspace", "default"],
            vec!["--expect-attachment", "11"],
            vec!["--revision", "0"],
            vec!["--attempts", "1"],
            vec!["--interval-ms", "5000"],
            vec!["--json=true"],
            vec!["--", "extra"],
        ] {
            let mut args = valid.clone();
            args.extend(extra);
            fixture.rejected(&args);
        }
    }
    fixture.rejected(&["observer", "enable", "--journal=journal.sqlite", "--json"]);
    fixture.rejected(&["observer", "--json", "status"]);
}

#[test]
fn jj_debug_observer_cli_help_is_exact_and_never_masks_invalid_options() {
    let fixture = Fixture::new();
    for flag in ["--help", "-h"] {
        for args in [
            vec!["observer", flag, "--json"],
            vec!["observer", "unknown", flag],
            vec!["observer", "--json", flag],
        ] {
            fixture.rejected(&args);
        }
        for action in ACTIONS {
            fixture.rejected(&["observer", action, flag, "--json"]);
            let mut args = fixture.args(action);
            args.push(flag);
            fixture.rejected(&args);
        }
    }
}

#[test]
fn jj_debug_observer_cli_enable_rejects_sqlite_special_names_before_absolutizing() {
    let fixture = Fixture::new();
    for path in [
        ":memory:",
        "file:missing.sqlite",
        "file:/missing?immutable=1",
    ] {
        let output = fixture
            .command(&["observer", "enable", "--journal", path, "--json"])
            .output()
            .unwrap();
        let code = if cfg!(any(target_os = "linux", target_os = "macos")) {
            "journal_unavailable"
        } else {
            "unsupported_platform"
        };
        fixture.check(output, code);
    }
}
