#[path = "jj_debug_observe_cli_args.rs"]
mod args;
#[path = "jj_debug_observe_cli_support.rs"]
mod support;

use support::{Fixture, option, replace};

#[test]
fn jj_debug_observe_cli_help_describes_finite_foreground_bounds() {
    let fixture = Fixture::new();
    for action in [None, Some("observe")] {
        for flag in ["--help", "-h"] {
            let mut args = Vec::new();
            args.extend(action.map(str::to_owned));
            args.push(flag.to_owned());
            let output = fixture.command(&args).output().unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout)
                .unwrap()
                .to_ascii_lowercase();
            for word in [
                "observe",
                "--journal",
                "--json",
                "--expect-source",
                "--expect-initialization-receipt",
                "--expect-baseline",
                "--expect-generation",
                "--expect-head",
                "--expect-workspace",
                "--expect-attachment",
                "--attempts",
                "--interval-ms",
                "finite",
                "foreground",
                "attribution",
                "disabled",
                "default",
                "1",
                "32",
                "250",
                "1000",
                "60000",
            ] {
                assert!(text.contains(word), "help lacks {word}");
            }
            fixture.preserved();
        }
    }
}

#[test]
fn jj_debug_observe_cli_accepts_default_and_bounded_control_syntax() {
    let fixture = Fixture::new();
    fixture.syntax_accepted(&fixture.args());
    for (flag, values) in [
        ("--attempts", ["1", "32"]),
        ("--interval-ms", ["250", "60000"]),
    ] {
        for value in values {
            let mut args = fixture.args();
            option(&mut args, flag, value);
            fixture.syntax_accepted(&args);
        }
    }
    for (attempts, interval) in [("1", "60000"), ("32", "250")] {
        let mut args = fixture.args();
        option(&mut args, "--attempts", attempts);
        option(&mut args, "--interval-ms", interval);
        fixture.syntax_accepted(&args);
    }
    let mut full = fixture.args();
    replace(&mut full, "--expect-generation", "9223372036854775806");
    for id in 1..=31 {
        option(&mut full, "--expect-head", &format!("{id:0128x}"));
    }
    option(&mut full, "--attempts", "32");
    option(&mut full, "--interval-ms", "60000");
    assert_eq!(full.len(), 84);
    fixture.syntax_accepted(&full);
}

#[test]
fn jj_debug_observe_cli_accepts_exact_byte_and_dash_leading_workspace_syntax() {
    let fixture = Fixture::new();
    for name in [
        "w".repeat(16 * 1024),
        "é".repeat(8 * 1024),
        "-workspace".into(),
        "--json".into(),
        "--expect-workspace".into(),
        "-h".into(),
        "--".into(),
        "--expect-workspace=literal".into(),
        "é".into(),
        "e\u{301}".into(),
    ] {
        let mut args = fixture.args();
        replace(&mut args, "--expect-workspace", name);
        fixture.syntax_accepted(&args);
    }
    for id in ["0".repeat(64), "f".repeat(64)] {
        let mut args = fixture.args();
        replace(&mut args, "--expect-attachment", id);
        fixture.syntax_accepted(&args);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[test]
fn jj_debug_observe_cli_unsupported_platform_precedes_configuration_and_paths() {
    let fixture = Fixture::new();
    let home = fixture
        .repo
        .test_home_path()
        .join("never-created-observe-home");
    let mut args = fixture.args();
    replace(
        &mut args,
        "--journal",
        home.join("journal.sqlite").to_str().unwrap(),
    );
    option(&mut args, "--attempts", "32");
    option(&mut args, "--interval-ms", "60000");
    let output = fixture
        .command(&args)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("GIT_AI_TEST_CONFIG_PATCH", "invalid-json")
        .env("GIT_CONFIG_GLOBAL", home.join("missing-config"))
        .output()
        .unwrap();
    fixture.check(output, "unsupported_platform");
    assert!(!home.exists());
}
