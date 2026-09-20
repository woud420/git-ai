use super::transport::{disabled_hooks_config, exec_notes_transport};
use super::{is_missing_remote_notes_ref_error, merge_fetched_authorship_notes};
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::operations::git::oid::is_non_zero_oid;
use crate::operations::git::refs::{AI_AUTHORSHIP_FULL_REF, copy_ref, tracking_ref_for_remote};
use crate::operations::git::repository::find_repository;

pub fn fetch_authorship_notes_from_repository(
    worktree: &str,
    source: &str,
) -> Result<(), GitAiError> {
    // Scope hook suppression to this repository handle, including notes merges
    // and ref publication, without changing concurrent daemon operations.
    let repository = find_repository(&[
        "-C".to_string(),
        worktree.to_string(),
        "-c".to_string(),
        disabled_hooks_config().to_string(),
    ])?;
    let config = crate::config::Config::fresh();
    if !repository.is_collection_allowed(&config)
        || config.notes_backend_kind() == crate::config::NotesBackendKind::Http
    {
        return Ok(());
    }
    // fetch-pack connects to its literal source. Replacing it with fetch here
    // would apply url.*.insteadOf and could import notes from another repository.
    let mut args = repository.global_args_for_exec();
    args.extend([
        "fetch-pack".to_string(),
        "--no-progress".to_string(),
        source.to_string(),
        AI_AUTHORSHIP_FULL_REF.to_string(),
    ]);
    let output = match exec_notes_transport(&args) {
        Ok(output) => output,
        Err(error) if is_missing_remote_notes_ref_error(&error) => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let oid = fetched_notes_oid(&output.stdout).ok_or_else(|| PersistenceError::Io {
        operation: "parse fetched notes ref",
        path: String::new(),
        kind: std::io::ErrorKind::InvalidData,
        message: "fetch-pack did not return one exact authorship ref".to_string(),
    })?;
    let tracking_ref = tracking_ref_for_remote(source);
    copy_ref(&repository, oid, &tracking_ref)?;
    merge_fetched_authorship_notes(&repository, &tracking_ref)?;
    Ok(())
}

fn fetched_notes_oid(stdout: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(stdout).ok()?;
    let mut lines = text.lines();
    let (oid, reference) = lines.next()?.split_once(' ')?;
    (lines.next().is_none() && reference == AI_AUTHORSHIP_FULL_REF && is_non_zero_oid(oid))
        .then_some(oid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_pack_sync_requires_one_exact_nonzero_notes_ref() {
        for length in [40, 64] {
            let oid = "a".repeat(length);
            assert_eq!(
                fetched_notes_oid(format!("{oid} refs/notes/ai\n").as_bytes()),
                Some(oid.as_str())
            );
        }
        for output in [
            String::new(),
            format!("{} refs/notes/ai\n", "0".repeat(40)),
            "abc refs/notes/ai\n".to_string(),
            format!("{} refs/heads/main\n", "a".repeat(40)),
            format!(
                "{} refs/notes/ai\n{} refs/notes/ai\n",
                "a".repeat(40),
                "b".repeat(40)
            ),
            format!("{} refs/notes/ai\nextra\n", "a".repeat(40)),
        ] {
            assert!(fetched_notes_oid(output.as_bytes()).is_none(), "{output:?}");
        }
    }
}
