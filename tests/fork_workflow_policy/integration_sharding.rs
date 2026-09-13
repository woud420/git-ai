use std::fs;
use std::path::Path;

fn module_names(source: &str) -> Vec<String> {
    fn flatten(stream: proc_macro2::TokenStream, tokens: &mut Vec<String>) {
        use proc_macro2::{Delimiter, TokenTree};
        for token in stream {
            match token {
                TokenTree::Group(group) => {
                    let delimiters = match group.delimiter() {
                        Delimiter::Brace => Some(("{", "}")),
                        Delimiter::Parenthesis => Some(("(", ")")),
                        _ => None,
                    };
                    if let Some((open, _)) = delimiters {
                        tokens.push(open.to_string());
                    }
                    flatten(group.stream(), tokens);
                    if let Some((_, close)) = delimiters {
                        tokens.push(close.to_string());
                    }
                }
                TokenTree::Ident(ident) => {
                    tokens.push(ident.to_string().trim_start_matches("r#").to_string());
                }
                TokenTree::Punct(punct) if ";!".contains(punct.as_char()) => {
                    tokens.push(punct.to_string());
                }
                TokenTree::Literal(_) => tokens.push(String::new()),
                TokenTree::Punct(_) => {}
            }
        }
    }
    let mut tokens = Vec::new();
    flatten(
        source.parse().expect("valid Rust source tokens"),
        &mut tokens,
    );
    tokens
        .windows(3)
        .filter(|tokens| tokens[0] == "mod" && matches!(tokens[2].as_str(), ";" | "{"))
        .map(|tokens| tokens[1].clone())
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
                for skipped in skip_collisions(&child, top_level) {
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
    let names = module_names(&main);
    let top_level: Vec<_> = names.iter().map(String::as_str).collect();
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
