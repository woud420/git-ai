use super::{Error, args::ObserveArgs, output, shared};
use crate::config::Config;
use crate::model::repository::jj_observation_journal::{JjObservationJournal, ReadBudget};
use crate::operations::jj::admission::{
    NativeAdmissionExpectation, NativeAdmissionOutcome, NativeReconciliationExpectation,
    NativeReconciliationOutcome, reconcile_registered_history,
};
use crate::operations::workspace_context::WorkspaceContext;
use serde_json::Value;
use std::io::{self, Write};
use std::time::{Duration, Instant};

enum Failure {
    Semantic(Error),
    Output,
}

impl From<Error> for Failure {
    fn from(value: Error) -> Self {
        Self::Semantic(value)
    }
}

struct Progress {
    generation: u64,
    heads: Vec<String>,
}

pub(super) fn run(request: ObserveArgs<'_>) -> i32 {
    match stream(request) {
        Ok(()) => 0,
        Err(Failure::Semantic(error)) => {
            let _ = emit(&error.value());
            1
        }
        Err(Failure::Output) => 1,
    }
}

fn stream(request: ObserveArgs<'_>) -> Result<(), Failure> {
    let expected = request.expected;
    let mut progress = Progress {
        generation: expected.generation,
        heads: expected.heads,
    };
    let mut session = None;
    for attempt in 1..=request.attempts {
        let deadline = Instant::now() + shared::COMMAND_DEADLINE;
        let config = Config::fresh();
        shared::require_opt_in(&config)?;
        if session.is_none() {
            let context = shared::current_context(&config, "observe")?;
            let journal = shared::open_writable_journal(request.journal, deadline)?;
            session = Some((context, journal));
        }
        let (context, journal) = session.as_mut().expect("observer context initialized");
        let (value, next) = sample(
            journal,
            context,
            &config,
            NativeReconciliationExpectation {
                admission: NativeAdmissionExpectation {
                    source_id: expected.source,
                    initialization_receipt_id: expected.initialization_receipt,
                    baseline_id: expected.baseline,
                    generation: progress.generation,
                    admitted_head_ids: &progress.heads,
                },
                workspace_name: request.workspace,
                attachment_id: request.attachment,
            },
            deadline,
            attempt,
        )?;
        drop(config);
        emit(&value)?;
        drop(value);
        progress = next;
        if attempt < request.attempts {
            std::thread::sleep(Duration::from_millis(request.interval_ms));
        }
    }
    Ok(())
}

fn sample(
    journal: &mut JjObservationJournal,
    context: &WorkspaceContext,
    config: &Config,
    expected: NativeReconciliationExpectation<'_>,
    deadline: Instant,
    attempt: u64,
) -> Result<(Value, Progress), Error> {
    let outcome = reconcile_registered_history(
        journal,
        context,
        config,
        expected,
        deadline,
        &mut ReadBudget::new(shared::WRITE_SQL_READ_BYTES),
    )
    .map_err(|error| Error::new("admission_unavailable", error))?;
    let cursor = match &outcome {
        NativeReconciliationOutcome::Unchanged(value) => value.cursor(),
        NativeReconciliationOutcome::Admission(
            NativeAdmissionOutcome::Admitted(value)
            | NativeAdmissionOutcome::AlreadyAdmitted(value),
        ) => value.current_cursor(),
    };
    let progress = Progress {
        generation: cursor.generation(),
        heads: cursor.admitted_head_ids().to_vec(),
    };
    // The full native outcome is dropped before stdout can block or the loop waits.
    Ok((output::observe(&outcome, attempt), progress))
}

fn emit(value: &Value) -> Result<(), Failure> {
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    serde_json::to_writer(&mut writer, value).map_err(|_| Failure::Output)?;
    writer.write_all(b"\n").map_err(|_| Failure::Output)?;
    writer.flush().map_err(|_| Failure::Output)
}
