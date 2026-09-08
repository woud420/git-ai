use super::ActorDaemonCoordinator;
use crate::model::jj_observer::{
    JjObserverControlReply as Reply, JjObserverError as Error, JjObserverSessionCursor,
};
use crate::model::repository::jj_observer_intent::{self as store, StoredIntent};
use crate::model::repository::sqlite::poisoned_lock;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, Notify};

mod control;
mod job_slot;
mod native;
mod worker;
use job_slot::{JobSlot, JobToken};
use native::Session;

pub(crate) struct Observer {
    state: Mutex<State>,
    mutation: AsyncMutex<()>,
    wake: Notify,
    path: PathBuf,
}

struct State {
    loaded: bool,
    available: bool,
    intent: Option<StoredIntent>,
    session: Option<Session>,
    error: Option<Error>,
    stopped: bool,
    jobs: JobSlot,
    next_due: Instant,
    coordinator: Weak<ActorDaemonCoordinator>,
}

impl State {
    fn revision(&self) -> u64 {
        self.intent.as_ref().map_or(0, |intent| intent.revision)
    }
    fn shutting_down(&self) -> bool {
        self.coordinator
            .upgrade()
            .is_none_or(|value| value.is_shutting_down())
    }
    fn runtime(&self) -> &'static str {
        if !self.loaded {
            return "pending";
        }
        if self.jobs.in_flight() && (self.stopped || self.shutting_down()) {
            return "stopping";
        }
        if self.jobs.in_flight() && self.session.is_none() {
            return "pending";
        }
        if self.error.is_some() {
            return "blocked";
        }
        if self.intent.as_ref().is_none_or(|intent| !intent.enabled) {
            return "disabled";
        }
        if self.session.is_some() && !self.stopped {
            "active"
        } else {
            "pending"
        }
    }
    fn can_control(&self) -> Result<(), Error> {
        if self.shutting_down() {
            return Err(Error::new("observer_busy", "Daemon is shutting down."));
        }
        if !self.loaded {
            return Err(busy());
        }
        if !self.available {
            return Err(Error::new(
                "intent_unavailable",
                "Observer intent is unavailable; restart is required to reload it.",
            ));
        }
        Ok(())
    }
    fn finish(&mut self, token: JobToken) -> bool {
        let revision = self.revision();
        let current = self.jobs.finish(token, revision)
            && self.available
            && !self.shutting_down()
            && !self.stopped;
        if !self.jobs.in_flight()
            && self.stopped
            && self.intent.as_ref().is_none_or(|intent| !intent.enabled)
        {
            self.session = None;
        }
        current
    }
    fn intent_failed(&mut self, error: Error) {
        let _ = self.jobs.cancel();
        self.stopped = true;
        self.available = false;
        self.intent = None;
        self.session = None;
        self.error = Some(error);
    }
}

fn busy() -> Error {
    Error::new(
        "observer_busy",
        "Observer is busy; inspect status before retrying.",
    )
}
fn changed() -> Error {
    Error::new(
        "intent_changed",
        "Observer intent changed while native work was running.",
    )
}
fn next_revision(value: Option<&StoredIntent>) -> Result<u64, Error> {
    value
        .map_or(0, |value| value.revision)
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Error::new("intent_persistence", "Observer control revision exhausted."))
}

impl Observer {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            mutation: AsyncMutex::new(()),
            wake: Notify::new(),
            state: Mutex::new(State {
                loaded: false,
                available: false,
                intent: None,
                session: None,
                error: None,
                stopped: true,
                jobs: JobSlot::default(),
                next_due: Instant::now(),
                coordinator: Weak::new(),
            }),
        }
    }

    pub(crate) fn status(&self, action: &str, disposition: &str, error: Option<Error>) -> Reply {
        let state = poisoned_lock(&self.state);
        let known = state.loaded && state.available;
        Reply {
            schema_version: 1,
            backend: "jj".to_owned(),
            attribution_enabled: false,
            action: format!("observer_{action}"),
            disposition: if error.is_some() {
                "error"
            } else {
                disposition
            }
            .to_owned(),
            revision: known.then(|| state.revision()),
            desired_intent: known.then(|| {
                if state.intent.as_ref().is_some_and(|intent| intent.enabled) {
                    "enabled"
                } else {
                    "disabled"
                }
                .to_owned()
            }),
            target: state
                .intent
                .as_ref()
                .map(|intent| intent.target.metadata.clone()),
            runtime: state.runtime().to_owned(),
            in_flight: state.jobs.in_flight(),
            session_cursor: state
                .session
                .as_ref()
                .map(|session| JjObserverSessionCursor {
                    generation: session.cursor.generation(),
                    admitted_head_ids: session.cursor.admitted_head_ids().to_vec(),
                }),
            last_error: state.error.clone(),
            error,
        }
    }

    // Callers hold the mutation gate through this joined blocking task. A lost
    // client response cannot release publication ownership before the commit ends.
    async fn publish(
        &self,
        expected: Option<StoredIntent>,
        next: StoredIntent,
    ) -> Result<(), Error> {
        let path = self.path.clone();
        let saved = next.clone();
        let outcome =
            tokio::task::spawn_blocking(move || store::replace(&path, &expected, &saved)).await;
        match outcome {
            Ok(Ok(())) => {
                poisoned_lock(&self.state).intent = Some(next);
                Ok(())
            }
            _ => {
                let error = Error::new(
                    "intent_persistence",
                    "Observer intent publication failed; durable state must be reloaded.",
                );
                poisoned_lock(&self.state).intent_failed(error.clone());
                self.wake.notify_one();
                Err(error)
            }
        }
    }

    async fn block(&self, error: Error) {
        let expected = poisoned_lock(&self.state).intent.clone();
        let Some(mut next) = expected.clone() else {
            return;
        };
        let revision = match next_revision(expected.as_ref()) {
            Ok(value) => value,
            Err(error) => {
                let mut state = poisoned_lock(&self.state);
                state.stopped = true;
                state.error = Some(error);
                return;
            }
        };
        next.revision = revision;
        let mut durable = error.clone();
        durable.persisted = true;
        next.blocked = Some(durable.clone());
        {
            let mut state = poisoned_lock(&self.state);
            state.stopped = true;
            state.error = Some(error);
        }
        if self.publish(expected, next).await.is_ok() {
            poisoned_lock(&self.state).error = Some(durable);
        }
    }
}

#[cfg(test)]
mod tests;
