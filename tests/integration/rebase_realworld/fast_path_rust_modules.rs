use super::{
    ExpectedLineExt, TestRepo, assert_blame_at_commit, assert_blame_sample_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, assert_note_no_forbidden_files,
    get_commit_chain,
};

#[test]
fn test_fast_path_rust_library_5_modules() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("src/lib.rs");
    init.set_contents(crate::lines!["// Rust library crate root"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a new Rust module ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: src/parser.rs
    let mut f1 = repo.filename("src/parser.rs");
    f1.set_contents(crate::lines![
        "pub struct Parser {".ai(),
        "    input: String,".ai(),
        "    pos: usize,".ai(),
        "}".ai(),
        "impl Parser {".ai(),
        "    pub fn new(input: &str) -> Self {".ai(),
        "        Self { input: input.to_string(), pos: 0 }".ai(),
        "    }".ai(),
        "    pub fn parse_token(&mut self) -> Option<&str> {".ai(),
        "        let start = self.pos;".ai(),
        "        while self.pos < self.input.len() && !self.input.as_bytes()[self.pos].is_ascii_whitespace() { self.pos += 1; }".ai(),
        "        if start == self.pos { None } else { Some(&self.input[start..self.pos]) }".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add parser module")
        .unwrap();

    // C2: src/validator.rs
    let mut f2 = repo.filename("src/validator.rs");
    f2.set_contents(crate::lines![
        "pub struct Validator {".ai(),
        "    rules: Vec<Box<dyn Fn(&str) -> bool>>,".ai(),
        "}".ai(),
        "impl Validator {".ai(),
        "    pub fn new() -> Self {".ai(),
        "        Self { rules: Vec::new() }".ai(),
        "    }".ai(),
        "    pub fn add_rule(&mut self, rule: impl Fn(&str) -> bool + 'static) {".ai(),
        "        self.rules.push(Box::new(rule));".ai(),
        "    }".ai(),
        "    pub fn validate(&self, input: &str) -> bool {".ai(),
        "        self.rules.iter().all(|r| r(input))".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add validator module")
        .unwrap();

    // C3: src/formatter.rs
    let mut f3 = repo.filename("src/formatter.rs");
    f3.set_contents(crate::lines![
        "pub struct Formatter {".ai(),
        "    indent: usize,".ai(),
        "    style: FormatterStyle,".ai(),
        "}".ai(),
        "pub enum FormatterStyle { Compact, Pretty }".ai(),
        "impl Formatter {".ai(),
        "    pub fn new(indent: usize, style: FormatterStyle) -> Self {".ai(),
        "        Self { indent, style }".ai(),
        "    }".ai(),
        "    pub fn format(&self, tokens: &[&str]) -> String {".ai(),
        "        let sep = match self.style { FormatterStyle::Compact => \"\", FormatterStyle::Pretty => \"\\n\" };".ai(),
        "        tokens.join(sep)".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add formatter module")
        .unwrap();

    // C4: src/encoder.rs
    let mut f4 = repo.filename("src/encoder.rs");
    f4.set_contents(crate::lines![
        "pub struct Encoder {".ai(),
        "    buffer: Vec<u8>,".ai(),
        "}".ai(),
        "impl Encoder {".ai(),
        "    pub fn new() -> Self {".ai(),
        "        Self { buffer: Vec::new() }".ai(),
        "    }".ai(),
        "    pub fn encode_str(&mut self, s: &str) {".ai(),
        "        let len = s.len() as u32;".ai(),
        "        self.buffer.extend_from_slice(&len.to_le_bytes());".ai(),
        "        self.buffer.extend_from_slice(s.as_bytes());".ai(),
        "    }".ai(),
        "    pub fn finish(self) -> Vec<u8> { self.buffer }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add encoder module")
        .unwrap();

    // C5: src/decoder.rs
    let mut f5 = repo.filename("src/decoder.rs");
    f5.set_contents(crate::lines![
        "pub struct Decoder<'a> {".ai(),
        "    data: &'a [u8],".ai(),
        "    pos: usize,".ai(),
        "}".ai(),
        "impl<'a> Decoder<'a> {".ai(),
        "    pub fn new(data: &'a [u8]) -> Self {".ai(),
        "        Self { data, pos: 0 }".ai(),
        "    }".ai(),
        "    pub fn decode_str(&mut self) -> Option<&'a str> {".ai(),
        "        if self.pos + 4 > self.data.len() { return None; }".ai(),
        "        let len = u32::from_le_bytes(self.data[self.pos..self.pos+4].try_into().ok()?) as usize;".ai(),
        "        self.pos += 4;".ai(),
        "        let s = std::str::from_utf8(&self.data[self.pos..self.pos+len]).ok()?;".ai(),
        "        self.pos += len; Some(s)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add decoder module")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "build: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "build.rs",
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
        "build: add build script",
    );
    repo.commit_untracked_file("benches/bench.rs",
        "use criterion::{criterion_group, criterion_main, Criterion};\nfn bench(_c: &mut Criterion) {}\ncriterion_group!(benches, bench);\ncriterion_main!(benches);\n",
        "bench: add criterion benchmark stub",
    );
    repo.commit_untracked_file(
        "examples/demo.rs",
        "fn main() { println!(\"demo\"); }\n",
        "examples: add demo",
    );
    repo.commit_untracked_file(
        "tests/integration.rs",
        "#[test]\nfn integration_placeholder() { assert!(true); }\n",
        "test: add integration test placeholder",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only src/parser.rs
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["src/parser.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "src/validator.rs",
            "src/formatter.rs",
            "src/encoder.rs",
            "src/decoder.rs",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "src/parser.rs",
        "sha0_blame",
        &[
            ("pub struct Parser {", true),
            ("input: String,", true),
            ("pos: usize,", true),
            ("}", true),
            ("impl Parser {", true),
            ("pub fn new(input: &str) -> Self {", true),
            ("Self { input: input.to_string(), pos: 0 }", true),
            ("}", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
            ("let start = self.pos;", true),
            ("while self.pos < self.input.len()", true),
            ("if start == self.pos", true),
            ("}", true),
            ("}", true),
        ],
    );

    // sha1 = C2': validator
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["src/validator.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["src/formatter.rs", "src/encoder.rs", "src/decoder.rs"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "src/validator.rs",
        "sha1_blame",
        &[
            ("pub struct Validator {", true),
            ("rules: Vec<Box<dyn Fn(&str) -> bool>>,", true),
            ("}", true),
            ("impl Validator {", true),
            ("pub fn new() -> Self {", true),
            ("Self { rules: Vec::new() }", true),
            ("}", true),
            (
                "pub fn add_rule(&mut self, rule: impl Fn(&str) -> bool + 'static) {",
                true,
            ),
            ("self.rules.push(Box::new(rule));", true),
            ("}", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
            ("self.rules.iter().all(|r| r(input))", true),
            ("}", true),
            ("}", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/parser.rs",
        "chain1_prior_parser_rs",
        &[
            ("pub struct Parser {", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
        ],
    );

    // sha2 = C3': formatter
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/formatter.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["src/encoder.rs", "src/decoder.rs"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/parser.rs",
        "chain2_prior_parser_rs",
        &[
            ("pub struct Parser {", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/validator.rs",
        "chain2_prior_validator_rs",
        &[
            ("pub struct Validator {", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
        ],
    );

    // sha3 = C4': encoder
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["src/encoder.rs"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["src/decoder.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/parser.rs",
        "chain3_prior_parser_rs",
        &[
            ("pub struct Parser {", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/validator.rs",
        "chain3_prior_validator_rs",
        &[
            ("pub struct Validator {", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/formatter.rs",
        "chain3_prior_formatter_rs",
        &[
            ("pub struct Formatter {", true),
            ("pub fn format(&self, tokens: &[&str]) -> String {", true),
        ],
    );

    // sha4 = C5': decoder
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["src/decoder.rs"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "src/decoder.rs",
        "sha4_blame",
        &[
            ("pub struct Decoder<'a> {", true),
            ("data: &'a [u8],", true),
            ("pos: usize,", true),
            ("}", true),
            ("impl<'a> Decoder<'a> {", true),
            ("pub fn new(data: &'a [u8]) -> Self {", true),
            ("Self { data, pos: 0 }", true),
            ("}", true),
            ("pub fn decode_str(&mut self) -> Option<&'a str> {", true),
            ("if self.pos + 4 > self.data.len()", true),
            ("let len = u32::from_le_bytes", true),
            ("self.pos += 4;", true),
            ("let s = std::str::from_utf8", true),
            ("self.pos += len; Some(s)", true),
            ("}", true),
        ],
    );
    // Verify C1's file (src/parser.rs) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/parser.rs",
        "sha4_parser_preserved",
        &[
            ("pub struct Parser {", true),
            ("pub fn new(input: &str) -> Self {", true),
            ("pub fn parse_token(&mut self)", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/validator.rs",
        "chain4_prior_validator_rs",
        &[
            ("pub struct Validator {", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/formatter.rs",
        "chain4_prior_formatter_rs",
        &[
            ("pub struct Formatter {", true),
            ("pub fn format(&self, tokens: &[&str]) -> String {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/encoder.rs",
        "chain4_prior_encoder_rs",
        &[
            ("pub struct Encoder {", true),
            ("pub fn encode_str(&mut self, s: &str) {", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_fast_path_rust_library_5_modules,);
