use crate::config::Config;
use crate::model::jj_observer::{JjObserverError as Error, JjObserverTarget, paths};
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::model::repository::jj_observer_intent::StoredTarget;
use crate::operations::jj::admission::NativeAdmissionCursor;
use crate::operations::jj::observation::{JjObservationSession, JjObservationStep};
use crate::operations::workspace_context::discover;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(5);
const STARTUP_READ_BYTES: usize = 32 * 1024 * 1024;
const SAMPLE_READ_BYTES: usize = 96 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Session {
    pub(super) cursor: NativeAdmissionCursor,
    pub(super) replaying: bool,
    native: Arc<Mutex<JjObservationSession>>,
}

fn fresh_policy() -> Result<Config, Error> {
    let config = Config::fresh();
    if !config.has_allowed_repositories() {
        return Err(Error::new(
            "collection_disabled",
            "Repository collection is not allowed.",
        ));
    }
    Ok(config)
}

fn open(path: &Path, deadline: Instant) -> Result<JjObservationJournal, Error> {
    check_deadline(deadline)?;
    let journal = JjObservationJournal::open_existing_at_path(path)
        .map_err(|error| Error::new("journal_unavailable", error.diagnostic()))?;
    check_deadline(deadline)?;
    Ok(journal)
}

fn check_deadline(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        return Err(Error::new(
            "admission_unavailable",
            "Observer validation deadline exceeded.",
        ));
    }
    Ok(())
}

pub(super) fn validate(
    journal_path_hex: &str,
    workspace_path_hex: &str,
    pinned: Option<&StoredTarget>,
) -> Result<(StoredTarget, Session), Error> {
    let deadline = Instant::now() + DEADLINE;
    let config = fresh_policy()?;
    let journal_path = paths::decode(journal_path_hex)?;
    let workspace_path = paths::decode(workspace_path_hex)?;
    let context =
        discover(&workspace_path).map_err(|error| Error::new("context_unavailable", error.code))?;
    if context.vcs != "jj" || context.jj.is_none() {
        return Err(Error::new(
            "not_jj_workspace",
            "Native observation requires a jj workspace.",
        ));
    }
    let journal_path = journal_path.canonicalize().map_err(|_| {
        Error::new(
            "journal_unavailable",
            "Existing journal path is unavailable.",
        )
    })?;
    let journal = open(&journal_path, deadline)?;
    let canonical_workspace_path = paths::encode(&context.workspace_root)?;
    let native = JjObservationSession::new(
        journal,
        context,
        &config,
        deadline,
        &mut ReadBudget::new(STARTUP_READ_BYTES),
    )
    .map_err(|error| Error::new("admission_unavailable", error))?;
    let cursor = native.saved_cursor();
    let target = StoredTarget {
        journal_path_hex: paths::encode(&journal_path)?,
        workspace_path_hex: canonical_workspace_path,
        metadata: JjObserverTarget {
            source_id: cursor.source_id().to_owned(),
            initialization_receipt_id: cursor.initialization_receipt_id().to_owned(),
            reader_profile: cursor.reader_profile().to_owned(),
            baseline_id: cursor.baseline_id().to_owned(),
            baseline_generation: cursor.baseline_generation(),
            workspace_name: native.registration().workspace_name().to_owned(),
            attachment_id: native.registration().attachment_id().to_owned(),
        },
    };
    if pinned.is_some_and(|expected| expected != &target) {
        return Err(Error::new(
            "target_mismatch",
            "Saved observer target no longer matches.",
        ));
    }
    let session = Session {
        cursor: cursor.clone(),
        replaying: true,
        native: Arc::new(Mutex::new(native)),
    };
    check_deadline(deadline)?;
    Ok((target, session))
}

pub(super) fn sample(mut session: Session) -> Result<Session, Error> {
    let deadline = Instant::now() + DEADLINE;
    let config = fresh_policy()?;
    let mut native = session.native.lock().map_err(|_| {
        Error::new(
            "admission_unavailable",
            "Native replay session is unavailable.",
        )
    })?;
    let outcome = native
        .step(&config, deadline, &mut ReadBudget::new(SAMPLE_READ_BYTES))
        .map_err(|error| Error::new("admission_unavailable", error))?;
    session.replaying = matches!(outcome, JjObservationStep::Replaying);
    if let Some(cursor) = native.verified_cursor() {
        session.cursor = cursor.clone();
    }
    drop(native);
    Ok(session)
}
