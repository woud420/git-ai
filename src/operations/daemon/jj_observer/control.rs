use super::*;
use crate::model::repository::jj_observer_intent::StoredTarget;

enum Activation {
    Enable { journal: String, workspace: String },
    Resume,
}

struct PreparedActivation {
    token: JobToken,
    journal: String,
    workspace: String,
    pinned: Option<StoredTarget>,
    was_stopped: bool,
}

impl Observer {
    pub(crate) async fn enable(&self, journal: String, workspace: String) -> Reply {
        self.activate(Activation::Enable { journal, workspace })
            .await
    }
    pub(crate) async fn resume(&self) -> Reply {
        self.activate(Activation::Resume).await
    }

    async fn activate(&self, activation: Activation) -> Reply {
        let action = if matches!(activation, Activation::Resume) {
            "resume"
        } else {
            "enable"
        };
        let gate = match self.mutation.try_lock() {
            Ok(gate) => gate,
            Err(_) => return self.status(action, "error", Some(busy())),
        };
        let prepared = {
            let mut state = poisoned_lock(&self.state);
            self.prepare_activation(&mut state, &activation)
        };
        let PreparedActivation {
            token,
            journal,
            workspace,
            pinned,
            was_stopped,
        } = match prepared {
            Ok(Some(value)) => value,
            Ok(None) => return self.status(action, "already_active", None),
            Err(error) => return self.status(action, "error", Some(error)),
        };
        drop(gate);
        let result = tokio::task::spawn_blocking(move || {
            native::validate(&journal, &workspace, pinned.as_ref())
        })
        .await
        .unwrap_or_else(|_| {
            Err(Error::new(
                "admission_unavailable",
                "Native observer validation did not complete.",
            ))
        });
        let _gate = self.mutation.lock().await;
        let current = poisoned_lock(&self.state).finish(token);
        if !current {
            self.wake.notify_one();
            return self.status(action, "error", Some(changed()));
        }
        let result = match result {
            Ok((target, session)) => self.install_activation(action, target, session).await,
            Err(error) => {
                poisoned_lock(&self.state).stopped = was_stopped;
                Err(error)
            }
        };
        self.wake.notify_one();
        match result {
            Ok(disposition) => self.status(action, disposition, None),
            Err(error) => self.status(action, "error", Some(error)),
        }
    }

    fn prepare_activation(
        &self,
        state: &mut State,
        activation: &Activation,
    ) -> Result<Option<PreparedActivation>, Error> {
        state.can_control()?;
        if matches!(activation, Activation::Resume)
            && state
                .intent
                .as_ref()
                .is_some_and(|value| value.enabled && value.blocked.is_none())
            && state.error.is_none()
            && !state.stopped
        {
            return Ok(None);
        }
        if state.jobs.in_flight() {
            return Err(busy());
        }
        let (journal, workspace, pinned) = match activation {
            Activation::Enable { journal, workspace } => {
                if state.error.is_some()
                    || state
                        .intent
                        .as_ref()
                        .is_some_and(|value| value.blocked.is_some())
                {
                    return Err(Error::new(
                        "resume_required",
                        "The saved observer is blocked; resume or disable it explicitly.",
                    ));
                }
                (journal.clone(), workspace.clone(), None)
            }
            Activation::Resume => {
                next_revision(state.intent.as_ref())?;
                let intent = state
                    .intent
                    .as_ref()
                    .ok_or_else(|| Error::new("not_enabled", "No observer target is saved."))?;
                (
                    intent.target.journal_path_hex.clone(),
                    intent.target.workspace_path_hex.clone(),
                    Some(intent.target.clone()),
                )
            }
        };
        let revision = state.revision();
        let token = state
            .jobs
            .reserve(revision)
            .map_err(|message| Error::new("observer_busy", message))?;
        let was_stopped = state.stopped;
        state.stopped = false;
        if state
            .intent
            .as_ref()
            .is_none_or(|intent| !intent.enabled || intent.blocked.is_some())
        {
            state.session = None;
        }
        Ok(Some(PreparedActivation {
            token,
            journal,
            workspace,
            pinned,
            was_stopped,
        }))
    }

    async fn install_activation(
        &self,
        action: &str,
        target: StoredTarget,
        session: Session,
    ) -> Result<&'static str, Error> {
        let expected = poisoned_lock(&self.state).intent.clone();
        if let Some(existing) = &expected
            && action == "enable"
            && existing.enabled
            && existing.blocked.is_none()
        {
            if existing.target != target {
                return Err(Error::new(
                    "target_mismatch",
                    "Disable and drain the saved observer before choosing another target.",
                ));
            }
            return Ok("already_enabled");
        }
        let next = StoredIntent {
            schema_version: 1,
            revision: next_revision(expected.as_ref())?,
            target,
            enabled: true,
            blocked: None,
        };
        self.publish(expected, next).await?;
        {
            let mut state = poisoned_lock(&self.state);
            state.session = Some(session);
            state.error = None;
            state.stopped = false;
            state.next_due = Instant::now();
        }
        Ok(if action == "resume" {
            "resume_started"
        } else {
            "enabled"
        })
    }

    pub(crate) async fn disable(&self) -> Reply {
        let _gate = self.mutation.lock().await;
        let prepared = {
            let mut state = poisoned_lock(&self.state);
            state.stopped = true;
            let cancelled = state
                .jobs
                .cancel()
                .map_err(|message| Error::new("intent_persistence", message));
            if !state.jobs.in_flight() {
                state.session = None;
            }
            cancelled
                .and_then(|()| state.can_control())
                .map(|()| state.intent.clone())
        };
        self.wake.notify_one();
        let expected = match prepared {
            Ok(value) => value,
            Err(error) => return self.status("disable", "error", Some(error)),
        };
        let Some(mut next) = expected.clone() else {
            return self.status("disable", "already_disabled", None);
        };
        if !next.enabled && next.blocked.is_none() {
            return self.status("disable", "already_disabled", None);
        }
        next.revision = match next_revision(expected.as_ref()) {
            Ok(revision) => revision,
            Err(error) => {
                poisoned_lock(&self.state).error = Some(error.clone());
                return self.status("disable", "error", Some(error));
            }
        };
        next.enabled = false;
        next.blocked = None;
        match self.publish(expected, next).await {
            Ok(()) => {
                poisoned_lock(&self.state).error = None;
                self.status("disable", "disable_requested", None)
            }
            Err(error) => self.status("disable", "error", Some(error)),
        }
    }
}

#[cfg(test)]
mod volatile_tests;
