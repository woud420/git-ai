use std::fs;
use std::path::Path;

fn module_names(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let raw_start =
            i + usize::from(matches!(bytes[i], b'b' | b'c') && bytes.get(i + 1) == Some(&b'r'));
        if bytes[i..].starts_with(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if bytes[raw_start] == b'r'
            && bytes
                .get(raw_start + 1)
                .is_some_and(|b| matches!(b, b'#' | b'"'))
        {
            let mut quote = raw_start + 1;
            while bytes.get(quote) == Some(&b'#') {
                quote += 1;
            }
            if bytes.get(quote) == Some(&b'"') {
                let terminator = format!("\"{}", "#".repeat(quote - raw_start - 1));
                i = source[quote + 1..]
                    .find(&terminator)
                    .map_or(bytes.len(), |end| quote + 1 + end + terminator.len());
                tokens.push("");
            } else {
                // Raw identifiers use the same module name in libtest filters.
                i += 2;
            }
        } else if bytes[i] == b'\''
            && (bytes.get(i + 1) == Some(&b'\\') || source[i + 1..].chars().nth(1) == Some('\''))
        {
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                i += 1;
                if current == b'\\' {
                    i = (i + 1).min(bytes.len());
                } else if current == b'\'' {
                    break;
                }
            }
            tokens.push("");
        } else if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                i += 1;
                if current == b'\\' {
                    i = (i + 1).min(bytes.len());
                } else if current == b'"' {
                    break;
                }
            }
            tokens.push("");
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push(&source[start..i]);
        } else {
            if matches!(bytes[i], b';' | b'{' | b'}' | b'!' | b'(' | b')') {
                tokens.push(&source[i..i + 1]);
            }
            i += 1;
        }
    }
    tokens
        .windows(3)
        .filter(|tokens| tokens[0] == "mod" && matches!(tokens[2], ";" | "{"))
        .map(|tokens| tokens[1])
        .collect()
}

fn skip_collisions<'a>(child: &str, top_level: &'a [&str]) -> Vec<&'a str> {
    // libtest --skip performs substring matching, so monorepo_rebase:: also
    // matches rebase::. Exact identifier equality would miss that omission.
    top_level
        .iter()
        .copied()
        .filter(|top| child.ends_with(top))
        .collect()
}

fn collect_module_collisions(directory: &Path, top_level: &[&str], collisions: &mut Vec<String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_module_collisions(&path, top_level, collisions);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path.file_name().is_none_or(|name| name != "main.rs")
        {
            let source = fs::read_to_string(&path).unwrap();
            for child in module_names(&source) {
                for skipped in skip_collisions(child, top_level) {
                    collisions.push(format!(
                        "{}: nested mod {child} matches --skip {skipped}::",
                        path.display()
                    ));
                }
            }
        }
    }
}

#[test]
fn nested_integration_modules_cannot_match_top_level_shard_skips() {
    let integration = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/integration");
    let main = fs::read_to_string(integration.join("main.rs")).unwrap();
    let top_level = module_names(&main);
    assert!(
        !top_level.is_empty(),
        "integration module inventory is empty"
    );
    let mut collisions = Vec::new();
    // Top-level suffix collisions predate module extraction; this guards the
    // child declarations introduced when suites are subdivided.
    collect_module_collisions(&integration, &top_level, &mut collisions);
    collisions.sort();
    assert!(
        collisions.is_empty(),
        "nested integration modules would be silently skipped by CI:\n{}",
        collisions.join("\n")
    );
}

#[test]
fn shard_skip_matching_checks_suffixes_without_rejecting_safe_names() {
    let top_level = ["rebase", "stats", "gemini"];
    assert_eq!(skip_collisions("rebase", &top_level), ["rebase"]);
    assert_eq!(skip_collisions("monorepo_rebase", &top_level), ["rebase"]);
    assert_eq!(skip_collisions("deletion_stats", &top_level), ["stats"]);
    assert!(skip_collisions("rebase_invocations", &top_level).is_empty());
    assert!(skip_collisions("gemini_presets", &top_level).is_empty());
}

#[test]
fn module_inventory_ignores_comments_and_fixture_strings() {
    let source = r###"
        // mod line_comment;
        /* mod block_comment; /* mod nested_comment; */ */
        const TEXT: &str = "mod quoted; \"mod escaped;\"";
        const RAW: &str = r##"mod raw; \""##;
        const BYTES: &[u8] = br#"mod bytes; " mod raw_bytes;"#;
        const C_STRING: &CStr = cr#"mod c_string; " mod raw_c_string;"#;
        #[path = "mod attribute;"]
        pub(crate) mod declared;
        mod inline { mod nested; }
        mod r#raw_identifier;
        const BYTE_QUOTE: u8 = b'"';
        const QUOTE: char = '"';
        const ESCAPED: char = '\'';
        const UNICODE: char = 'é';
        fn lifetime<'a>(_: &'a str) {}
        mod after_lifetime;
    "###;
    assert_eq!(
        module_names(source),
        [
            "declared",
            "inline",
            "nested",
            "raw_identifier",
            "after_lifetime"
        ]
    );
}
