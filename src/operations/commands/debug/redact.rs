/// Replaces the userinfo of every `scheme://userinfo@host` URL in `text` with
/// `[REDACTED]`, so remote URLs carrying tokens or passwords are safe to print.
pub(super) fn redact_url_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find("://") {
        let (head, tail) = rest.split_at(idx + 3);
        out.push_str(head);
        let authority_end = tail.find(['/', ' ', '\t']).unwrap_or(tail.len());
        rest = match tail[..authority_end].rfind('@') {
            Some(at) => {
                out.push_str("[REDACTED]");
                &tail[at..]
            }
            None => tail,
        };
    }
    out.push_str(rest);
    out
}

pub(super) fn redact_git_config_line(line: &str) -> String {
    redact_url_userinfo(&redact_sensitive_config_value(line))
}

fn redact_sensitive_config_value(line: &str) -> String {
    if !line.contains('\t') {
        if let Some((key, value)) = line.split_once('=')
            && should_redact_key_value(key, value)
        {
            return format!("{}=[REDACTED]", key);
        }
        return line.to_string();
    }

    let mut parts = line.splitn(3, '\t');
    let first = match parts.next() {
        Some(v) => v,
        None => return line.to_string(),
    };
    let second = match parts.next() {
        Some(v) => v,
        None => return line.to_string(),
    };

    match parts.next() {
        // 3-field format: scope \t origin \t key=value
        // (from `git config --list --show-origin --show-scope`)
        Some(key_value) => {
            let (key, value) = match key_value.split_once('=') {
                Some((key, value)) => (key, value),
                None => return line.to_string(),
            };
            if should_redact_key_value(key, value) {
                format!("{}\t{}\t{}=[REDACTED]", first, second, key)
            } else {
                line.to_string()
            }
        }
        // 2-field format: origin \t key=value
        // (from `git config --list --show-origin` without --show-scope)
        None => {
            let (key, value) = match second.split_once('=') {
                Some((key, value)) => (key, value),
                None => return line.to_string(),
            };
            if should_redact_key_value(key, value) {
                format!("{}\t{}=[REDACTED]", first, key)
            } else {
                line.to_string()
            }
        }
    }
}

fn should_redact_key_value(key: &str, value: &str) -> bool {
    let key_lower = key.to_lowercase();
    let value_lower = value.to_lowercase();

    let sensitive_key_markers = [
        "password",
        "passwd",
        "token",
        "secret",
        "oauth",
        "authorization",
        "apikey",
        "api_key",
        "extraheader",
    ];

    if sensitive_key_markers
        .iter()
        .any(|marker| key_lower.contains(marker))
    {
        return true;
    }

    if key_lower.starts_with("url.") {
        return true;
    }

    sensitive_key_markers
        .iter()
        .any(|marker| value_lower.contains(marker))
}

pub(super) fn redact_env_value(key: &str, value: &str) -> String {
    let key_lower = key.to_lowercase();
    let sensitive_markers = ["token", "secret", "password", "key"];
    if sensitive_markers
        .iter()
        .any(|marker| key_lower.contains(marker))
    {
        return "[REDACTED]".to_string();
    }

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }

    if trimmed.len() > 200 {
        let truncated: String = trimmed.chars().take(200).collect();
        return format!("{}...[truncated]", truncated);
    }

    trimmed.to_string()
}
