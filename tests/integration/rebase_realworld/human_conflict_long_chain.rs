use super::*;

/// Test 10: Rust 7-commit chain — feature adds AI functions across multiple
/// files; main edits shared.rs causing conflict on C4 (middle of a 7-commit
/// chain).  Verifies 7-element chain: C1'–C3' accumulate normally, C4' loses
/// shared.rs, C5'–C7' continue accumulating math.rs, string_utils.rs, io.rs.
#[test]
fn test_human_conflict_rust_7_commit_chain_c4_conflict_surroundings_intact() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/shared.rs",
        "pub fn identity<T>(x: T) -> T { x }\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds a constant and a function to shared.rs → conflict with feature C4 edit
    repo.commit_untracked_file(
        "src/shared.rs",
        "pub const VERSION: &str = \"1.0\";\npub fn identity<T>(x: T) -> T { x }\n",
        "main: add VERSION constant to shared.rs",
    );
    repo.commit_untracked_file(
        "src/log.rs",
        "pub fn log(msg: &str) { eprintln!(\"{}\", msg); }\n",
        "main: add log",
    );
    repo.commit_untracked_file("src/env.rs",
        "pub fn env_or(key: &str, default: &str) -> String { std::env::var(key).unwrap_or_else(|_| default.to_string()) }\n",
        "main: add env helper",
    );
    repo.commit_untracked_file("src/fs_utils.rs",
        "pub fn read_to_string(path: &str) -> std::io::Result<String> { std::fs::read_to_string(path) }\n",
        "main: add fs_utils",
    );
    repo.commit_untracked_file("src/assert_utils.rs",
        "pub fn assert_non_empty(s: &str) { assert!(!s.is_empty(), \"expected non-empty string\"); }\n",
        "main: add assert_utils",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates src/vec_utils.rs
    let mut vec_utils = repo.filename("src/vec_utils.rs");
    vec_utils.set_contents(crate::lines![
        "pub fn dedup<T: Eq + std::hash::Hash + Clone>(v: &[T]) -> Vec<T> {".ai(),
        "    let mut seen = std::collections::HashSet::new();".ai(),
        "    v.iter().filter(|x| seen.insert((*x).clone())).cloned().collect()".ai(),
        "}".ai(),
        "pub fn flatten<T: Clone>(nested: &[Vec<T>]) -> Vec<T> {".ai(),
        "    nested.iter().flat_map(|v| v.iter().cloned()).collect()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add vec_utils").unwrap();

    // C2: AI creates src/option_utils.rs
    let mut opt_utils = repo.filename("src/option_utils.rs");
    opt_utils.set_contents(crate::lines![
        "pub fn or_default<T: Default>(opt: Option<T>) -> T {".ai(),
        "    opt.unwrap_or_default()".ai(),
        "}".ai(),
        "pub fn map_or_none<T, U, F: FnOnce(T) -> Option<U>>(opt: Option<T>, f: F) -> Option<U> {"
            .ai(),
        "    opt.and_then(f)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add option_utils")
        .unwrap();

    // C3: AI creates src/result_utils.rs
    let mut result_utils = repo.filename("src/result_utils.rs");
    result_utils.set_contents(crate::lines![
        "pub fn ok_or_log<T, E: std::fmt::Display>(r: Result<T, E>, ctx: &str) -> Option<T> {".ai(),
        "    r.map_err(|e| eprintln!(\"{}: {}\", ctx, e)).ok()".ai(),
        "}".ai(),
        "pub fn map_err_string<T, E: std::fmt::Display>(r: Result<T, E>) -> Result<T, String> {"
            .ai(),
        "    r.map_err(|e| e.to_string())".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add result_utils")
        .unwrap();

    // C4: AI edits shared.rs to add a clamp function — WILL CONFLICT with main's VERSION
    let mut shared = repo.filename("src/shared.rs");
    shared.replace_at(0, "pub fn clamp<T: PartialOrd>(x: T, lo: T, hi: T) -> T { if x < lo { lo } else if x > hi { hi } else { x } }".ai());
    repo.stage_all_and_commit("feat: C4 add clamp to shared.rs")
        .unwrap();

    // C5: AI creates src/math.rs
    let mut math = repo.filename("src/math.rs");
    math.set_contents(crate::lines![
        "pub fn gcd(mut a: u64, mut b: u64) -> u64 { while b != 0 { let t = b; b = a % b; a = t; } a }".ai(),
        "pub fn lcm(a: u64, b: u64) -> u64 { a / gcd(a, b) * b }".ai(),
        "pub fn is_prime(n: u64) -> bool { if n < 2 { return false; } (2..=(n as f64).sqrt() as u64).all(|i| n % i != 0) }".ai(),
        "pub fn factorial(n: u64) -> u64 { (1..=n).product() }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add math utils")
        .unwrap();

    // C6: AI creates src/string_utils.rs
    let mut str_utils = repo.filename("src/string_utils.rs");
    str_utils.set_contents(crate::lines![
        "pub fn capitalize(s: &str) -> String {".ai(),
        "    let mut c = s.chars();".ai(),
        "    match c.next() {".ai(),
        "        None => String::new(),".ai(),
        "        Some(f) => f.to_uppercase().to_string() + c.as_str(),".ai(),
        "    }".ai(),
        "}".ai(),
        "pub fn snake_to_camel(s: &str) -> String {".ai(),
        "    s.split('_').enumerate().map(|(i, w)| if i == 0 { w.to_string() } else { capitalize(w) }).collect()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C6 add string_utils")
        .unwrap();

    // C7: AI creates src/io_utils.rs
    let mut io_utils = repo.filename("src/io_utils.rs");
    io_utils.set_contents(crate::lines![
        "use std::io::{self, BufRead};".ai(),
        "".ai(),
        "pub fn read_lines(path: &str) -> io::Result<Vec<String>> {".ai(),
        "    let file = std::fs::File::open(path)?;".ai(),
        "    let reader = io::BufReader::new(file);".ai(),
        "    reader.lines().collect()".ai(),
        "}".ai(),
        "".ai(),
        "pub fn write_lines(path: &str, lines: &[String]) -> io::Result<()> {".ai(),
        "    std::fs::write(path, lines.join(\"\\n\"))".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C7 add io_utils").unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/shared.rs at C4"
    );

    // Human resolves: keep VERSION constant and add clamp function
    fs::write(
        repo.path().join("src/shared.rs"),
        "pub const VERSION: &str = \"1.0\";\npub fn identity<T>(x: T) -> T { x }\npub fn clamp<T: PartialOrd>(x: T, lo: T, hi: T) -> T { if x < lo { lo } else if x > hi { hi } else { x } }\n",
    ).unwrap();
    repo.git(&["add", "src/shared.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 7);

    // C1': vec_utils.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/vec_utils.rs"]);

    // C2': option_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/option_utils.rs"]);

    // C3': result_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/result_utils.rs"]);

    // C4': shared.rs human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &[]);

    // C5': math.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/math.rs"]);

    // C6': string_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[5], "c6_base");
    assert_note_files_exact(&repo, &chain[5], "c6_files", &["src/string_utils.rs"]);

    // C7': io_utils.rs only
    assert_note_base_commit_matches(&repo, &chain[6], "c7_base");
    assert_note_files_exact(&repo, &chain[6], "c7_files", &["src/io_utils.rs"]);
}

crate::reuse_tests_in_worktree!(
    test_human_conflict_rust_7_commit_chain_c4_conflict_surroundings_intact,
);
