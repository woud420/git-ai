use super::{CheckpointJournalError, sha256_hex};
use crate::model::attribution::{Attribution, LineAttribution};
use crate::model::working_log::{
    AgentId, Checkpoint, CheckpointKind, CheckpointLineStats, KnownHumanMetadata, WorkingLogEntry,
};
use serde::ser::{SerializeSeq, SerializeTuple};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

pub(super) const VERSION: u64 = 2;
pub(super) const API_VERSION: &str = "checkpoint/1.0.0";
const CHECKSUM_MARKER: &[u8] = b",\"c\":\"";

// Readers remain compatible with legacy and v1 records. Writing v2 is a
// one-way migration because older binaries cannot decode this compact shape.

#[derive(Serialize)]
struct UnsignedRecord<'a> {
    #[serde(rename = "a")]
    author: &'a str,
    #[serde(rename = "d")]
    diff: &'a str,
    #[serde(rename = "e")]
    entries: EntriesRef<'a>,
    #[serde(rename = "g", skip_serializing_if = "Option::is_none")]
    git_ai_version: Option<&'a str>,
    #[serde(rename = "h", skip_serializing_if = "Option::is_none")]
    known_human_metadata: Option<KnownHumanMetadataRef<'a>>,
    #[serde(rename = "i", skip_serializing_if = "Option::is_none")]
    agent_id: Option<AgentIdRef<'a>>,
    #[serde(rename = "k")]
    kind: u8,
    #[serde(rename = "m", skip_serializing_if = "Option::is_none")]
    agent_metadata: Option<BTreeMap<&'a str, &'a str>>,
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    trace_id: Option<&'a str>,
    #[serde(rename = "s")]
    line_stats: LineStatsRef,
    #[serde(rename = "t")]
    timestamp: u64,
    #[serde(rename = "v")]
    version: u64,
    #[serde(rename = "y", skip_serializing_if = "Option::is_none")]
    delivery_id: Option<&'a str>,
}

struct EntriesRef<'a>(&'a [WorkingLogEntry]);

struct EntryRef<'a>(&'a WorkingLogEntry);

struct AttributionsRef<'a>(&'a [Attribution]);

struct AttributionRef<'a>(&'a Attribution);

struct LineAttributionsRef<'a>(&'a [LineAttribution]);

struct LineAttributionRef<'a>(&'a LineAttribution);

#[derive(Serialize)]
struct AgentIdRef<'a>(&'a str, &'a str, &'a str);

#[derive(Serialize)]
struct KnownHumanMetadataRef<'a>(&'a str, &'a str, &'a str);

#[derive(Serialize)]
struct LineStatsRef(u32, u32, u32, u32);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedRecord {
    #[serde(rename = "a")]
    author: String,
    #[serde(rename = "d")]
    diff: String,
    #[serde(rename = "e")]
    entries: Vec<OwnedEntry>,
    #[serde(rename = "g")]
    git_ai_version: Option<String>,
    #[serde(rename = "h")]
    known_human_metadata: Option<OwnedKnownHumanMetadata>,
    #[serde(rename = "i")]
    agent_id: Option<OwnedAgentId>,
    #[serde(rename = "k")]
    kind: u8,
    #[serde(rename = "m")]
    agent_metadata: Option<BTreeMap<String, String>>,
    #[serde(rename = "r")]
    trace_id: Option<String>,
    #[serde(rename = "s")]
    line_stats: OwnedLineStats,
    #[serde(rename = "t")]
    timestamp: u64,
    #[serde(rename = "v")]
    version: u64,
    #[serde(rename = "y")]
    delivery_id: Option<String>,
    #[serde(rename = "c")]
    checksum: String,
}

#[derive(Deserialize)]
struct OwnedEntry(
    String,
    String,
    Vec<OwnedAttribution>,
    Vec<OwnedLineAttribution>,
);

#[derive(Deserialize)]
struct OwnedAttribution(usize, usize, String, u128);

#[derive(Deserialize)]
struct OwnedLineAttribution(u32, u32, String, Option<String>);

#[derive(Deserialize)]
struct OwnedAgentId(String, String, String);

#[derive(Deserialize)]
struct OwnedKnownHumanMetadata(String, String, String);

#[derive(Deserialize)]
struct OwnedLineStats(u32, u32, u32, u32);

pub(super) fn encode(checkpoint: &Checkpoint) -> Result<Vec<u8>, CheckpointJournalError> {
    if checkpoint.api_version != API_VERSION {
        return Err(CheckpointJournalError::Integrity(format!(
            "unsupported checkpoint api version {}",
            checkpoint.api_version
        )));
    }

    let unsigned = UnsignedRecord {
        author: &checkpoint.author,
        diff: &checkpoint.diff,
        entries: EntriesRef(&checkpoint.entries),
        git_ai_version: checkpoint.git_ai_version.as_deref(),
        known_human_metadata: checkpoint
            .known_human_metadata
            .as_ref()
            .map(KnownHumanMetadataRef::from),
        agent_id: checkpoint.agent_id.as_ref().map(AgentIdRef::from),
        kind: encode_kind(checkpoint.kind),
        agent_metadata: checkpoint.agent_metadata.as_ref().map(|metadata| {
            metadata
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect()
        }),
        trace_id: checkpoint.trace_id.as_deref(),
        line_stats: LineStatsRef::from(&checkpoint.line_stats),
        timestamp: checkpoint.timestamp,
        version: VERSION,
        delivery_id: checkpoint.delivery_id.as_deref(),
    };
    let unsigned = serde_json::to_vec(&unsigned)?;
    sign(unsigned)
}

pub(super) fn decode(bytes: &[u8]) -> Result<Checkpoint, CheckpointJournalError> {
    let terminal = terminal_checksum(bytes)?.ok_or_else(|| {
        CheckpointJournalError::Integrity(
            "checkpoint record checksum is missing or invalid".to_string(),
        )
    })?;
    decode_terminal(bytes, terminal)
}

pub(super) fn decode_if_terminal(
    bytes: &[u8],
) -> Result<Option<Checkpoint>, CheckpointJournalError> {
    terminal_checksum(bytes)?
        .map(|terminal| decode_terminal(bytes, terminal))
        .transpose()
}

fn decode_terminal(
    bytes: &[u8],
    (marker_start, supplied_checksum): (usize, &str),
) -> Result<Checkpoint, CheckpointJournalError> {
    if raw_checksum(bytes, marker_start) != supplied_checksum {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record checksum mismatch".to_string(),
        ));
    }

    let record: SignedRecord = serde_json::from_slice(bytes)?;
    if record.version != VERSION {
        return Err(CheckpointJournalError::Integrity(format!(
            "unsupported checkpoint record version {}",
            record.version
        )));
    }
    if record.checksum != supplied_checksum {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record checksum mismatch".to_string(),
        ));
    }

    let mut checkpoint = Checkpoint::new(
        decode_kind(record.kind)?,
        record.diff,
        record.author,
        record
            .entries
            .into_iter()
            .map(WorkingLogEntry::from)
            .collect(),
    );
    checkpoint.timestamp = record.timestamp;
    checkpoint.agent_id = record.agent_id.map(AgentId::from);
    checkpoint.agent_metadata = record
        .agent_metadata
        .map(|metadata| metadata.into_iter().collect::<HashMap<_, _>>());
    checkpoint.line_stats = CheckpointLineStats::from(record.line_stats);
    checkpoint.api_version = API_VERSION.to_string();
    checkpoint.git_ai_version = record.git_ai_version;
    checkpoint.known_human_metadata = record.known_human_metadata.map(KnownHumanMetadata::from);
    checkpoint.trace_id = record.trace_id;
    checkpoint.delivery_id = record.delivery_id;
    checkpoint.mark_journal_record_version(VERSION);
    Ok(checkpoint)
}

pub(super) fn is_record(bytes: &[u8]) -> bool {
    terminal_checksum(bytes).is_ok_and(|checksum| checksum.is_some())
        || serde_json::from_slice::<Value>(bytes)
            .ok()
            .is_some_and(|value| claims_version(&value))
}

pub(super) fn claims_version(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("v"))
}

pub(super) fn claims_checksum(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("c"))
}

fn sign(mut unsigned: Vec<u8>) -> Result<Vec<u8>, CheckpointJournalError> {
    let checksum = sha256_hex(&unsigned);
    if unsigned.pop() != Some(b'}') {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record is not a JSON object".to_string(),
        ));
    }
    unsigned.extend_from_slice(CHECKSUM_MARKER);
    unsigned.extend_from_slice(checksum.as_bytes());
    unsigned.extend_from_slice(b"\"}");
    Ok(unsigned)
}

fn terminal_checksum(bytes: &[u8]) -> Result<Option<(usize, &str)>, CheckpointJournalError> {
    let Some(marker_start) = bytes
        .windows(CHECKSUM_MARKER.len())
        .rposition(|window| window == CHECKSUM_MARKER)
    else {
        return Ok(None);
    };
    let checksum_start = marker_start + CHECKSUM_MARKER.len();
    let checksum_end = checksum_start + 64;
    if bytes.get(checksum_end..) != Some(b"\"}") {
        return Ok(None);
    }
    let supplied = &bytes[checksum_start..checksum_end];
    if !supplied
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(CheckpointJournalError::Integrity(
            "checkpoint record checksum is not lowercase hexadecimal".to_string(),
        ));
    }

    let supplied = std::str::from_utf8(supplied).map_err(|_| {
        CheckpointJournalError::Integrity("checkpoint record checksum is not UTF-8".to_string())
    })?;
    Ok(Some((marker_start, supplied)))
}

fn raw_checksum(bytes: &[u8], marker_start: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(&bytes[..marker_start]);
    hasher.update(b"}");
    format!("{:x}", hasher.finalize())
}

fn encode_kind(kind: CheckpointKind) -> u8 {
    match kind {
        CheckpointKind::Human => 0,
        CheckpointKind::AiAgent => 1,
        CheckpointKind::AiTab => 2,
        CheckpointKind::KnownHuman => 3,
    }
}

fn decode_kind(kind: u8) -> Result<CheckpointKind, CheckpointJournalError> {
    match kind {
        0 => Ok(CheckpointKind::Human),
        1 => Ok(CheckpointKind::AiAgent),
        2 => Ok(CheckpointKind::AiTab),
        3 => Ok(CheckpointKind::KnownHuman),
        _ => Err(CheckpointJournalError::Integrity(format!(
            "unsupported checkpoint kind code {kind}"
        ))),
    }
}

impl Serialize for EntriesRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for entry in self.0 {
            sequence.serialize_element(&EntryRef(entry))?;
        }
        sequence.end()
    }
}

impl Serialize for EntryRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(4)?;
        tuple.serialize_element(&self.0.file)?;
        tuple.serialize_element(&self.0.blob_sha)?;
        tuple.serialize_element(&AttributionsRef(&self.0.attributions))?;
        tuple.serialize_element(&LineAttributionsRef(&self.0.line_attributions))?;
        tuple.end()
    }
}

impl Serialize for AttributionsRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for attribution in self.0 {
            sequence.serialize_element(&AttributionRef(attribution))?;
        }
        sequence.end()
    }
}

impl Serialize for AttributionRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(4)?;
        tuple.serialize_element(&self.0.start)?;
        tuple.serialize_element(&self.0.end)?;
        tuple.serialize_element(&self.0.author_id)?;
        tuple.serialize_element(&self.0.ts)?;
        tuple.end()
    }
}

impl Serialize for LineAttributionsRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for attribution in self.0 {
            sequence.serialize_element(&LineAttributionRef(attribution))?;
        }
        sequence.end()
    }
}

impl Serialize for LineAttributionRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(4)?;
        tuple.serialize_element(&self.0.start_line)?;
        tuple.serialize_element(&self.0.end_line)?;
        tuple.serialize_element(&self.0.author_id)?;
        tuple.serialize_element(&self.0.overrode.as_deref())?;
        tuple.end()
    }
}

impl<'a> From<&'a AgentId> for AgentIdRef<'a> {
    fn from(agent: &'a AgentId) -> Self {
        Self(&agent.tool, &agent.id, &agent.model)
    }
}

impl<'a> From<&'a KnownHumanMetadata> for KnownHumanMetadataRef<'a> {
    fn from(metadata: &'a KnownHumanMetadata) -> Self {
        Self(
            &metadata.editor,
            &metadata.editor_version,
            &metadata.extension_version,
        )
    }
}

impl From<&CheckpointLineStats> for LineStatsRef {
    fn from(stats: &CheckpointLineStats) -> Self {
        Self(
            stats.additions,
            stats.deletions,
            stats.additions_sloc,
            stats.deletions_sloc,
        )
    }
}

impl From<OwnedEntry> for WorkingLogEntry {
    fn from(entry: OwnedEntry) -> Self {
        Self::new(
            entry.0,
            entry.1,
            entry.2.into_iter().map(Attribution::from).collect(),
            entry.3.into_iter().map(LineAttribution::from).collect(),
        )
    }
}

impl From<OwnedAttribution> for Attribution {
    fn from(attribution: OwnedAttribution) -> Self {
        Self::new(attribution.0, attribution.1, attribution.2, attribution.3)
    }
}

impl From<OwnedLineAttribution> for LineAttribution {
    fn from(attribution: OwnedLineAttribution) -> Self {
        Self::new(attribution.0, attribution.1, attribution.2, attribution.3)
    }
}

impl From<OwnedAgentId> for AgentId {
    fn from(agent: OwnedAgentId) -> Self {
        Self {
            tool: agent.0,
            id: agent.1,
            model: agent.2,
        }
    }
}

impl From<OwnedKnownHumanMetadata> for KnownHumanMetadata {
    fn from(metadata: OwnedKnownHumanMetadata) -> Self {
        Self {
            editor: metadata.0,
            editor_version: metadata.1,
            extension_version: metadata.2,
        }
    }
}

impl From<OwnedLineStats> for CheckpointLineStats {
    fn from(stats: OwnedLineStats) -> Self {
        Self {
            additions: stats.0,
            deletions: stats.1,
            additions_sloc: stats.2,
            deletions_sloc: stats.3,
        }
    }
}
