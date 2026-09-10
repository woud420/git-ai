use super::support::{Fixture, option, remove, replace};

#[test]
fn jj_debug_observe_cli_requires_exact_options_and_unambiguous_values() {
    let fixture = Fixture::new();
    for flag in [
        "--journal",
        "--expect-source",
        "--expect-initialization-receipt",
        "--expect-baseline",
        "--expect-generation",
        "--expect-head",
        "--expect-workspace",
        "--expect-attachment",
    ] {
        let mut missing = fixture.args();
        remove(&mut missing, flag);
        fixture.rejected(&missing);
        missing.push(flag.to_owned());
        fixture.rejected(&missing);
        let mut duplicate = fixture.args();
        let index = duplicate.iter().position(|arg| arg == flag).unwrap();
        let pair = duplicate[index..index + 2].to_vec();
        duplicate.extend(pair);
        fixture.rejected(&duplicate);
        if flag != "--expect-workspace" {
            let mut option_value = fixture.args();
            replace(&mut option_value, flag, "--json");
            fixture.rejected(&option_value);
        }
    }
    let mut no_json = fixture.args();
    no_json.retain(|arg| arg != "--json");
    fixture.rejected(&no_json);
    for extra in [
        vec!["--json"],
        vec!["--unknown"],
        vec!["extra"],
        vec!["--help"],
        vec!["-h"],
        vec!["--"],
        vec!["--source", &"11".repeat(32)],
        vec!["--admission", &"22".repeat(32)],
    ] {
        let mut args = fixture.args();
        args.extend(extra.into_iter().map(str::to_owned));
        fixture.rejected(&args);
    }
    for flag in [
        "--journal",
        "--expect-workspace",
        "--expect-attachment",
        "--attempts",
        "--interval-ms",
    ] {
        let mut args = fixture.args();
        if args.iter().any(|arg| arg == flag) {
            remove(&mut args, flag);
        }
        args.push(format!("{flag}=value"));
        fixture.rejected(&args);
    }
    for args in [
        vec!["observe"],
        vec!["observe", "--json"],
        vec!["observe", "--help", "--json"],
    ] {
        fixture.rejected(&args.into_iter().map(str::to_owned).collect::<Vec<_>>());
    }
}

#[test]
fn jj_debug_observe_cli_rejects_noncanonical_or_out_of_range_controls() {
    let fixture = Fixture::new();
    for (flag, bounds) in [
        ("--attempts", ["0", "33"]),
        ("--interval-ms", ["249", "60001"]),
    ] {
        for bad in bounds.into_iter().chain([
            "",
            "-1",
            "+1",
            "01",
            " 1",
            "1 ",
            "1.0",
            "1e3",
            "١",
            "18446744073709551616",
        ]) {
            let mut args = fixture.args();
            option(&mut args, flag, bad);
            fixture.rejected(&args);
        }
        let valid = if flag == "--attempts" { "1" } else { "250" };
        let mut duplicate = fixture.args();
        option(&mut duplicate, flag, valid);
        option(&mut duplicate, flag, valid);
        fixture.rejected(&duplicate);
        let mut missing = fixture.args();
        missing.push(flag.to_owned());
        fixture.rejected(&missing);
        let mut option_value = fixture.args();
        option(&mut option_value, flag, "--json");
        fixture.rejected(&option_value);
    }
    for bad in ["0", "0250", "+250", "250 ", " 250"] {
        let mut args = fixture.args();
        option(&mut args, "--interval-ms", bad);
        fixture.rejected(&args);
    }
}

#[test]
fn jj_debug_observe_cli_retains_capture_id_generation_and_head_rules() {
    let fixture = Fixture::new();
    for flag in [
        "--expect-source",
        "--expect-initialization-receipt",
        "--expect-baseline",
        "--expect-attachment",
    ] {
        for bad in [
            "".into(),
            "1".repeat(63),
            "1".repeat(65),
            "AA".repeat(32),
            "gg".repeat(32),
        ] {
            let mut args = fixture.args();
            replace(&mut args, flag, bad);
            fixture.rejected(&args);
        }
    }
    for bad in [
        "",
        "-1",
        "+1",
        "01",
        " 1",
        "1 ",
        "1.0",
        "١",
        "9223372036854775807",
        "18446744073709551616",
    ] {
        let mut args = fixture.args();
        replace(&mut args, "--expect-generation", bad);
        fixture.rejected(&args);
    }
    for bad in [
        "".into(),
        "0".repeat(128),
        "1".repeat(127),
        "1".repeat(129),
        "AA".repeat(64),
        "gg".repeat(64),
    ] {
        let mut args = fixture.args();
        replace(&mut args, "--expect-head", bad);
        fixture.rejected(&args);
    }
    let mut over_heads = fixture.args();
    for id in 1..=32 {
        option(&mut over_heads, "--expect-head", &format!("{id:0128x}"));
    }
    assert_eq!(over_heads.len(), 82);
    fixture.rejected(&over_heads);
    over_heads.extend((0..7).map(|_| "extra".to_owned()));
    assert_eq!(over_heads.len(), 89);
    fixture.rejected(&over_heads);
}

#[test]
fn jj_debug_observe_cli_workspace_limit_counts_utf8_bytes() {
    let fixture = Fixture::new();
    for bad in [
        String::new(),
        "w".repeat(16 * 1024 + 1),
        format!("{}a", "é".repeat(8 * 1024)),
    ] {
        let mut args = fixture.args();
        replace(&mut args, "--expect-workspace", bad);
        fixture.rejected(&args);
    }
}

#[test]
fn jj_debug_observe_cli_options_are_irrelevant_to_existing_actions() {
    let fixture = Fixture::new();
    let observe = fixture.args();
    let mut capture = observe.clone();
    capture[0] = "capture".into();
    remove(&mut capture, "--expect-workspace");
    remove(&mut capture, "--expect-attachment");
    let base = observe[..4].to_vec();
    let mut status = base.clone();
    status[0] = "status".into();
    let mut initialize = base.clone();
    initialize[0] = "initialize".into();
    let mut receipt = base;
    receipt[0] = "receipt".into();
    option(&mut receipt, "--source", &"11".repeat(32));
    option(&mut receipt, "--admission", &"22".repeat(32));
    for base in [status, receipt, initialize, capture] {
        for (flag, value) in [
            ("--attempts", "1".to_owned()),
            ("--interval-ms", "250".to_owned()),
            ("--expect-workspace", "default".to_owned()),
            ("--expect-attachment", "55".repeat(32)),
        ] {
            let mut args = base.clone();
            option(&mut args, flag, &value);
            fixture.rejected(&args);
        }
    }
}
