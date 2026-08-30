use crate::commit_metric_metadata::isolated_metrics_db_path;
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::extract_json_object;
use chrono::NaiveDate;
use serde_json::Value;
use std::time::{Duration, Instant};

fn seed_ai_commit(repo: &TestRepo) {
    let mut file = repo.filename("app.rs");
    file.set_contents(crate::lines!["fn main() {}", "let answer = 42;".ai()]);
    repo.stage_all_and_commit("AI commit")
        .expect("AI commit should succeed");
    file.assert_committed_lines(crate::lines![
        "fn main() {}".human(),
        "let answer = 42;".ai()
    ]);
}

/// Run `git-ai usage` against the daemon's isolated metrics database, retrying
/// until the committed activity is visible or the deadline passes.
fn usage_json(repo: &TestRepo, metrics_db_path: &str, extra_args: &[&str]) -> Value {
    let mut args = vec!["usage"];
    args.extend_from_slice(extra_args);
    args.push("--json");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        repo.sync_daemon_force();
        let result =
            repo.git_ai_with_env(&args, &[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path)]);
        if let Ok(output) = &result {
            let json = extract_json_object(output);
            if let Ok(value) = serde_json::from_str::<Value>(&json) {
                return value;
            }
        }

        if Instant::now() >= deadline {
            panic!("usage {args:?} did not return activity data: {result:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The window spans `calendar_start` to `calendar_end` as local dates. Around a
/// DST boundary, subtracting whole 24-hour periods can shift the date span by one.
fn window_span_days(value: &Value) -> i64 {
    let parse = |key: &str| {
        let text = value[key].as_str().expect("date field should be a string");
        NaiveDate::parse_from_str(text, "%Y-%m-%d").expect("date field should parse")
    };
    (parse("calendar_end") - parse("calendar_start")).num_days()
}

fn assert_window(value: &Value, expected_label: &str, expected_days: i64) {
    assert_eq!(value["period_label"].as_str().unwrap(), expected_label);
    let span = window_span_days(value);
    assert!(
        (expected_days - 1..=expected_days).contains(&span),
        "expected a {expected_days}-day window (tolerating one DST day), got span {span}"
    );
}

#[test]
fn usage_period_valid_tokens_report_their_label_and_window() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    seed_ai_commit(&repo);

    for (token, expected_label, expected_days) in [
        ("1d", "last 24 hours", 1),
        ("3d", "last 3 days", 3),
        ("7d", "last 7 days", 7),
        ("30d", "last 30 days", 30),
    ] {
        let value = usage_json(&repo, &metrics_db_path, &["--period", token]);
        assert_window(&value, expected_label, expected_days);
    }
}

#[test]
fn usage_period_accepts_the_equals_form() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    seed_ai_commit(&repo);

    let value = usage_json(&repo, &metrics_db_path, &["--period=3d"]);

    assert_window(&value, "last 3 days", 3);
}

#[test]
fn usage_without_period_defaults_to_thirty_day_window() {
    let (_metrics_db_dir, metrics_db_path) = isolated_metrics_db_path();
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_METRICS_DB_PATH", metrics_db_path.as_str())]);
    seed_ai_commit(&repo);

    let value = usage_json(&repo, &metrics_db_path, &[]);

    assert_window(&value, "last 30 days", 30);
}

#[test]
fn usage_period_invalid_token_exits_nonzero_with_message() {
    let repo = TestRepo::new();

    let err = repo
        .git_ai(&["usage", "--period", "90d"])
        .expect_err("an invalid --period value should exit non-zero");

    assert!(
        err.contains("Invalid --period value: 90d. Expected one of 1d, 3d, 7d, 30d."),
        "unexpected error output: {err}"
    );
}

#[test]
fn usage_period_missing_value_exits_nonzero_with_message() {
    let repo = TestRepo::new();

    let err = repo
        .git_ai(&["usage", "--period"])
        .expect_err("a missing --period value should exit non-zero");

    assert!(
        err.contains("Missing value for --period."),
        "unexpected error output: {err}"
    );
}

#[test]
fn usage_help_documents_period_values_and_default() {
    let repo = TestRepo::new();

    for args in [&["--help"][..], &["usage", "--help"][..]] {
        let help = repo.git_ai(args).expect("help should exit successfully");
        assert!(
            help.contains("--period <1d|3d|7d|30d>") && help.contains("default: 30d"),
            "help for {args:?} did not document the period contract:\n{help}"
        );
    }
}
