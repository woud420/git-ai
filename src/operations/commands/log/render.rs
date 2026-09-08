use super::{GIT_LOG_FIELD_COUNT, GIT_LOG_FORMAT, LOG_BATCH_SIZE, LogError, ParsedLogArgs};
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::authorship::ignore::effective_ignore_patterns;
use crate::operations::authorship::stats::{
    stats_for_commit_stats_with_parent_and_authorship, write_stats_to_terminal,
};
use crate::operations::git::repository::Repository;
use std::io::{BufRead, BufReader};
use std::process::Child;

pub(super) struct LogRenderer {
    pub(super) repo: Repository,
    pub(super) options: ParsedLogArgs,
    pub(super) stream: CommitStream,
    pub(super) ignore_patterns: Vec<String>,
    pub(super) eof: bool,
}

impl LogRenderer {
    pub(super) fn new(repo: Repository, options: ParsedLogArgs) -> Result<Self, LogError> {
        let ignore_patterns = effective_ignore_patterns(&repo, &[], &[]);
        let stream = CommitStream::spawn(&repo, &options.git_log_args, options.show_decorations)?;
        Ok(Self {
            repo,
            options,
            stream,
            ignore_patterns,
            eof: false,
        })
    }

    pub(super) fn render_next_batch(&mut self) -> Result<Vec<String>, LogError> {
        if self.eof {
            return Ok(Vec::new());
        }

        let mut commits = Vec::new();
        for _ in 0..LOG_BATCH_SIZE {
            match self.stream.next_commit()? {
                Some(commit) => commits.push(commit),
                None => {
                    self.eof = true;
                    break;
                }
            }
        }

        if commits.is_empty() {
            return Ok(Vec::new());
        }

        let shas: Vec<String> = commits.iter().map(|commit| commit.sha.clone()).collect();
        let notes = crate::operations::git::notes_api::read_notes_batch(&self.repo, &shas)?;

        Ok(commits
            .iter()
            .map(|commit| {
                let note = notes.get(&commit.sha).map(String::as_str);
                render_commit(
                    &self.repo,
                    commit,
                    note,
                    &self.options,
                    &self.ignore_patterns,
                )
            })
            .collect())
    }

    pub(super) fn is_eof(&self) -> bool {
        self.eof
    }
}

pub(super) struct CommitStream {
    pub(super) child: Option<Child>,
    pub(super) stdout: BufReader<std::process::ChildStdout>,
}

impl CommitStream {
    pub(super) fn spawn(
        repo: &Repository,
        git_log_args: &[String],
        show_decorations: bool,
    ) -> Result<Self, LogError> {
        let mut command_args = repo.global_args_for_exec();
        command_args.push("log".to_string());
        command_args.push("--no-color".to_string());
        command_args.push("--no-notes".to_string());
        if show_decorations {
            command_args.push("--decorate=short".to_string());
        } else {
            command_args.push("--no-decorate".to_string());
        }
        command_args.push(format!("--format=format:{}", GIT_LOG_FORMAT));
        command_args.extend(git_log_args.iter().cloned());

        let mut child = crate::clients::git_cli::spawn_git_stdout(&command_args)?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LogError::Message("failed to capture git log stdout".to_string()))?;

        Ok(Self {
            child: Some(child),
            stdout: BufReader::new(stdout),
        })
    }

    pub(super) fn next_commit(&mut self) -> Result<Option<CommitRecord>, LogError> {
        let mut fields = Vec::with_capacity(GIT_LOG_FIELD_COUNT);
        for field_index in 0..GIT_LOG_FIELD_COUNT {
            match read_nul_field(&mut self.stdout)? {
                Some(field) => fields.push(field),
                None if field_index == 0 => {
                    self.wait_for_git_log()?;
                    return Ok(None);
                }
                None => {
                    self.wait_for_git_log()?;
                    return Err(LogError::Message(
                        "malformed git log output: truncated commit record".to_string(),
                    ));
                }
            }
        }

        Ok(Some(CommitRecord {
            sha: normalize_commit_sha_field(&fields[0]),
            parents: fields[1]
                .split_whitespace()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
            author_name: fields[2].clone(),
            author_email: fields[3].clone(),
            author_date: fields[4].clone(),
            decorations: fields[5].clone(),
            subject: fields[6].clone(),
            body: fields[7].clone(),
        }))
    }

    pub(super) fn wait_for_git_log(&mut self) -> Result<(), LogError> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        let status = child.wait().map_err(LogError::Io)?;
        if status.success() {
            Ok(())
        } else {
            Err(LogError::Message(format!(
                "git log exited with status {}",
                status
            )))
        }
    }
}

impl Drop for CommitStream {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) fn read_nul_field<R: BufRead>(reader: &mut R) -> Result<Option<String>, LogError> {
    let mut bytes = Vec::new();
    let read = reader.read_until(0, &mut bytes).map_err(LogError::Io)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    Ok(Some(String::from_utf8_lossy(&bytes).to_string()))
}

pub(super) fn normalize_commit_sha_field(field: &str) -> String {
    field.trim_start_matches('\n').to_string()
}

#[derive(Debug, Clone)]
pub(super) struct CommitRecord {
    pub(super) sha: String,
    pub(super) parents: Vec<String>,
    pub(super) author_name: String,
    pub(super) author_email: String,
    pub(super) author_date: String,
    pub(super) decorations: String,
    pub(super) subject: String,
    pub(super) body: String,
}

pub(super) fn render_commit(
    repo: &Repository,
    commit: &CommitRecord,
    raw_note: Option<&str>,
    options: &ParsedLogArgs,
    ignore_patterns: &[String],
) -> String {
    let authorship_log =
        raw_note.and_then(|note| AuthorshipLog::deserialize_from_string(note).ok());
    let mut out = String::new();

    if options.oneline {
        out.push_str(&short_sha(&commit.sha));
        out.push(' ');
        out.push_str(&commit.subject);
        if options.show_decorations && !commit.decorations.trim().is_empty() {
            out.push(' ');
            out.push('(');
            out.push_str(commit.decorations.trim());
            out.push(')');
        }
        out.push('\n');
    } else {
        out.push_str("commit ");
        out.push_str(&commit.sha);
        if options.show_decorations && !commit.decorations.trim().is_empty() {
            out.push(' ');
            out.push('(');
            out.push_str(commit.decorations.trim());
            out.push(')');
        }
        out.push('\n');

        if commit.parents.len() > 1 {
            out.push_str("Merge: ");
            out.push_str(
                &commit
                    .parents
                    .iter()
                    .map(|sha| short_sha(sha))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            out.push('\n');
        }

        out.push_str("Author: ");
        out.push_str(&commit.author_name);
        if !commit.author_email.is_empty() {
            out.push_str(" <");
            out.push_str(&commit.author_email);
            out.push('>');
        }
        out.push('\n');
        out.push_str("Date:   ");
        out.push_str(&commit.author_date);
        out.push_str("\n\n");

        append_indented_line(&mut out, &commit.subject, 4);
        let trimmed_body = commit.body.trim_end_matches('\n');
        if !trimmed_body.is_empty() {
            out.push('\n');
            for line in trimmed_body.lines() {
                append_indented_line(&mut out, line, 4);
            }
        }
        out.push('\n');
    }

    out.push_str("    Git AI stats:\n");
    match render_stats(
        repo,
        &commit.sha,
        &commit.parents,
        authorship_log.as_ref(),
        ignore_patterns,
    ) {
        Ok(stats) => append_indented_block(&mut out, &stats, 6),
        Err(message) => append_indented_line(&mut out, &message, 6),
    }

    if options.show_raw_notes {
        out.push('\n');
        out.push_str("    Authorship note:\n");
        match raw_note {
            Some(note) if !note.trim().is_empty() => append_indented_block(&mut out, note, 6),
            _ => append_indented_line(&mut out, "(none)", 6),
        }
    }

    out.push('\n');
    out
}

pub(super) fn render_stats(
    repo: &Repository,
    commit_sha: &str,
    parents: &[String],
    authorship_log: Option<&AuthorshipLog>,
    ignore_patterns: &[String],
) -> Result<String, String> {
    if parents.len() > 1 {
        return Err("stats skipped for merge commit".to_string());
    }

    let parent_sha = parents
        .first()
        .map(String::as_str)
        .unwrap_or("4b825dc642cb6eb9a060e54bf8d69288fbee4904");

    if let Ok(estimate) =
        crate::operations::authorship::post_commit::estimate_stats_cost_for_commit_range(
            repo,
            parent_sha,
            commit_sha,
            ignore_patterns,
        )
        && estimate.should_skip()
    {
        return Err(format!(
            "stats skipped for large commit; run `git-ai stats {}` to compute on demand",
            commit_sha
        ));
    }

    let stats = stats_for_commit_stats_with_parent_and_authorship(
        repo,
        commit_sha,
        parents.first().map(String::as_str),
        ignore_patterns,
        authorship_log,
    )
    .map_err(|e| format!("stats unavailable: {}", e))?;
    Ok(write_stats_to_terminal(&stats, false))
}

pub(super) fn append_indented_line(out: &mut String, line: &str, spaces: usize) {
    out.push_str(&" ".repeat(spaces));
    out.push_str(line);
    out.push('\n');
}

pub(super) fn append_indented_block(out: &mut String, block: &str, spaces: usize) {
    for line in block.trim_end_matches('\n').lines() {
        append_indented_line(out, line, spaces);
    }
}

pub(super) fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}
