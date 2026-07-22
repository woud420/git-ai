// Per-agent tool names, email patterns, and commit message markers used by
// commit-metadata recovery to identify which AI agent produced a commit.

const CODEX_TOOLS: &[&str] = &["codex", "codex-cloud"];
const CLAUDE_TOOLS: &[&str] = &["claude", "claude-web"];
const CURSOR_TOOLS: &[&str] = &["cursor", "cursor-agent"];
const COPILOT_TOOLS: &[&str] = &[
    "github-copilot",
    "github-copilot-cli",
    "github-copilot-agent",
    "copilot",
];
const DEVIN_TOOLS: &[&str] = &["devin"];
const DROID_TOOLS: &[&str] = &["droid"];
const WINDSURF_TOOLS: &[&str] = &["windsurf"];
const AMP_TOOLS: &[&str] = &["amp"];
const OPENCODE_TOOLS: &[&str] = &["opencode"];
const GEMINI_TOOLS: &[&str] = &["gemini"];
const CONTINUE_TOOLS: &[&str] = &["continue-cli"];

const CODEX_EMAILS: &[&str] = &["codex@openai.com"];
const CLAUDE_EMAILS: &[&str] = &[];
const CURSOR_EMAILS: &[&str] = &["cursoragent@cursor.com"];
const COPILOT_EMAILS: &[&str] = &["+copilot@users.noreply.github.com"];
const DEVIN_EMAILS: &[&str] = &["+devin-ai-integration[bot]@users.noreply.github.com"];
const DROID_EMAILS: &[&str] = &["+factory-droid[bot]@users.noreply.github.com"];
const WINDSURF_EMAILS: &[&str] = &["noreply@windsurf.com", "noreply@codeium.com"];
const AMP_EMAILS: &[&str] = &[];
const OPENCODE_EMAILS: &[&str] = &[];
const GEMINI_EMAILS: &[&str] = &[];
const CONTINUE_EMAILS: &[&str] = &[];

const CODEX_MARKER_EMAILS: &[&str] = &["noreply@openai.com"];
const CLAUDE_MARKER_EMAILS: &[&str] = &["noreply@anthropic.com"];
const CURSOR_MARKER_EMAILS: &[&str] = &[];
const COPILOT_MARKER_EMAILS: &[&str] = &[];
const DEVIN_MARKER_EMAILS: &[&str] = &[];
const DROID_MARKER_EMAILS: &[&str] = &[];
const WINDSURF_MARKER_EMAILS: &[&str] = &[];
const AMP_MARKER_EMAILS: &[&str] = &[];
const OPENCODE_MARKER_EMAILS: &[&str] = &[];
const GEMINI_MARKER_EMAILS: &[&str] = &[];
const CONTINUE_MARKER_EMAILS: &[&str] = &[];

const CODEX_MARKERS: &[&str] = &["codex"];
const CLAUDE_MARKERS: &[&str] = &["claude"];
const CURSOR_MARKERS: &[&str] = &["cursor"];
const COPILOT_MARKERS: &[&str] = &["copilot", "github copilot"];
const DEVIN_MARKERS: &[&str] = &["devin"];
const DROID_MARKERS: &[&str] = &["droid", "factory-droid"];
const WINDSURF_MARKERS: &[&str] = &["windsurf"];
const AMP_MARKERS: &[&str] = &["ampcode", "amp code"];
const OPENCODE_MARKERS: &[&str] = &["opencode", "open code"];
const GEMINI_MARKERS: &[&str] = &["gemini"];
const CONTINUE_MARKERS: &[&str] = &["continue-cli"];

/// Describes one known AI agent for the purposes of commit-metadata recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CommitAgentKind {
    pub(super) key: &'static str,
    pub(super) tools: &'static [&'static str],
    pub(super) emails: &'static [&'static str],
    pub(super) marker_emails: &'static [&'static str],
    pub(super) markers: &'static [&'static str],
}

pub(super) const KNOWN_COMMIT_AGENT_KINDS: &[CommitAgentKind] = &[
    CommitAgentKind {
        key: "codex",
        tools: CODEX_TOOLS,
        emails: CODEX_EMAILS,
        marker_emails: CODEX_MARKER_EMAILS,
        markers: CODEX_MARKERS,
    },
    CommitAgentKind {
        key: "claude",
        tools: CLAUDE_TOOLS,
        emails: CLAUDE_EMAILS,
        marker_emails: CLAUDE_MARKER_EMAILS,
        markers: CLAUDE_MARKERS,
    },
    CommitAgentKind {
        key: "cursor",
        tools: CURSOR_TOOLS,
        emails: CURSOR_EMAILS,
        marker_emails: CURSOR_MARKER_EMAILS,
        markers: CURSOR_MARKERS,
    },
    CommitAgentKind {
        key: "github-copilot",
        tools: COPILOT_TOOLS,
        emails: COPILOT_EMAILS,
        marker_emails: COPILOT_MARKER_EMAILS,
        markers: COPILOT_MARKERS,
    },
    CommitAgentKind {
        key: "devin",
        tools: DEVIN_TOOLS,
        emails: DEVIN_EMAILS,
        marker_emails: DEVIN_MARKER_EMAILS,
        markers: DEVIN_MARKERS,
    },
    CommitAgentKind {
        key: "droid",
        tools: DROID_TOOLS,
        emails: DROID_EMAILS,
        marker_emails: DROID_MARKER_EMAILS,
        markers: DROID_MARKERS,
    },
    CommitAgentKind {
        key: "windsurf",
        tools: WINDSURF_TOOLS,
        emails: WINDSURF_EMAILS,
        marker_emails: WINDSURF_MARKER_EMAILS,
        markers: WINDSURF_MARKERS,
    },
    CommitAgentKind {
        key: "amp",
        tools: AMP_TOOLS,
        emails: AMP_EMAILS,
        marker_emails: AMP_MARKER_EMAILS,
        markers: AMP_MARKERS,
    },
    CommitAgentKind {
        key: "opencode",
        tools: OPENCODE_TOOLS,
        emails: OPENCODE_EMAILS,
        marker_emails: OPENCODE_MARKER_EMAILS,
        markers: OPENCODE_MARKERS,
    },
    CommitAgentKind {
        key: "gemini",
        tools: GEMINI_TOOLS,
        emails: GEMINI_EMAILS,
        marker_emails: GEMINI_MARKER_EMAILS,
        markers: GEMINI_MARKERS,
    },
    CommitAgentKind {
        key: "continue-cli",
        tools: CONTINUE_TOOLS,
        emails: CONTINUE_EMAILS,
        marker_emails: CONTINUE_MARKER_EMAILS,
        markers: CONTINUE_MARKERS,
    },
];

/// Raw commit metadata fetched from git for use in commit-metadata recovery.
#[derive(Debug)]
pub(super) struct CommitMetadata {
    pub(super) message: String,
    pub(super) author_name: String,
    pub(super) author_email: String,
}

/// One agent detected from a commit's metadata (message, author identity, etc.).
#[derive(Clone, Debug)]
pub(super) struct CommitAgentDetection {
    pub(super) kind: CommitAgentKind,
    pub(super) source: &'static str,
    pub(super) marker: String,
}

/// Detect AI agents mentioned in a commit message or author identity.
pub(super) fn detect_commit_metadata_agents(
    metadata: &CommitMetadata,
) -> Vec<CommitAgentDetection> {
    let mut detections = Vec::new();
    for line in metadata.message.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some((name, value)) = trimmed.split_once(':') {
            let name_lower = name.trim().to_ascii_lowercase();
            if name_lower == "co-authored-by" {
                if let Some(kind) = detect_agent_from_identity(value) {
                    push_commit_agent_detection(
                        &mut detections,
                        kind,
                        "co_authored_by",
                        value.trim(),
                    );
                }
                continue;
            }
        }

        if lower.starts_with("claude-session:") {
            push_commit_agent_detection(
                &mut detections,
                commit_agent_kind_by_key("claude"),
                "session_trailer",
                trimmed,
            );
        } else if lower.starts_with("codex-session:") {
            push_commit_agent_detection(
                &mut detections,
                commit_agent_kind_by_key("codex"),
                "session_trailer",
                trimmed,
            );
        } else if lower.starts_with("cursor-session:") {
            push_commit_agent_detection(
                &mut detections,
                commit_agent_kind_by_key("cursor"),
                "session_trailer",
                trimmed,
            );
        }
    }

    let author_identity = format!("{} <{}>", metadata.author_name, metadata.author_email);
    if let Some(kind) = detect_agent_from_identity(&author_identity) {
        push_commit_agent_detection(&mut detections, kind, "author_identity", &author_identity);
    }

    detections
}

fn commit_agent_kind_by_key(key: &str) -> CommitAgentKind {
    KNOWN_COMMIT_AGENT_KINDS
        .iter()
        .copied()
        .find(|kind| kind.key == key)
        .expect("known commit agent key should exist")
}

fn push_commit_agent_detection(
    detections: &mut Vec<CommitAgentDetection>,
    kind: CommitAgentKind,
    source: &'static str,
    marker: &str,
) {
    if detections
        .iter()
        .any(|detection| detection.kind.key == kind.key)
    {
        return;
    }
    detections.push(CommitAgentDetection {
        kind,
        source,
        marker: marker.to_string(),
    });
}

fn detect_agent_from_identity(identity: &str) -> Option<CommitAgentKind> {
    let lower = identity.to_ascii_lowercase();
    let email = email_from_identity(identity);
    if let Some(email) = email.as_deref()
        && let Some(kind) = detect_agent_from_email(email)
    {
        return Some(kind);
    }

    KNOWN_COMMIT_AGENT_KINDS.iter().copied().find(|kind| {
        let marker_matches = kind
            .markers
            .iter()
            .any(|marker| contains_identity_marker(&lower, marker));
        if !marker_matches {
            return false;
        }

        match email.as_deref() {
            Some(email) => kind
                .marker_emails
                .iter()
                .any(|pattern| email_matches_pattern(email, pattern)),
            None => true,
        }
    })
}

fn contains_identity_marker(identity_lower: &str, marker: &str) -> bool {
    let marker_lower = marker.to_ascii_lowercase();
    let mut search_start = 0;
    while let Some(relative_start) = identity_lower[search_start..].find(&marker_lower) {
        let start = search_start + relative_start;
        let end = start + marker_lower.len();
        let before = identity_lower[..start].chars().next_back();
        let after = identity_lower[end..].chars().next();
        let before_boundary = before.is_none_or(|ch| !ch.is_ascii_alphanumeric());
        let after_boundary = after.is_none_or(|ch| !ch.is_ascii_alphanumeric());
        if before_boundary && after_boundary {
            return true;
        }
        search_start = end;
    }
    false
}

fn detect_agent_from_email(email: &str) -> Option<CommitAgentKind> {
    let email = email.trim().trim_matches('<').trim_matches('>');
    if email.is_empty() {
        return None;
    }

    KNOWN_COMMIT_AGENT_KINDS.iter().copied().find(|kind| {
        kind.emails
            .iter()
            .any(|pattern| email_matches_pattern(email, pattern))
    })
}

fn email_matches_pattern(email: &str, pattern: &str) -> bool {
    let email_lower = email.trim().to_ascii_lowercase();
    let pattern_lower = pattern.to_ascii_lowercase();
    if pattern_lower.starts_with('+') {
        email_lower.ends_with(&pattern_lower)
    } else {
        email_lower == pattern_lower
    }
}

fn email_from_identity(identity: &str) -> Option<String> {
    let start = identity.find('<')?;
    let end = identity[start + 1..].find('>')? + start + 1;
    Some(identity[start + 1..end].trim().to_string())
}
