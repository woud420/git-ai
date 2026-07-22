/// Tokenize a non-shell Git alias for git-ai's alias expansion paths.
///
/// Shell aliases (`!command`) and unterminated quotes cannot be expanded into a
/// regular Git invocation, so they return `None`.
pub(crate) fn parse_alias_tokens(value: &str) -> Option<Vec<String>> {
    let trimmed = value.trim_start();
    if trimmed.starts_with('!') {
        return None;
    }

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in trimmed.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current.push(ch);
            }
            continue;
        }

        if in_double {
            match ch {
                '"' => in_double = false,
                '\\' => escaped = true,
                _ => current.push(ch),
            }
            continue;
        }

        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            '\\' => escaped = true,
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if escaped {
        current.push('\\');
    }
    if in_single || in_double {
        return None;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::parse_alias_tokens;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn preserves_git_alias_syntax() {
        let cases = [
            ("", Some(Vec::<String>::new())),
            ("  commit -v", Some(argv(&["commit", "-v"]))),
            (
                "log --format='author name'",
                Some(argv(&["log", "--format=author name"])),
            ),
            (
                r#"commit -m "quoted \"message\"""#,
                Some(argv(&["commit", "-m", "quoted \"message\""])),
            ),
            (
                r"checkout feature\ branch",
                Some(argv(&["checkout", "feature branch"])),
            ),
            (r"show trailing\", Some(argv(&["show", r"trailing\"]))),
            ("'unterminated", None),
            ("\"unterminated", None),
            ("  !git status", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_alias_tokens(input), expected, "input: {input:?}");
        }
    }
}
