use crate::model::repository::streams_db::{StreamRecord, StreamsDatabase};
use crate::model::stream_types::StreamError;
use crate::operations::streams::agent::{Agent, StreamDescriptor, get_all_agents};
use crate::operations::streams::sweep::{DiscoveredSession, SweepStrategy};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Work items discovered by the sweep.
#[derive(Debug, Clone)]
pub enum SweepItem {
    /// A session has at least one owned (non-shared) stream with new data.
    /// The worker should expand all streams for this session.
    Session {
        session_id: String,
        tool: String,
        canonical_path: PathBuf,
        external_session_id: String,
        external_parent_session_id: Option<String>,
    },
    /// A shared stream has new data. Process just this stream.
    SharedStream {
        tool: String,
        stream_kind: String,
        canonical_path: PathBuf,
    },
}

/// Orchestrates periodic sweeps across all registered agents.
///
/// Discovers sessions via each agent's filesystem scan, then checks staleness
/// separately for owned streams (per-session) and shared streams (once per agent).
pub struct SweepCoordinator {
    streams_db: Arc<StreamsDatabase>,
    agent_registry: Vec<(String, Box<dyn Agent>)>,
    lookback_days: Option<u32>,
}

impl SweepCoordinator {
    pub fn new(streams_db: Arc<StreamsDatabase>) -> Self {
        let lookback_days = crate::config::Config::get().transcript_streaming_lookback_days();
        Self {
            streams_db,
            agent_registry: get_all_agents(),
            lookback_days,
        }
    }

    #[cfg(test)]
    pub fn with_lookback(streams_db: Arc<StreamsDatabase>, lookback_days: Option<u32>) -> Self {
        Self {
            streams_db,
            agent_registry: get_all_agents(),
            lookback_days,
        }
    }

    /// Run a full sweep across all agents.
    pub fn run_sweep(&self) -> Result<Vec<SweepItem>, StreamError> {
        let mut items = Vec::new();

        for (agent_type, agent) in &self.agent_registry {
            if !matches!(agent.sweep_strategy(), SweepStrategy::Periodic(_)) {
                continue;
            }

            let discovered = match agent.discover_sessions() {
                Ok(sessions) => sessions,
                Err(e) => {
                    tracing::error!(
                        agent_type = %agent_type,
                        error = %e,
                        "agent discovery failed during sweep, skipping"
                    );
                    continue;
                }
            };

            let streams = agent.streams();
            let (shared, owned): (Vec<_>, Vec<_>) = streams.into_iter().partition(|s| s.shared);

            let canonical_paths: Vec<PathBuf> = discovered
                .iter()
                .map(|session| Self::canonicalize_path(&session.stream_path))
                .collect();

            // Per-session: only check owned streams for staleness
            for (session, canonical) in discovered.iter().zip(&canonical_paths) {
                match self.any_stream_stale(session, canonical, &owned) {
                    Ok(true) => items.push(SweepItem::Session {
                        session_id: session.session_id.clone(),
                        tool: session.tool.clone(),
                        canonical_path: canonical.clone(),
                        external_session_id: session.external_session_id.clone(),
                        external_parent_session_id: session.external_parent_session_id.clone(),
                    }),
                    Ok(false) => {}
                    Err(e) => tracing::error!(
                        session_id = %session.session_id,
                        error = %e,
                        "stream lookup failed during sweep, skipping session"
                    ),
                }
            }

            // Shared streams: resolve from each distinct path (e.g., Code vs Code Insiders
            // have separate globalStorage directories with independent OTEL DBs)
            let mut seen_shared_paths = HashSet::new();
            for (session, canonical) in discovered.iter().zip(&canonical_paths) {
                for stream in &shared {
                    let Some(item) = self.check_shared_stream(stream, canonical, &session.tool)?
                    else {
                        continue;
                    };
                    let SweepItem::SharedStream {
                        canonical_path: ref p,
                        ..
                    } = item
                    else {
                        continue;
                    };
                    if seen_shared_paths.insert(p.clone()) {
                        items.push(item);
                    }
                }
            }
        }

        Ok(items)
    }

    /// Returns true if any owned (non-shared) stream file is new or has changed.
    fn any_stream_stale(
        &self,
        session: &DiscoveredSession,
        canonical_path: &Path,
        streams: &[StreamDescriptor],
    ) -> Result<bool, StreamError> {
        for stream in streams {
            let Some(path) = stream.resolve_path(canonical_path) else {
                continue;
            };
            // Stat once: a file removed after discovery is skipped, not a sweep error.
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };

            let path_str = path.display().to_string();
            let stale = match self.streams_db.get_stream(
                &session.session_id,
                stream.stream_kind,
                &path_str,
            )? {
                None => self.is_within_lookback(&metadata),
                Some(existing) => Self::is_file_stale(&metadata, &existing),
            };
            if stale {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Check a single shared stream for processing.
    ///
    /// Shared streams (e.g., SQLite DBs in WAL mode) bypass file-metadata
    /// staleness checks because WAL writes don't update the main file's
    /// size/mtime. Instead, we always return them if the file exists —
    /// the watermark cursor inside the processing logic prevents re-processing.
    fn check_shared_stream(
        &self,
        stream: &StreamDescriptor,
        canonical_path: &Path,
        tool: &str,
    ) -> Result<Option<SweepItem>, StreamError> {
        let Some(path) = stream.resolve_path(canonical_path) else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }

        Ok(Some(SweepItem::SharedStream {
            tool: tool.to_string(),
            stream_kind: stream.stream_kind.to_string(),
            canonical_path: path,
        }))
    }

    fn is_within_lookback(&self, metadata: &std::fs::Metadata) -> bool {
        let Some(days) = self.lookback_days else {
            return true;
        };
        let cutoff =
            SystemTime::now() - std::time::Duration::from_secs(u64::from(days) * 24 * 60 * 60);
        metadata
            .modified()
            .map(|mtime| mtime >= cutoff)
            .unwrap_or(true)
    }

    fn is_file_stale(metadata: &std::fs::Metadata, existing: &StreamRecord) -> bool {
        let file_size = metadata.len() as i64;
        let modified = Self::get_modified_timestamp(metadata);
        file_size != existing.last_known_size
            || (modified.is_some() && modified != existing.last_modified)
    }

    fn canonicalize_path(path: &Path) -> PathBuf {
        crate::operations::git::canonicalize::canonicalize_or_self(path)
    }

    fn get_modified_timestamp(metadata: &std::fs::Metadata) -> Option<i64> {
        metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::stream_types::StreamFormat;
    use crate::model::stream_watermark::WatermarkType;

    #[test]
    fn stream_file_removed_after_discovery_is_not_a_sweep_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let db = Arc::new(StreamsDatabase::open(dir.path().join("streams.db")).unwrap());
        db.insert_stream(&StreamRecord {
            session_id: "s".to_string(),
            stream_kind: "transcript".to_string(),
            tool: "claude".to_string(),
            stream_path: path.display().to_string(),
            stream_format: StreamFormat::ClaudeJsonl,
            watermark_type: WatermarkType::ByteOffset,
            watermark_value: "0".to_string(),
            external_session_id: "s".to_string(),
            external_parent_session_id: None,
            first_seen_at: 0,
            last_processed_at: 0,
            last_known_size: 0,
            last_modified: None,
            processing_errors: 0,
            last_error: None,
            repo_work_dir: None,
        })
        .unwrap();
        let coordinator = SweepCoordinator::with_lookback(db, None);
        let session = DiscoveredSession {
            session_id: "s".to_string(),
            tool: "claude".to_string(),
            stream_path: path.clone(),
            external_session_id: "s".to_string(),
            external_parent_session_id: None,
        };
        let streams = [StreamDescriptor::identity_transcript(
            StreamFormat::ClaudeJsonl,
        )];
        assert!(
            coordinator
                .any_stream_stale(&session, &path, &streams)
                .unwrap(),
            "the recorded size differs from the file on disk"
        );

        std::fs::remove_file(&path).unwrap();
        assert!(
            !coordinator
                .any_stream_stale(&session, &path, &streams)
                .unwrap()
        );
    }
}
