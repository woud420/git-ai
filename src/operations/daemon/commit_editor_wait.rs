use super::*;
use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct RootEditorState {
    is_commit: bool,
    // A nested mutation may have moved refs already; an ambiguous editor
    // sequence cannot prove the parent is still blocked before its ref update.
    must_remain_ordered: bool,
    editor_child: Option<u64>,
}

#[derive(Default)]
pub(crate) struct CommitEditorWaits {
    roots: HashMap<String, RootEditorState>,
}

impl CommitEditorWaits {
    fn is_waiting(&self, root: &str) -> bool {
        self.roots
            .get(root)
            .is_some_and(|state| state.editor_child.is_some())
    }

    fn observe(&mut self, payload: &Value) -> Option<String> {
        let sid = payload.get("sid")?.as_str()?;
        let root = trace_root_sid(sid);
        let event = payload.get("event")?.as_str()?;
        let was_waiting = self.is_waiting(root);
        if event == "start" {
            let argv = trace_payload_argv(payload);
            let primary = trace_argv_primary_command(&argv);
            if sid == root && primary.as_deref() == Some("commit") {
                self.roots.entry(root.into()).or_default().is_commit = true;
            } else if sid != root && trace_invocation_may_mutate_refs(primary.as_deref(), &argv) {
                let state = self.roots.entry(root.into()).or_default();
                state.must_remain_ordered = true;
                state.editor_child = None;
            }
        } else if sid == root {
            let child_id = payload.get("child_id").and_then(Value::as_u64);
            if let Some(state) = self.roots.get_mut(root) {
                if event == "child_start"
                    && payload.get("child_class").and_then(Value::as_str) == Some("editor")
                    && state.is_commit
                    && !state.must_remain_ordered
                {
                    if child_id.is_none()
                        || state
                            .editor_child
                            .is_some_and(|previous| Some(previous) != child_id)
                    {
                        state.must_remain_ordered = true;
                        state.editor_child = None;
                    } else {
                        state.editor_child = child_id;
                    }
                } else if (event == "child_exit" && child_id == state.editor_child)
                    || event == "exit"
                    || event == "signal"
                {
                    state.editor_child = None;
                }
            }
            if is_terminal_root_trace_event(event, sid, root) {
                self.roots.remove(root);
            }
        }
        (!was_waiting && self.is_waiting(root)).then(|| root.to_string())
    }
}

impl ActorDaemonCoordinator {
    pub(crate) fn commit_editor_is_waiting(&self, root: &str) -> bool {
        self.commit_editor_waits
            .lock()
            .is_ok_and(|waits| waits.is_waiting(root))
    }

    pub(crate) fn clear_commit_editor_wait(&self, root: &str) -> Result<(), GitAiError> {
        self.commit_editor_waits
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "commit editor waits",
            })?
            .roots
            .remove(root);
        Ok(())
    }

    pub(crate) fn update_commit_editor_wait_state(
        self: &Arc<Self>,
        payload: &Value,
    ) -> Result<(), GitAiError> {
        // Only the asynchronous trace worker updates this state. The listener
        // keeps its existing ingestion work and never probes an editor process.
        let yielded = self
            .commit_editor_waits
            .lock()
            .map_err(|_| PersistenceError::LockPoisoned {
                what: "commit editor waits",
            })?
            .observe(payload);
        if let Some(root) = yielded {
            let family = self
                .pending_root_slots_by_root
                .lock()
                .map_err(|_| PersistenceError::LockPoisoned {
                    what: "pending root slots map",
                })?
                .get(&root)
                .map(|slot| slot.family.clone());
            if let Some(family) = family {
                self.schedule_family_drain_detached(&family)?;
            }
        }
        Ok(())
    }

    pub(crate) fn ready_family_orders(
        &self,
        family: &str,
        state: &FamilySequencerState,
    ) -> Result<Vec<FamilySequencerOrder>, GitAiError> {
        let mut ready = Vec::new();
        for (order, entry) in &state.entries {
            if let FamilySequencerEntry::PendingRoot { root_sid } = entry {
                if self.commit_editor_is_waiting(root_sid) {
                    continue;
                }
                break;
            }
            let root = match entry {
                FamilySequencerEntry::ReadyCommand(command) => Some(command.root_sid.as_str()),
                _ => None,
            };
            if self.family_entry_blocked_by_prior_open_trace_root(
                family,
                order.started_at_ns,
                root,
            )? {
                break;
            }
            ready.push(*order);
        }
        Ok(ready)
    }
}

#[cfg(test)]
mod tests;
