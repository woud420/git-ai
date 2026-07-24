//! Classifies and validates [`super::BlameAnalysisResult`] output against the
//! expected untracked/known-human/AI line attribution used by the
//! `git-ai debug` attribution self-check.

use super::BlameAnalysisResult;
use crate::model::working_log::CheckpointKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineClassification {
    Untracked,
    KnownHuman,
    Ai,
    Unknown,
}

impl LineClassification {
    fn as_str(self) -> &'static str {
        match self {
            LineClassification::Untracked => "untracked",
            LineClassification::KnownHuman => "known_human",
            LineClassification::Ai => "ai",
            LineClassification::Unknown => "unknown",
        }
    }
}

pub(crate) fn validate_self_check_blame_analysis(
    analysis: Result<BlameAnalysisResult, String>,
) -> Result<Vec<String>, String> {
    let analysis = analysis.map_err(|err| format!("blame analysis failed: {}", err))?;
    let expected = [
        (1, LineClassification::Untracked),
        (2, LineClassification::KnownHuman),
        (3, LineClassification::Ai),
    ];
    let mut details = Vec::new();

    for (line, expected_class) in expected {
        let actual = classify_line(&analysis, line);
        let raw_author = analysis
            .line_authors
            .get(&line)
            .cloned()
            .unwrap_or_else(|| "<missing>".to_string());
        details.push(format!(
            "line {}: {} (expected {}, raw={})",
            line,
            actual.as_str(),
            expected_class.as_str(),
            raw_author
        ));
        if actual != expected_class {
            return Err(format!(
                "unexpected attribution for line {}: got {}, expected {}\n{}",
                line,
                actual.as_str(),
                expected_class.as_str(),
                format_blame_analysis_debug(&analysis)
            ));
        }
    }

    details.push(format_blame_analysis_debug(&analysis));
    Ok(details)
}

fn classify_line(analysis: &BlameAnalysisResult, line: u32) -> LineClassification {
    let Some(author) = analysis.line_authors.get(&line) else {
        return LineClassification::Unknown;
    };

    if author == &CheckpointKind::Human.to_str() {
        return LineClassification::Untracked;
    }

    if author.starts_with("h_") && analysis.humans.contains_key(author) {
        return LineClassification::KnownHuman;
    }

    if analysis
        .prompt_records
        .get(author)
        .is_some_and(|prompt| prompt.agent_id.tool == "mock_ai")
    {
        return LineClassification::Ai;
    }

    LineClassification::Unknown
}

fn format_blame_analysis_debug(analysis: &BlameAnalysisResult) -> String {
    let mut prompt_keys = analysis.prompt_records.keys().cloned().collect::<Vec<_>>();
    prompt_keys.sort();
    let mut session_keys = analysis.session_records.keys().cloned().collect::<Vec<_>>();
    session_keys.sort();
    let mut human_keys = analysis.humans.keys().cloned().collect::<Vec<_>>();
    human_keys.sort();

    format!(
        "blame analysis: line_authors={:?}, prompt_keys={:?}, session_keys={:?}, human_keys={:?}",
        analysis.line_authors, prompt_keys, session_keys, human_keys
    )
}
