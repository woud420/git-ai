use super::*;

impl Observer {
    pub(crate) fn start(
        self: &Arc<Self>,
        coordinator: Arc<ActorDaemonCoordinator>,
    ) -> tokio::task::JoinHandle<()> {
        poisoned_lock(&self.state).coordinator = Arc::downgrade(&coordinator);
        let observer = Arc::clone(self);
        tokio::spawn(async move {
            observer.run(coordinator).await;
        })
    }

    async fn run(&self, coordinator: Arc<ActorDaemonCoordinator>) {
        self.bootstrap().await;
        loop {
            if coordinator.is_shutting_down() {
                break;
            }
            let wait = {
                let state = poisoned_lock(&self.state);
                if Self::may_sample(&state) {
                    Some(state.next_due.saturating_duration_since(Instant::now()))
                } else {
                    None
                }
            };
            if wait.is_some_and(|delay| delay.is_zero()) {
                self.tick().await;
                continue;
            }
            tokio::select! {
                _ = self.wake.notified() => {},
                _ = coordinator.wait_for_shutdown() => break,
                _ = tokio::time::sleep(wait.unwrap_or(Duration::from_secs(3600))), if wait.is_some() => {},
            }
        }
        {
            let _gate = self.mutation.lock().await;
            let mut state = poisoned_lock(&self.state);
            state.stopped = true;
            let _ = state.jobs.cancel();
        }
        loop {
            let wake = self.wake.notified();
            if !poisoned_lock(&self.state).jobs.in_flight() {
                break;
            }
            wake.await;
        }
        poisoned_lock(&self.state).session = None;
    }

    fn may_sample(state: &State) -> bool {
        state.available
            && !state.stopped
            && !state.shutting_down()
            && state.error.is_none()
            && state.session.is_some()
            && !state.jobs.in_flight()
            && state
                .intent
                .as_ref()
                .is_some_and(|intent| intent.enabled && intent.blocked.is_none())
    }

    async fn bootstrap(&self) {
        let gate = self.mutation.lock().await;
        let path = self.path.clone();
        let result = tokio::task::spawn_blocking(move || store::load(&path)).await;
        let prepared = {
            let mut state = poisoned_lock(&self.state);
            state.loaded = true;
            match result {
                Ok(Ok(intent)) => {
                    state.available = true;
                    state.error = intent.as_ref().and_then(|value| value.blocked.clone());
                    state.intent = intent;
                }
                _ => state.intent_failed(Error::new(
                    "intent_unavailable",
                    "Observer intent could not be loaded.",
                )),
            }
            let enabled = state.available
                && !state.shutting_down()
                && state
                    .intent
                    .as_ref()
                    .is_some_and(|intent| intent.enabled && intent.blocked.is_none());
            if enabled {
                let revision = state.revision();
                let token = state
                    .jobs
                    .reserve(revision)
                    .expect("first observer job has an unused epoch");
                state.stopped = false;
                Some((token, state.intent.as_ref().unwrap().target.clone()))
            } else {
                None
            }
        };
        drop(gate);
        let Some((token, target)) = prepared else {
            return;
        };
        let result = tokio::task::spawn_blocking(move || {
            native::validate(
                &target.journal_path_hex,
                &target.workspace_path_hex,
                Some(&target),
            )
        })
        .await
        .unwrap_or_else(|_| {
            Err(Error::new(
                "admission_unavailable",
                "Native observer startup did not complete.",
            ))
        });
        let _gate = self.mutation.lock().await;
        if !poisoned_lock(&self.state).finish(token) {
            self.wake.notify_one();
            return;
        }
        match result {
            Ok((_, session)) => {
                let mut state = poisoned_lock(&self.state);
                state.session = Some(session);
                state.next_due = Instant::now();
            }
            Err(error) => self.block(error).await,
        }
        self.wake.notify_one();
    }

    async fn tick(&self) {
        let gate = self.mutation.lock().await;
        let prepared = {
            let mut state = poisoned_lock(&self.state);
            if !Self::may_sample(&state) || state.next_due > Instant::now() {
                return;
            }
            let revision = state.revision();
            match state.jobs.reserve(revision) {
                Ok(token) => Some((
                    token,
                    state.session.as_ref().unwrap().clone(),
                    state.intent.as_ref().unwrap().target.clone(),
                )),
                Err(message) => {
                    state.stopped = true;
                    state.error = Some(Error::new("intent_persistence", message));
                    None
                }
            }
        };
        drop(gate);
        let Some((token, session, target)) = prepared else {
            return;
        };
        let result = tokio::task::spawn_blocking(move || native::sample(session, &target))
            .await
            .unwrap_or_else(|_| {
                Err(Error::new(
                    "admission_unavailable",
                    "Native observer sample did not complete.",
                ))
            });
        let _gate = self.mutation.lock().await;
        if !poisoned_lock(&self.state).finish(token) {
            self.wake.notify_one();
            return;
        }
        match result {
            Ok(session) => {
                let mut state = poisoned_lock(&self.state);
                state.session = Some(session);
                state.next_due = Instant::now() + Duration::from_secs(5);
            }
            Err(error) => self.block(error).await,
        }
        self.wake.notify_one();
    }
}
