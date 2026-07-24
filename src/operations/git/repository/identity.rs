//! Git author/committer identity resolution.
//!
//! Resolves identities via `git var GIT_COMMITTER_IDENT` / `GIT_AUTHOR_IDENT`
//! (which honor the full env > config > system-default precedence) with a
//! `git config user.name`/`user.email` fallback, plus git-ai author-config
//! overlays.

use crate::clients::git_cli::exec_git;
use crate::config;
use crate::error::GitAiError;

/// A Git identity (name + email) for the current repository.
///
/// Resolved via `git var GIT_COMMITTER_IDENT` which respects the full git precedence
/// chain (env vars > config > system defaults), unlike a raw `git config user.name`
/// lookup which can miss identities configured via environment variables or system-level
/// defaults.
#[derive(Debug, Clone, Default)]
pub struct GitAuthorIdentity {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl GitAuthorIdentity {
    /// Apply git-ai's optional author config as a partial override.
    pub fn with_author_config(&self, author: &config::AuthorConfig) -> Self {
        GitAuthorIdentity {
            name: author.name.clone().or_else(|| self.name.clone()),
            email: author.email.clone().or_else(|| self.email.clone()),
        }
    }

    /// Format as `"Name <email>"`, `"Name"`, `"<email>"`, or `None`.
    pub fn formatted(&self) -> Option<String> {
        match (&self.name, &self.email) {
            (Some(n), Some(e)) => Some(format!("{} <{}>", n, e)),
            (Some(n), None) => Some(n.clone()),
            (None, Some(e)) => Some(format!("<{}>", e)),
            (None, None) => None,
        }
    }

    /// Return the full identity (`"Name <email>"`) or fall back to name-only / `"unknown"`.
    pub fn formatted_or_unknown(&self) -> String {
        self.formatted().unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct GitIdentityResolution {
    pub raw_git_var: Option<String>,
    pub identity: GitAuthorIdentity,
}

#[derive(Debug, Clone, Default)]
pub struct GitConfigIdentityResolution {
    pub raw_name: Option<String>,
    pub raw_email: Option<String>,
    pub identity: GitAuthorIdentity,
}

/// Parse `git var GIT_COMMITTER_IDENT` output into name and email.
///
/// The output format is: `Name <email> unix-timestamp timezone`
/// For example: `John Doe <john@example.com> 1234567890 +0000`
pub fn parse_git_var_identity(output: &str) -> GitAuthorIdentity {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return GitAuthorIdentity::default();
    }

    // Find email in angle brackets
    let email_start = trimmed.find('<');
    let email_end = trimmed.find('>');

    match (email_start, email_end) {
        (Some(start), Some(end)) if end > start => {
            let name = trimmed[..start].trim();
            let email = trimmed[start + 1..end].trim();
            GitAuthorIdentity {
                name: if name.is_empty() {
                    None
                } else {
                    Some(name.to_string())
                },
                email: if email.is_empty() {
                    None
                } else {
                    Some(email.to_string())
                },
            }
        }
        _ => {
            // No angle brackets - just treat the whole string as a name
            GitAuthorIdentity {
                name: Some(trimmed.to_string()),
                email: None,
            }
        }
    }
}

pub fn global_git_config_committer_identity() -> Result<GitAuthorIdentity, GitAiError> {
    Ok(global_git_config_identity_resolution()?.identity)
}

pub fn global_git_config_identity_resolution() -> Result<GitConfigIdentityResolution, GitAiError> {
    let config =
        gix_config::File::from_globals().map_err(|e| GitAiError::GixError(e.to_string()))?;
    Ok(git_config_identity_resolution_from_config(&config))
}

pub fn current_git_committer_identity_resolution() -> GitIdentityResolution {
    resolve_git_var_identity_with_args(Vec::new(), "GIT_COMMITTER_IDENT", || {
        global_git_config_committer_identity().unwrap_or_default()
    })
}

pub(super) fn git_config_identity_resolution_from_config(
    config: &gix_config::File<'_>,
) -> GitConfigIdentityResolution {
    let raw_name = config.string("user.name").map(|cow| cow.to_string());
    let raw_email = config.string("user.email").map(|cow| cow.to_string());
    let name = raw_name
        .as_deref()
        .map(ToOwned::to_owned)
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());
    let email = raw_email
        .as_deref()
        .map(ToOwned::to_owned)
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string());

    GitConfigIdentityResolution {
        raw_name,
        raw_email,
        identity: GitAuthorIdentity { name, email },
    }
}

/// Resolve the git-ai effective author identity for the `X-Author-Identity`
/// HTTP header, without requiring a `Repository` instance.
///
/// Uses the shared git identity helper (full precedence chain: env vars >
/// config > system defaults), overlays any configured git-ai author fields,
/// falls back to the system hostname when git identity is unavailable, and
/// percent-encodes the result so it is safe for an HTTP header value.
///
/// Lives in the git adapter so the network client (`clients/api`) can accept
/// the resolved identity as plain data instead of reaching into `operations`.
pub fn resolve_api_author_identity() -> Option<String> {
    let author_config = config::Config::fresh_author_cached();
    let identity = current_git_committer_identity_resolution()
        .identity
        .with_author_config(&author_config);
    if let Some(formatted) = identity.formatted() {
        return Some(encode_for_header(&formatted));
    }

    resolve_fallback_identity()
        .map(|id| parse_git_var_identity(&id).with_author_config(&author_config))
        .and_then(|identity| identity.formatted())
        .map(|id| encode_for_header(&id))
}

/// Build a fallback identity matching git's format: `"Username <username@hostname>"`.
fn resolve_fallback_identity() -> Option<String> {
    let username = resolve_username()?;
    let hostname = resolve_hostname().unwrap_or_else(|| "localhost".to_string());
    Some(format!("{} <{}@{}>", username, username, hostname))
}

fn resolve_username() -> Option<String> {
    #[cfg(windows)]
    if let Ok(u) = std::env::var("USERNAME")
        && !u.trim().is_empty()
    {
        return Some(u.trim().to_string());
    }
    #[cfg(not(windows))]
    if let Ok(u) = std::env::var("USER")
        && !u.trim().is_empty()
    {
        return Some(u.trim().to_string());
    }
    None
}

fn resolve_hostname() -> Option<String> {
    #[cfg(windows)]
    if let Ok(h) = std::env::var("COMPUTERNAME")
        && !h.trim().is_empty()
    {
        return Some(h.trim().to_string());
    }
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.trim().is_empty()
    {
        return Some(h.trim().to_string());
    }
    let mut cmd = std::process::Command::new("hostname");
    #[cfg(windows)]
    {
        use crate::process_spawn::CREATE_NO_WINDOW;
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok()?;
    let h = String::from_utf8(output.stdout).ok()?;
    let h = h.trim();
    if h.is_empty() {
        None
    } else {
        Some(h.to_string())
    }
}

/// Percent-encode non-ASCII and control bytes so the value is safe for HTTP headers.
/// ureq 2.x accepts only visible ASCII (0x21..=0x7E) and space/tab in header values.
fn encode_for_header(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'%' => encoded.push_str("%25"),
            0x20..=0x7E => encoded.push(byte as char),
            _ => write!(encoded, "%{:02X}", byte).unwrap(),
        }
    }
    encoded
}

pub(super) fn resolve_git_var_identity_with_args<F>(
    mut args: Vec<String>,
    git_var: &str,
    fallback_identity: F,
) -> GitIdentityResolution
where
    F: FnOnce() -> GitAuthorIdentity,
{
    args.push("var".to_string());
    args.push(git_var.to_string());

    if let Ok(output) = exec_git(&args)
        && let Ok(stdout) = String::from_utf8(output.stdout)
    {
        let identity = parse_git_var_identity(&stdout);
        if identity.name.is_some() || identity.email.is_some() {
            return GitIdentityResolution {
                raw_git_var: Some(stdout.trim().to_string()),
                identity,
            };
        }
    }

    GitIdentityResolution {
        raw_git_var: None,
        identity: fallback_identity(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_config_overlays_full_identity() {
        let git_identity = GitAuthorIdentity {
            name: Some("Git User".to_string()),
            email: Some("git@example.com".to_string()),
        };
        let author = config::AuthorConfig {
            name: Some("Config User".to_string()),
            email: Some("config@example.com".to_string()),
        };

        assert_eq!(
            git_identity
                .with_author_config(&author)
                .formatted()
                .as_deref(),
            Some("Config User <config@example.com>")
        );
    }

    #[test]
    fn author_config_supports_partial_overrides() {
        let git_identity = GitAuthorIdentity {
            name: Some("Git User".to_string()),
            email: Some("git@example.com".to_string()),
        };

        let name_only = config::AuthorConfig {
            name: Some("Config User".to_string()),
            email: None,
        };
        assert_eq!(
            git_identity
                .with_author_config(&name_only)
                .formatted()
                .as_deref(),
            Some("Config User <git@example.com>")
        );

        let email_only = config::AuthorConfig {
            name: None,
            email: Some("config@example.com".to_string()),
        };
        assert_eq!(
            git_identity
                .with_author_config(&email_only)
                .formatted()
                .as_deref(),
            Some("Git User <config@example.com>")
        );
    }

    #[test]
    fn test_encode_for_header_ascii_passthrough() {
        let value = "John Doe <john@example.com>";
        assert_eq!(encode_for_header(value), value);
    }

    #[test]
    fn test_encode_for_header_non_ascii() {
        assert_eq!(
            encode_for_header("Ex\u{00f6}utf8lastname <user@example.com>"),
            "Ex%C3%B6utf8lastname <user@example.com>"
        );
    }

    #[test]
    fn test_encode_for_header_percent_encoded_for_reversibility() {
        assert_eq!(encode_for_header("100% done"), "100%25 done");
    }

    #[test]
    fn test_encode_for_header_special_ascii_chars_passthrough() {
        let value = "Name+Tag <user+tag@sub.example.com>";
        assert_eq!(encode_for_header(value), value);
    }

    #[test]
    fn test_encode_for_header_all_bytes_valid_for_ureq() {
        let input = "Ñoño García <nono@example.com>";
        let encoded = encode_for_header(input);
        assert!(
            encoded
                .bytes()
                .all(|b| b == b' ' || b == b'\t' || (0x21..=0x7E).contains(&b)),
            "encoded value contains invalid header bytes: {:?}",
            encoded
        );
    }
}
