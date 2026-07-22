#[allow(unused_imports)]
use super::*;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::operations::git::repo_state::common_dir_for_worktree;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;

impl ActorDaemonCoordinator {
    pub(crate) fn trace_invocation_participates_in_family_sequencer(
        primary_command: Option<&str>,
        argv: &[String],
    ) -> bool {
        primary_command.is_some_and(|cmd| {
            crate::operations::git::command_classification::git_invocation_participates_in_family_sequencer(
                cmd,
                &trace_invocation_command_args(Some(cmd), argv),
            )
        })
    }

    pub(crate) fn append_pending_root_entry(
        &self,
        family: &str,
        root_sid: &str,
        started_at_ns: u128,
    ) -> Result<(), GitAiError> {
        {
            let pending_slots = self.pending_root_slots_by_root.lock().map_err(|_| {
                PersistenceError::LockPoisoned {
                    what: "pending root slots map",
                }
            })?;
            if pending_slots.contains_key(root_sid) {
                return Ok(());
            }
        }

        let order = {
            let mut sequencers = self.family_sequencers_by_family.lock().map_err(|_| {
                PersistenceError::LockPoisoned {
                    what: "family sequencer map",
                }
            })?;
            let state =
                sequencers
                    .entry(family.to_string())
                    .or_insert_with(|| FamilySequencerState {
                        next_ordinal: 1,
                        entries: BTreeMap::new(),
                    });
            let order = FamilySequencerOrder {
                started_at_ns,
                ordinal: state.next_ordinal,
            };
            state.next_ordinal = state.next_ordinal.saturating_add(1);
            state
                .entries
                .insert(order, FamilySequencerEntry::PendingRoot);
            order
        };

        self.pending_root_slots_by_root
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending root slots map",
            })?
            .insert(
                root_sid.to_string(),
                PendingRootSlot {
                    family: family.to_string(),
                    order,
                },
            );
        Ok(())
    }

    pub(crate) fn take_pending_root_slot(
        &self,
        root_sid: &str,
    ) -> Result<Option<PendingRootSlot>, GitAiError> {
        self.pending_root_slots_by_root
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "pending root slots map",
            })
            .map_err(Into::into)
            .map(|mut slots| slots.remove(root_sid))
    }

    pub(crate) fn maybe_append_pending_root_from_trace_payload(
        &self,
        payload: &Value,
    ) -> Result<(), GitAiError> {
        let event = payload
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event == TRACE_CONNECTION_CLOSED_EVENT {
            return Ok(());
        }

        let Some(sid) = payload.get("sid").and_then(Value::as_str) else {
            return Ok(());
        };
        let root_sid = trace_root_sid(sid);
        if root_sid != sid {
            return Ok(());
        }

        let argv = trace_payload_effective_argv(payload);
        let primary_command =
            trace_payload_primary_command(payload).or_else(|| trace_argv_primary_command(&argv));
        if !Self::trace_invocation_participates_in_family_sequencer(
            primary_command.as_deref(),
            &argv,
        ) {
            return Ok(());
        }

        let Some(worktree) = trace_payload_worktree_hint(payload) else {
            return Ok(());
        };
        let Some(common_dir) = common_dir_for_worktree(&worktree) else {
            return Ok(());
        };
        let started_at_ns = trace_payload_root_started_at_ns(payload)
            .or_else(|| trace_payload_time_ns(payload))
            .unwrap_or_else(now_unix_nanos);
        let family = common_dir
            .canonicalize()
            .unwrap_or(common_dir)
            .to_string_lossy()
            .to_string();
        self.append_pending_root_entry(&family, root_sid, started_at_ns)
    }

    pub(crate) async fn append_ready_command_entry(
        &self,
        family: &str,
        command: crate::model::domain::NormalizedCommand,
    ) -> Result<(), GitAiError> {
        let exec_lock = self.side_effect_exec_lock(family)?;
        let _guard = exec_lock.lock().await;
        {
            let mut sequencers = self.family_sequencers_by_family.lock().map_err(|_| {
                PersistenceError::LockPoisoned {
                    what: "family sequencer map",
                }
            })?;
            let state =
                sequencers
                    .entry(family.to_string())
                    .or_insert_with(|| FamilySequencerState {
                        next_ordinal: 1,
                        entries: BTreeMap::new(),
                    });
            let order = FamilySequencerOrder {
                started_at_ns: command.started_at_ns,
                ordinal: state.next_ordinal,
            };
            state.next_ordinal = state.next_ordinal.saturating_add(1);
            state
                .entries
                .insert(order, FamilySequencerEntry::ReadyCommand(Box::new(command)));
        }
        self.drain_ready_family_sequencer_entries_locked(family)
            .await
    }

    pub(crate) async fn drain_ready_family_sequencer_entries(
        &self,
        family: &str,
    ) -> Result<(), GitAiError> {
        let exec_lock = self.side_effect_exec_lock(family)?;
        let _guard = exec_lock.lock().await;
        self.drain_ready_family_sequencer_entries_locked(family)
            .await
    }

    pub(crate) async fn drain_all_ready_family_sequencers(&self) -> Result<(), GitAiError> {
        let families = {
            let map = self.family_sequencers_by_family.lock().map_err(|_| {
                PersistenceError::LockPoisoned {
                    what: "family sequencer map",
                }
            })?;
            map.keys().cloned().collect::<Vec<_>>()
        };
        for family in families {
            self.drain_ready_family_sequencer_entries(&family).await?;
        }
        Ok(())
    }

    pub(crate) async fn drain_ready_family_sequencers_after_root_cleared(
        &self,
        family: Option<String>,
    ) -> Result<(), GitAiError> {
        if let Some(family) = family {
            self.drain_ready_family_sequencer_entries(&family).await
        } else {
            self.drain_all_ready_family_sequencers().await
        }
    }

    pub(crate) async fn replace_pending_root_entry(
        &self,
        root_sid: &str,
        replacement: FamilySequencerEntry,
    ) -> Result<Option<String>, GitAiError> {
        let Some(slot) = self.take_pending_root_slot(root_sid)? else {
            return Ok(None);
        };
        let family = slot.family.clone();
        let exec_lock = self.side_effect_exec_lock(&family)?;
        let _guard = exec_lock.lock().await;
        {
            let mut sequencers = self.family_sequencers_by_family.lock().map_err(|_| {
                PersistenceError::LockPoisoned {
                    what: "family sequencer map",
                }
            })?;
            let state = sequencers
                .entry(family.clone())
                .or_insert_with(|| FamilySequencerState {
                    next_ordinal: 1,
                    entries: BTreeMap::new(),
                });
            let Some(entry) = state.entries.get_mut(&slot.order) else {
                return Err(GitAiError::Generic(format!(
                    "missing pending root sequencer entry for sid={} family={} order={:?}",
                    root_sid, family, slot.order
                )));
            };
            match entry {
                FamilySequencerEntry::PendingRoot => {
                    *entry = replacement;
                }
                _ => {
                    return Err(GitAiError::Generic(format!(
                        "sequencer entry for sid={} family={} order={:?} was not pending",
                        root_sid, family, slot.order
                    )));
                }
            }
        }
        self.drain_ready_family_sequencer_entries_locked(&family)
            .await?;
        Ok(Some(family))
    }

    pub(crate) fn family_entry_blocked_by_prior_open_trace_root(
        &self,
        family: &str,
        started_at_ns: u128,
        entry_root_sid: Option<&str>,
    ) -> Result<bool, GitAiError> {
        let ingress =
            self.trace_ingress_state
                .lock()
                .map_err(|_| PersistenceError::LockPoisoned {
                    what: "trace ingress state",
                })?;

        for (root_sid, open_count) in &ingress.root_open_connections {
            if *open_count == 0 || entry_root_sid == Some(root_sid.as_str()) {
                continue;
            }
            if ingress.root_definitely_read_only.contains(root_sid) {
                continue;
            }
            if !ingress.root_mutating.get(root_sid).copied().unwrap_or(true) {
                continue;
            }
            if ingress
                .root_started_at_ns
                .get(root_sid)
                .copied()
                .is_some_and(|root_started| root_started > started_at_ns)
            {
                continue;
            }
            if ingress
                .root_families
                .get(root_sid)
                .is_none_or(|root_family| root_family == family)
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(crate) fn record_side_effect_error(
        &self,
        family: &str,
        seq: u64,
        error: &GitAiError,
    ) -> Result<(), GitAiError> {
        let mut map = self.side_effect_errors_by_family.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "side effect errors map",
            }
        })?;
        let family_errors = map.entry(family.to_string()).or_insert_with(BTreeMap::new);
        family_errors.insert(seq, error.to_string());
        while family_errors.len() > 256 {
            if let Some(oldest) = family_errors.keys().next().copied() {
                family_errors.remove(&oldest);
            } else {
                break;
            }
        }
        Ok(())
    }

    pub(crate) fn latest_side_effect_error(
        &self,
        family: &str,
    ) -> Result<Option<String>, GitAiError> {
        let map = self.side_effect_errors_by_family.lock().map_err(|_| {
            PersistenceError::LockPoisoned {
                what: "side effect errors map",
            }
        })?;
        Ok(map
            .get(family)
            .and_then(|errors| errors.iter().next_back().map(|(_, error)| error.clone())))
    }

    pub(crate) fn record_recent_replay_prerequisite(
        &self,
        family: &str,
        prerequisite: RecentReplayPrerequisite,
    ) -> Result<(), GitAiError> {
        const MAX_RECENT_REPLAY_PREREQUISITES_PER_FAMILY: usize = 256;

        let mut map = self
            .recent_replay_prerequisites_by_family
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "recent replay prerequisites map",
            })?;
        let entries = map.entry(family.to_string()).or_insert_with(VecDeque::new);
        entries.push_back(prerequisite);
        while entries.len() > MAX_RECENT_REPLAY_PREREQUISITES_PER_FAMILY {
            let _ = entries.pop_front();
        }
        Ok(())
    }

    pub(crate) fn maybe_append_test_completion_log(
        &self,
        family: &str,
        entry: &TestCompletionLogEntry,
    ) -> Result<(), GitAiError> {
        let Some(dir) = self.test_completion_log_dir.as_ref() else {
            return Ok(());
        };
        let _guard =
            self.test_completion_log_lock
                .lock()
                .map_err(|_| PersistenceError::LockPoisoned {
                    what: "test completion log",
                })?;

        fs::create_dir_all(dir)?;
        let mut hasher = Sha256::new();
        hasher.update(family.as_bytes());
        let digest = format!("{:x}", hasher.finalize());
        let path = dir.join(format!("{}.jsonl", &digest[..16]));
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        let line = serde_json::to_string(entry).map_err(GitAiError::from)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub(crate) fn append_command_completion_log(
        &self,
        family: &str,
        applied: &crate::model::domain::AppliedCommand,
        result: &Result<(), GitAiError>,
        error_order: u64,
    ) -> Result<(), GitAiError> {
        let sync_tracked =
            crate::operations::daemon::test_sync::tracks_primary_command_for_test_sync(
                applied.command.primary_command.as_deref(),
                &applied.command.invoked_args,
            );
        let test_sync_session =
            crate::operations::daemon::test_sync::test_sync_session_from_invocation(
                &parsed_invocation_for_normalized_command(&applied.command),
            );
        let log_entry = TestCompletionLogEntry {
            seq: applied.seq,
            family_key: family.to_string(),
            kind: "command".to_string(),
            primary_command: applied.command.primary_command.clone(),
            test_sync_session,
            exit_code: Some(applied.command.exit_code),
            sync_tracked,
            status: if result.is_ok() {
                "ok".to_string()
            } else {
                "error".to_string()
            },
            error: result.as_ref().err().map(|error| error.to_string()),
        };
        if let Err(error) = self.maybe_append_test_completion_log(family, &log_entry) {
            let _ = self.record_side_effect_error(family, error_order, &error);
            return Err(error);
        }
        Ok(())
    }
}
