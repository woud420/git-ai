use super::{
    ExpectedLineExt, TestRepo, assert_blame_sample_at_commit, assert_note_base_commit_matches,
    assert_note_files_exact, get_commit_chain,
};

/// Test 9: Large function blocks with 20-line license header prepended.
/// Feature adds 15-AI-line functions to processor.rs per commit.
/// Line offsets shift by 20 after rebase. Blame at sha0 checks the offset.
#[test]
fn test_slow_path_large_function_blocks_line_offset() {
    let repo = TestRepo::new();

    // Initial: processor.rs with two human lines + trailing newline
    repo.commit_untracked_file(
        "processor.rs",
        "// Processor module\nuse std::io;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend a 20-line license header (forces slow path, creates big line offset)
    repo.commit_untracked_file(
        "processor.rs",
        concat!(
            "// Copyright 2024 MyOrg. All rights reserved.\n",
            "// \n",
            "// Redistribution and use in source and binary forms,\n",
            "// with or without modification, are permitted provided\n",
            "// that the following conditions are met:\n",
            "// \n",
            "//   1. Redistributions of source code must retain the\n",
            "//      above copyright notice, this list of conditions\n",
            "//      and the following disclaimer.\n",
            "// \n",
            "//   2. Redistributions in binary form must reproduce the\n",
            "//      above copyright notice, this list of conditions\n",
            "//      and the following disclaimer in the documentation.\n",
            "// \n",
            "// THIS SOFTWARE IS PROVIDED 'AS IS' WITHOUT WARRANTY\n",
            "// OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT\n",
            "// LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS\n",
            "// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.\n",
            "// See the License file for details.\n",
            "//\n",
            "// Processor module\n",
            "use std::io;\n",
        ),
        "main: prepend 20-line license header to processor.rs",
    );
    repo.commit_untracked_file(
        "error.rs",
        "#[derive(Debug)] pub enum ProcessError { Io(std::io::Error), Invalid(String) }\n",
        "main: add error types",
    );
    repo.commit_untracked_file("types.rs",
        "pub type Bytes = Vec<u8>;\npub type Result<T> = std::result::Result<T, crate::error::ProcessError>;\n",
        "main: add common types",
    );
    repo.commit_untracked_file(
        "tests/smoke.rs",
        "#[test] fn smoke() { assert!(true); }\n",
        "main: add smoke test",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"processor\"\nversion = \"0.1.0\"\n",
        "main: add Cargo.toml",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: add 15-AI-line function process_batch to processor.rs
    let mut proc = repo.filename("processor.rs");
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    if data.is_empty() {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty input\"));".ai(),
        "    }".ai(),
        "    let mut out = Vec::with_capacity(data.len());".ai(),
        "    for &byte in data {".ai(),
        "        let processed = if byte.is_ascii_uppercase() {".ai(),
        "            byte.to_ascii_lowercase()".ai(),
        "        } else if byte.is_ascii_lowercase() {".ai(),
        "            byte.to_ascii_uppercase()".ai(),
        "        } else {".ai(),
        "            byte".ai(),
        "        };".ai(),
        "        out.push(processed);".ai(),
        "    }".ai(),
        "    Ok(out)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add process_batch function")
        .unwrap();

    // C2: add 15-AI-line function validate_input
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    if data.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty\")); }".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.is_empty() {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"data is empty\"));".ai(),
        "    }".ai(),
        "    if data.len() > max_len {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,".ai(),
        "            format!(\"data length {} exceeds max {}\", data.len(), max_len)));".ai(),
        "    }".ai(),
        "    if data.iter().any(|&b| b == 0) {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"null byte\"));".ai(),
        "    }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add validate_input function")
        .unwrap();

    // C3: add 15-AI-line function chunk_data
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty\")); }".ai(),
        "    if data.len() > max_len { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"too long\")); }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn chunk_data(data: &[u8], chunk_size: usize) -> Vec<&[u8]> {".ai(),
        "    if chunk_size == 0 { return Vec::new(); }".ai(),
        "    let n = (data.len() + chunk_size - 1) / chunk_size;".ai(),
        "    let mut chunks = Vec::with_capacity(n);".ai(),
        "    let mut offset = 0;".ai(),
        "    while offset < data.len() {".ai(),
        "        let end = (offset + chunk_size).min(data.len());".ai(),
        "        chunks.push(&data[offset..end]);".ai(),
        "        offset += chunk_size;".ai(),
        "    }".ai(),
        "    chunks".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add chunk_data function")
        .unwrap();

    // C4: add 15-AI-line function compress
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty\")); }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
        "pub fn chunk_data(data: &[u8], size: usize) -> Vec<&[u8]> {".ai(),
        "    if size == 0 { return Vec::new(); }".ai(),
        "    (0..data.len()).step_by(size).map(|i| &data[i..(i + size).min(data.len())]).collect()".ai(),
        "}".ai(),
        "".ai(),
        "pub fn run_length_encode(data: &[u8]) -> Vec<(u8, usize)> {".ai(),
        "    if data.is_empty() { return Vec::new(); }".ai(),
        "    let mut result = Vec::new();".ai(),
        "    let mut current = data[0];".ai(),
        "    let mut count = 1usize;".ai(),
        "    for &b in &data[1..] {".ai(),
        "        if b == current { count += 1; }".ai(),
        "        else { result.push((current, count)); current = b; count = 1; }".ai(),
        "    }".ai(),
        "    result.push((current, count));".ai(),
        "    result".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add run_length_encode function")
        .unwrap();

    // C5: add 15-AI-line function transform_pipeline
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.len() > max_len { Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"too long\")) } else { Ok(()) }".ai(),
        "}".ai(),
        "pub fn chunk_data(data: &[u8], size: usize) -> Vec<&[u8]> {".ai(),
        "    (0..data.len()).step_by(size).map(|i| &data[i..(i+size).min(data.len())]).collect()".ai(),
        "}".ai(),
        "pub fn run_length_encode(data: &[u8]) -> Vec<(u8, usize)> {".ai(),
        "    let mut r = Vec::new(); let mut c = data[0]; let mut n = 1usize;".ai(),
        "    for &b in &data[1..] { if b == c { n += 1; } else { r.push((c, n)); c = b; n = 1; } }".ai(),
        "    r.push((c, n)); r".ai(),
        "}".ai(),
        "".ai(),
        "pub fn transform_pipeline(data: &[u8], transforms: &[fn(&[u8]) -> Vec<u8>]) -> Vec<u8> {".ai(),
        "    let mut current = data.to_vec();".ai(),
        "    for transform in transforms {".ai(),
        "        current = transform(&current);".ai(),
        "    }".ai(),
        "    current".ai(),
        "}".ai(),
        "".ai(),
        "pub fn hexdump(data: &[u8]) -> String {".ai(),
        "    data.iter().map(|b| format!(\"{:02x}\", b)).collect::<Vec<_>>().join(\" \")".ai(),
        "}".ai(),
        "".ai(),
        "pub fn count_bytes(data: &[u8]) -> std::collections::HashMap<u8, usize> {".ai(),
        "    let mut map = std::collections::HashMap::new();".ai(),
        "    for &b in data { *map.entry(b).or_insert(0) += 1; }".ai(),
        "    map".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add transform_pipeline and helpers")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': processor.rs
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["processor.rs"]);

    // sha0 blame: 20 license lines (human) + 1 "// Processor module" (human) + 1 "use std::io;" (human)
    // then the blank + function (AI lines) start
    assert_blame_sample_at_commit(
        &repo,
        &chain[0],
        "processor.rs",
        "sha0_blame_offset",
        &[
            ("// Copyright 2024 MyOrg", false),
            ("// Redistribution", false),
            ("// THIS SOFTWARE IS PROVIDED", false),
            ("// Processor module", false),
            ("use std::io;", false),
            ("pub fn process_batch", true),
        ],
    );

    // sha1 = C2': C2 added validate_input function.
    // The 20-line license header prepend shifts ALL feature lines by 20.
    // assert_blame_sample_at_commit verifies key lines across the intermediate commit,
    // confirming the line-offset accounting is correct after the upstream prepend.
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["processor.rs"]);
    // The 20-line license header shifts ALL feature lines by +20. We check that
    // known-AI lines in the intermediate commit C2′ are correctly attributed.
    // Only lines whose content also exists in the final feature tip (C5) are
    // attributable via the hunk-based content-map lookup; lines that C3/C4/C5
    // later rewrote are no longer in the content map and show as human.
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "processor.rs",
        "sha1_blame_offset",
        &[
            ("// Copyright 2024 MyOrg", false), // license header (human) — not AI
            ("// Processor module", false),     // original human header
            ("use std::io;", false),            // original human line
            ("pub fn process_batch", true),     // C1 AI line, offset +20 correctly applied
            ("pub fn validate_input", true),    // C2 AI line — function sig survived to tip
        ],
    );

    // sha2 = C3': C3 added chunk_data function
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "processor.rs",
        "sha2_chunk_data",
        &[
            ("pub fn chunk_data", true),
            ("chunk_size == 0", true),
            ("chunks.push", true),
        ],
    );

    // sha3 = C4': C4 added run_length_encode function
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "processor.rs",
        "sha3_rle",
        &[("pub fn run_length_encode", true), ("result.push", true)],
    );

    // sha4 = C5': C5 added transform_pipeline and helpers
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "processor.rs",
        "sha4_transform",
        &[
            ("pub fn transform_pipeline", true),
            ("pub fn hexdump", true),
            ("pub fn count_bytes", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_large_function_blocks_line_offset,);
