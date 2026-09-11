use super::*;

/// Test 2: compute.rs function body — feature (C2) implements a function with
/// 10 AI lines, main also implements it differently (conflict).  AI resolution
/// produces 15 merged lines (all .ai()).  Extra lines from resolution are counted.
#[test]
fn test_conflict_ai_resolves_with_added_extra_lines() {
    let repo = TestRepo::new();

    // Initial: compute.rs with a function stub (human)
    repo.commit_untracked_file(
        "src/compute.rs",
        "pub fn compute(data: &[f64]) -> f64 { 0.0 }\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: implements the function differently (human) → will conflict
    repo.commit_untracked_file("src/compute.rs",
        "pub fn compute(data: &[f64]) -> f64 {\n    data.iter().sum::<f64>() / data.len() as f64\n}\n",
        "main: implement compute as mean",
    );
    repo.commit_untracked_file(
        "src/main.rs",
        "fn main() { println!(\"hello\"); }\n",
        "main: add main",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"compute\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "src/tests.rs",
        "#[cfg(test)]\nmod tests { #[test] fn it_works() {} }\n",
        "main: add tests",
    );
    repo.commit_untracked_file(
        "README.md",
        "# compute\nA compute library.\n",
        "main: add README",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates types.rs (8 AI lines)
    let mut types = repo.filename("src/types.rs");
    types.set_contents(crate::lines![
        "#[derive(Debug, Clone, PartialEq)]".ai(),
        "pub struct DataPoint {".ai(),
        "    pub value: f64,".ai(),
        "    pub weight: f64,".ai(),
        "}".ai(),
        "".ai(),
        "impl DataPoint {".ai(),
        "    pub fn new(value: f64, weight: f64) -> Self { Self { value, weight } }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add DataPoint type")
        .unwrap();

    // C2: AI implements compute.rs with 10 AI lines — WILL CONFLICT with main's implementation
    let mut compute = repo.filename("src/compute.rs");
    compute.set_contents(crate::lines![
        "pub fn compute(data: &[f64]) -> f64 {".ai(),
        "    if data.is_empty() { return 0.0; }".ai(),
        "    let n = data.len() as f64;".ai(),
        "    let mean = data.iter().sum::<f64>() / n;".ai(),
        "    let variance = data.iter()".ai(),
        "        .map(|x| (x - mean).powi(2))".ai(),
        "        .sum::<f64>() / n;".ai(),
        "    variance.sqrt()".ai(),
        "}".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 AI implements compute as std-dev")
        .unwrap();

    // C3: AI creates validator.rs (8 AI lines)
    let mut validator = repo.filename("src/validator.rs");
    validator.set_contents(crate::lines![
        "pub fn validate_data(data: &[f64]) -> Result<(), String> {".ai(),
        "    if data.is_empty() { return Err(\"empty data\".into()); }".ai(),
        "    if data.iter().any(|x| x.is_nan()) { return Err(\"NaN in data\".into()); }".ai(),
        "    if data.iter().any(|x| x.is_infinite()) { return Err(\"Inf in data\".into()); }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn normalize(data: &mut Vec<f64>) { let m = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max); data.iter_mut().for_each(|x| *x /= m); }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add data validator")
        .unwrap();

    // C4: AI creates encoder.rs (8 AI lines)
    let mut encoder = repo.filename("src/encoder.rs");
    encoder.set_contents(crate::lines![
        "pub fn encode(data: &[f64]) -> Vec<u8> {".ai(),
        "    data.iter()".ai(),
        "        .flat_map(|x| x.to_le_bytes())".ai(),
        "        .collect()".ai(),
        "}".ai(),
        "".ai(),
        "pub fn decode(bytes: &[u8]) -> Vec<f64> {".ai(),
        "    bytes.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()"
            .ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add encoder").unwrap();

    // C5: AI creates decoder.rs (8 AI lines)
    let mut decoder = repo.filename("src/decoder.rs");
    decoder.set_contents(crate::lines![
        "use crate::encoder::decode;".ai(),
        "".ai(),
        "pub struct Decoder {".ai(),
        "    buffer: Vec<u8>,".ai(),
        "}".ai(),
        "".ai(),
        "impl Decoder {".ai(),
        "    pub fn new() -> Self { Self { buffer: Vec::new() } }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add decoder struct")
        .unwrap();

    // Rebase onto main — C2 will conflict on src/compute.rs
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/compute.rs at C2"
    );

    // AI resolves: writes a 15-line merged implementation (all .ai())
    let mut conflict_compute = repo.filename("src/compute.rs");
    conflict_compute.set_contents(crate::lines![
        "pub fn compute(data: &[f64]) -> f64 {".ai(),
        "    if data.is_empty() { return 0.0; }".ai(),
        "    let n = data.len() as f64;".ai(),
        "    let mean = data.iter().sum::<f64>() / n;".ai(),
        "    let variance = data.iter()".ai(),
        "        .map(|x| (x - mean).powi(2))".ai(),
        "        .sum::<f64>() / n;".ai(),
        "    let std_dev = variance.sqrt();".ai(),
        "    // Also return weighted mean as a combined metric".ai(),
        "    let weighted_sum: f64 = data.iter().enumerate().map(|(i, x)| x * (i + 1) as f64).sum();".ai(),
        "    let weight_total: f64 = (1..=data.len()).map(|i| i as f64).sum();".ai(),
        "    let weighted_mean = weighted_sum / weight_total;".ai(),
        "    std_dev * 0.5 + weighted_mean * 0.5".ai(),
        "}".ai(),
        "".ai(),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': types.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/types.rs"]);

    // C2': compute.rs only (AI-resolved)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/compute.rs"]);

    // blame at chain[1] for compute.rs:
    // Line 1 and "}" are unchanged from C2's parent (main branch version),
    // so git-blame traces them to the main branch commit (no note → human).
    // All other lines are new in C2' and the AI checkpoint captured them → AI.
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "src/compute.rs",
        "c2_blame_compute",
        &[
            ("pub fn compute", false), // unchanged from parent, traces to main branch commit (human)
            ("is_empty", true),        // new in C2', AI per checkpoint
            ("let n =", true),
            ("let mean =", true),
            ("variance = data", true),
            (".map(|x|", true),
            (".sum::<f64>", true),
            ("let std_dev", true), // new in C2', AI per checkpoint
            ("weighted mean", true),
            ("weighted_sum", true),
            ("weight_total:", true),
            ("weighted_mean =", true),
            ("std_dev * 0.5", true),
            ("}", false), // unchanged from parent ("}" line), traces to main branch (human)
        ],
    );

    // C3': validator.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/validator.rs"]);

    // C4': encoder.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/encoder.rs"]);

    // C5': decoder.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/decoder.rs"]);
}

crate::reuse_tests_in_worktree!(test_conflict_ai_resolves_with_added_extra_lines,);
