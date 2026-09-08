#[path = "jj_debug_observer_cli_args.rs"]
mod args;
#[path = "jj_debug_observer_cli_support.rs"]
mod support;

use support::{ACTIONS, Fixture};

#[test]
fn jj_debug_observer_cli_help_describes_explicit_daemon_controls() {
    let fixture = Fixture::new();
    let mut prefixes = vec![vec![], vec!["observer"]];
    prefixes.extend(ACTIONS.map(|action| vec!["observer", action]));
    for prefix in prefixes {
        for flag in ["--help", "-h"] {
            let mut args = prefix.clone();
            args.push(flag);
            let output = fixture.command(&args).output().unwrap();
            assert!(output.status.success());
            assert!(output.stdout.len() <= 32 * 1024);
            let text = String::from_utf8(output.stdout)
                .unwrap()
                .to_ascii_lowercase();
            for word in [
                "observer",
                "enable",
                "status",
                "disable",
                "resume",
                "--journal",
                "--json",
                "experimental",
                "daemon",
                "attribution",
                "disabled",
            ] {
                assert!(text.contains(word), "help lacks {word}");
            }
            fixture.preserved();
        }
    }
}

#[test]
fn jj_debug_observer_cli_valid_syntax_reaches_only_the_isolated_missing_daemon() {
    let fixture = Fixture::new();
    for action in ACTIONS {
        fixture.accepted(&fixture.args(action));
    }
    for path in [
        fixture.journal(),
        "./literal journal.sqlite".into(),
        "./workspace-工/é.sqlite".into(),
    ] {
        fixture.accepted(&["observer", "enable", "--json", "--journal", &path]);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn jj_debug_observer_cli_unsupported_precedes_config_paths_and_daemon_access() {
    let fixture = Fixture::new();
    for action in ACTIONS {
        let output = fixture.poisoned(&fixture.args(action)).output().unwrap();
        fixture.check(output, "unsupported_platform");
    }
}
