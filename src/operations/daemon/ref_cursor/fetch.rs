use super::*;

impl RefCursor {
    pub(super) fn enrich_fetch(&mut self, cmd: &mut NormalizedCommand) -> Result<(), GitAiError> {
        if let Err(error) = self.consume_fetch_transitions(cmd) {
            cmd.ref_changes.clear();
            tracing::warn!(%error, "best-effort fetch reflog enrichment failed");
        }
        Ok(())
    }

    pub(super) fn consume_fetch_transitions(
        &mut self,
        cmd: &mut NormalizedCommand,
    ) -> Result<(), GitAiError> {
        let Some(remote) = super::super::transport_notes::explicit_fetch_remote(cmd) else {
            return Ok(());
        };
        let prefix = format!("refs/remotes/{remote}/");
        let action = format!("fetch {remote}: ");
        let window = reflog_timestamp_window(cmd);
        for reference in self
            .discover_common_refs()?
            .into_iter()
            .filter(|r| r.starts_with(&prefix))
        {
            let key = common_key(&reference);
            let path = self.common_dir().join("logs").join(&reference);
            let start = self.reflog_start_offset(&key, &path)?;
            let mut candidates = read_reflog_entries(key, &path, &reference, start)?
                .into_iter()
                .filter(|entry| {
                    !self.entry_consumed(entry)
                        && entry.message.starts_with(&action)
                        && entry.timestamp_secs.is_some_and(|time| window.contains(time))
                        && valid_non_zero_oid(&entry.new)
                        // A newly created tracking ref has no captured cursor.
                        // Existing refs require a boundary, never their live tip.
                        && (start.is_some() || crate::operations::git::oid::is_zero_oid(&entry.old))
                });
            let Some(entry) = candidates.next() else {
                continue;
            };
            if candidates.next().is_some() {
                continue;
            }
            self.consume_entry(&entry)?;
            cmd.ref_changes.push(entry_to_ref_change(&entry));
        }
        Ok(())
    }
}
