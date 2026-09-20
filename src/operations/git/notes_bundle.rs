use crate::clients::git_cli::{exec_git, exec_git_stdin, exec_git_with_stdin_writer};
use crate::config::{Config, NotesBackendKind};
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::{AUTHORSHIP_LOG_VERSION, AuthorshipLog};
use crate::model::git_oid::is_non_zero_oid;
use crate::operations::git::{notes_api, refs, repository::Repository};
use std::collections::BTreeSet;
use std::io::{Error, ErrorKind, Write};
use std::path::Path;

pub(crate) const MAX_COMMITS: usize = 32;
const MAX_NOTE_BYTES: usize = 1024 * 1024;

const REPOSITORY_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_QUARANTINE_PATH",
];

fn invalid(message: impl Into<String>) -> GitAiError {
    Error::new(ErrorKind::InvalidInput, message.into()).into()
}

pub(crate) fn export_bundle(
    repo: &Repository,
    output: &Path,
    commits: &[String],
) -> Result<(usize, usize), GitAiError> {
    // --git-dir alone does not isolate the temporary repository from inherited
    // object, ref, index and config overrides. Refuse those invocation forms.
    if let Some(key) = REPOSITORY_ENV
        .iter()
        .find(|key| std::env::var_os(key).is_some())
    {
        return Err(invalid(format!("notes bundle does not support {key}")));
    }
    if commits.is_empty() || commits.len() > MAX_COMMITS {
        return Err(invalid(format!(
            "select between 1 and {MAX_COMMITS} full commit IDs"
        )));
    }
    if commits.iter().any(|oid| !is_non_zero_oid(oid)) {
        return Err(invalid("notes bundle requires full, non-zero commit IDs"));
    }
    let commits: Vec<String> = commits
        .iter()
        .map(|oid| oid.to_ascii_lowercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if commits.iter().any(|oid| oid.len() != commits[0].len()) {
        return Err(invalid("commit IDs must use one object format"));
    }
    let config = Config::fresh();
    if !repo.is_collection_allowed(&config) {
        return Err(invalid("repository collection is disabled"));
    }
    if config.notes_backend_kind() == NotesBackendKind::Http {
        return Err(invalid(
            "notes bundle currently supports git_notes and sqlite backends",
        ));
    }
    if output.as_os_str().is_empty() || output == Path::new("-") {
        return Err(invalid("notes bundle requires a new output file"));
    }
    if output.symlink_metadata().is_ok() {
        return Err(Error::new(ErrorKind::AlreadyExists, "output already exists").into());
    }
    let mut args = repo.global_args_for_exec();
    args.extend([
        "--no-replace-objects".into(),
        "cat-file".into(),
        "--batch-check=%(objectname) %(objecttype)".into(),
    ]);
    let checked = exec_git_stdin(&args, (commits.join("\n") + "\n").as_bytes())?;
    let text = String::from_utf8(checked.stdout)?;
    if text.lines().count() != commits.len()
        || text
            .lines()
            .zip(&commits)
            .any(|(line, oid)| line != format!("{oid} commit"))
    {
        return Err(invalid(
            "every selected ID must identify an existing commit",
        ));
    }
    let notes = notes_api::read_notes_batch(repo, &commits)?;
    let mut entries = Vec::new();
    for oid in &commits {
        let Some(content) = notes.get(oid) else {
            continue;
        };
        if content.len() > MAX_NOTE_BYTES {
            return Err(invalid(format!(
                "note for {oid} exceeds the 1 MiB export limit"
            )));
        }
        let parsed = AuthorshipLog::deserialize_from_string(content)
            .map_err(|error| invalid(format!("invalid note for {oid}: {error}")))?;
        if parsed.metadata.schema_version != AUTHORSHIP_LOG_VERSION
            || parsed.metadata.base_commit_sha != *oid
        {
            return Err(invalid(format!(
                "note schema or commit identity does not match {oid}"
            )));
        }
        entries.push((oid.as_str(), content.as_str()));
    }
    if entries.is_empty() {
        return Err(invalid(
            "no authorship notes available for selected commits",
        ));
    }
    write_bundle(output, &entries, commits[0].len() == 64)?;
    Ok((entries.len(), commits.len() - entries.len()))
}

fn write_bundle(output: &Path, entries: &[(&str, &str)], sha256: bool) -> Result<(), GitAiError> {
    let directory = tempfile::tempdir()?;
    let git_dir = directory.path().join("repository");
    let hooks = directory.path().join("hooks");
    std::fs::create_dir(&hooks)?;
    let prefix = vec![
        format!("--git-dir={}", git_dir.display()),
        "-c".into(),
        format!("core.hooksPath={}", hooks.display()),
    ];
    let mut args = prefix.clone();
    args.extend([
        "init".into(),
        "--bare".into(),
        "--template=".into(),
        format!("--object-format={}", if sha256 { "sha256" } else { "sha1" }),
    ]);
    exec_git(&args)?;

    let mut args = prefix.clone();
    args.extend(["fast-import".into(), "--quiet".into()]);
    exec_git_with_stdin_writer(&args, |writer| {
        for (i, (_, content)) in entries.iter().enumerate() {
            refs::write_blob_stanza(writer, i + 1, content)?;
        }
        refs::write_notes_commit_header(
            writer,
            "refs/notes/ai",
            format_args!("git-ai <git-ai@localhost> 1000000000 +0000"),
            None,
        )?;
        for (i, (oid, _)) in entries.iter().enumerate() {
            writeln!(writer, "M 100644 :{} {oid}", i + 1)?;
        }
        writer.write_all(b"\n")
    })?;
    let mut args = prefix;
    args.extend([
        "bundle".into(),
        "create".into(),
        "-".into(),
        "refs/notes/ai".into(),
    ]);
    let bundle = exec_git(&args)?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(&bundle.stdout)?;
    staged.as_file().sync_all()?;
    staged
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}
