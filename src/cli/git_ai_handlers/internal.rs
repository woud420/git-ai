use crate::cli::machine_json::{
    emit_machine_json_error, parse_machine_json_arg, parse_machine_request,
    print_machine_json_serializable, resolve_repo_or_machine_error,
};
use crate::operations::authorship::ignore::effective_ignore_patterns;
use crate::operations::commands;
use crate::operations::git::repository::Repository;
use crate::operations::git::sync_authorship::{
    NotesExistence, fetch_authorship_notes, push_authorship_notes,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct EffectiveIgnorePatternsRequest {
    pub(super) user_patterns: Vec<String>,
    pub(super) extra_patterns: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct EffectiveIgnorePatternsResponse {
    pub(super) patterns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BlameAnalysisRequest {
    pub(super) file_path: String,
    #[serde(default)]
    pub(super) options: commands::blame::GitAiBlameOptions,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthorshipRemoteRequest {
    pub(super) remote_name: String,
}

#[derive(Debug, Serialize)]
pub(super) struct FetchAuthorshipNotesResponse {
    pub(super) notes_existence: String,
}

#[derive(Debug, Serialize)]
pub(super) struct PushAuthorshipNotesResponse {
    pub(super) ok: bool,
}

pub(super) fn disable_debug_logs_for_machine_command() {
    // SAFETY: git-ai command handlers run on the main thread and mutate process env
    // before spawning any worker threads for these internal machine commands.
    unsafe {
        std::env::set_var("GIT_AI_DEBUG", "0");
        std::env::remove_var("GIT_AI_DEBUG_PERFORMANCE");
    }
}

pub(super) fn parse_authorship_remote_request(
    args: &[String],
    command: &str,
) -> (Repository, AuthorshipRemoteRequest) {
    let payload =
        parse_machine_json_arg(args, command).unwrap_or_else(|msg| emit_machine_json_error(msg));

    let request: AuthorshipRemoteRequest = parse_machine_request(&payload);

    if request.remote_name.trim().is_empty() {
        emit_machine_json_error("remote_name cannot be empty");
    }

    let repo = resolve_repo_or_machine_error();

    (repo, request)
}

pub(super) fn notes_existence_label(existence: NotesExistence) -> &'static str {
    match existence {
        NotesExistence::Found => "found",
        NotesExistence::NotFound => "not_found",
    }
}

pub(crate) fn handle_effective_ignore_patterns_internal(args: &[String]) {
    let payload = parse_machine_json_arg(args, "effective-ignore-patterns")
        .unwrap_or_else(|msg| emit_machine_json_error(msg));

    let request: EffectiveIgnorePatternsRequest = parse_machine_request(&payload);

    let repo = resolve_repo_or_machine_error();

    let response = EffectiveIgnorePatternsResponse {
        patterns: effective_ignore_patterns(&repo, &request.user_patterns, &request.extra_patterns),
    };

    print_machine_json_serializable(&response);
}

pub(crate) fn handle_blame_analysis_internal(args: &[String]) {
    let payload = parse_machine_json_arg(args, "blame-analysis")
        .unwrap_or_else(|msg| emit_machine_json_error(msg));

    let request: BlameAnalysisRequest = parse_machine_request(&payload);

    if request.file_path.trim().is_empty() {
        emit_machine_json_error("file_path cannot be empty");
    }

    let repo = resolve_repo_or_machine_error();

    let analysis = repo
        .blame_analysis(&request.file_path, &request.options)
        .unwrap_or_else(|e| emit_machine_json_error(format!("blame_analysis failed: {}", e)));

    print_machine_json_serializable(&analysis);
}

pub(crate) fn handle_fetch_authorship_notes_internal(args: &[String]) {
    disable_debug_logs_for_machine_command();
    let (repo, request) = parse_authorship_remote_request(args, "fetch-authorship-notes");

    let notes_existence = fetch_authorship_notes(&repo, &request.remote_name).unwrap_or_else(|e| {
        emit_machine_json_error(format!("fetch_authorship_notes failed: {}", e))
    });

    let response = FetchAuthorshipNotesResponse {
        notes_existence: notes_existence_label(notes_existence).to_string(),
    };
    print_machine_json_serializable(&response);
}

pub(crate) fn handle_push_authorship_notes_internal(args: &[String]) {
    disable_debug_logs_for_machine_command();
    let (repo, request) = parse_authorship_remote_request(args, "push-authorship-notes");

    push_authorship_notes(&repo, &request.remote_name).unwrap_or_else(|e| {
        emit_machine_json_error(format!("push_authorship_notes failed: {}", e))
    });

    let response = PushAuthorshipNotesResponse { ok: true };
    print_machine_json_serializable(&response);
}
