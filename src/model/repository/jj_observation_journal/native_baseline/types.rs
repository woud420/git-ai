use super::super::{JournalError, invalid};
use super::bounded;
use crate::model::jj_observation::{
    JJ_OBSERVATION_SCHEMA_VERSION, JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root,
    validate_ids, validate_profile, validate_source,
};
use serde::{Deserialize, Serialize};

pub(super) const RECORD_VERSION: u16 = 1;
pub(super) const DOMAIN: &str = "git-ai/jj/current-state-baseline/install/v1";
pub(super) const MODE: &str = "current_state";
// This is a fixed storage-format bound, independent of later reader profiles.
pub(super) const MAX_HEADS: usize = 32;
pub(crate) const MAX_BASELINE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_STATE_BYTES: usize = 128 * 1024;

#[derive(Serialize)]
pub(in crate::model::repository::jj_observation_journal) struct Request<'a> {
    record_version: u16,
    domain: &'static str,
    pub source_id: &'a str,
    reader_profile: &'a str,
    mode: &'static str,
    expected_native_generation: u64,
    captured_head_ids: Vec<&'a String>,
    anchors: Vec<&'a JjOperationEvidence>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredBaseline {
    record_version: u16,
    domain: String,
    pub source_id: String,
    pub reader_profile: String,
    mode: String,
    pub expected_native_generation: u64,
    #[serde(deserialize_with = "bounded::heads")]
    pub captured_head_ids: Vec<String>,
    #[serde(deserialize_with = "bounded::anchors")]
    pub anchors: Vec<JjOperationEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeBaselineState {
    state_version: u16,
    pub source_id: String,
    pub reader_profile: String,
    pub baseline_id: String,
    pub generation: u64,
    #[serde(deserialize_with = "bounded::heads")]
    pub captured_head_ids: Vec<String>,
}

impl<'a> Request<'a> {
    pub(in crate::model::repository::jj_observation_journal) fn new(
        source: &'a str,
        expected: u64,
        profile: &'a str,
        heads: &'a [String],
        anchors: &[&'a JjOperationEvidence],
    ) -> Result<Self, JournalError> {
        validate_source(source)?;
        if expected != 0 {
            return Err(invalid("native baseline expected generation must be zero"));
        }
        validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, profile)?;
        validate_contents(heads, anchors.iter().copied())?;
        let mut captured_head_ids: Vec<_> = heads.iter().collect();
        captured_head_ids.sort();
        let mut anchors = anchors.to_vec();
        anchors.sort_by(|left, right| left.operation_id.cmp(&right.operation_id));
        Ok(Self {
            record_version: RECORD_VERSION,
            domain: DOMAIN,
            source_id: source,
            reader_profile: profile,
            mode: MODE,
            expected_native_generation: expected,
            captured_head_ids,
            anchors,
        })
    }

    pub(super) fn state(&self, baseline_id: String) -> NativeBaselineState {
        NativeBaselineState {
            state_version: RECORD_VERSION,
            source_id: self.source_id.to_owned(),
            reader_profile: self.reader_profile.to_owned(),
            baseline_id,
            generation: 1,
            captured_head_ids: self
                .captured_head_ids
                .iter()
                .map(|id| (*id).clone())
                .collect(),
        }
    }

    pub(in crate::model::repository::jj_observation_journal) fn matches(
        &self,
        stored: &StoredBaseline,
    ) -> bool {
        stored.record_version == self.record_version
            && stored.domain == self.domain
            && stored.source_id == self.source_id
            && stored.reader_profile == self.reader_profile
            && stored.mode == self.mode
            && stored.expected_native_generation == self.expected_native_generation
            && self
                .captured_head_ids
                .iter()
                .map(|id| id.as_str())
                .eq(stored.captured_head_ids.iter().map(String::as_str))
            && self.anchors.iter().copied().eq(stored.anchors.iter())
    }
}

impl StoredBaseline {
    pub(super) fn validate(&self, source: &str) -> Result<(), JournalError> {
        if self.record_version != RECORD_VERSION
            || self.domain != DOMAIN
            || self.mode != MODE
            || self.expected_native_generation != 0
        {
            return Err(invalid("native baseline record contract invalid"));
        }
        if self.source_id != source {
            return Err(invalid("native baseline source mismatch"));
        }
        validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, &self.reader_profile)
            .map_err(|_| invalid("native baseline reader profile invalid"))?;
        validate_contents(&self.captured_head_ids, self.anchors.iter())?;
        if !strictly_sorted(&self.captured_head_ids)
            || self
                .anchors
                .windows(2)
                .any(|pair| pair[0].operation_id >= pair[1].operation_id)
        {
            return Err(invalid("native baseline collections are not canonical"));
        }
        Ok(())
    }
}

impl NativeBaselineState {
    pub(super) fn validate(&self, source: &str, baseline_id: &str) -> Result<(), JournalError> {
        if self.state_version != RECORD_VERSION
            || self.generation != 1
            || self.source_id != source
            || self.baseline_id != baseline_id
        {
            return Err(invalid(
                "native baseline state identity or generation invalid",
            ));
        }
        validate_source(&self.baseline_id)
            .map_err(|_| invalid("native baseline digest identity invalid"))?;
        validate_profile(JJ_OBSERVATION_SCHEMA_VERSION, &self.reader_profile)
            .map_err(|_| invalid("native baseline state reader profile invalid"))?;
        validate_heads(&self.captured_head_ids)?;
        if !strictly_sorted(&self.captured_head_ids) {
            return Err(invalid("native baseline state heads are not canonical"));
        }
        Ok(())
    }

    pub(super) fn matches(&self, record: &StoredBaseline) -> bool {
        self.source_id == record.source_id
            && self.reader_profile == record.reader_profile
            && self.captured_head_ids == record.captured_head_ids
    }
}

fn strictly_sorted(ids: &[String]) -> bool {
    ids.windows(2).all(|pair| pair[0] < pair[1])
}

fn validate_heads(heads: &[String]) -> Result<(), JournalError> {
    validate_ids(heads, MAX_HEADS)
        .map_err(|_| invalid("native baseline head identities invalid"))?;
    if heads.is_empty() || heads.iter().any(|head| is_root(head)) {
        return Err(invalid("native baseline head set invalid"));
    }
    Ok(())
}

fn validate_contents<'a>(
    heads: &[String],
    anchors: impl ExactSizeIterator<Item = &'a JjOperationEvidence>,
) -> Result<(), JournalError> {
    validate_heads(heads)?;
    if anchors.len() != heads.len() {
        return Err(invalid("native baseline anchor count mismatch"));
    }
    let mut seen = std::collections::HashSet::with_capacity(heads.len());
    let mut raw_bytes = 0usize;
    for anchor in anchors {
        anchor.validate()?;
        raw_bytes = raw_bytes
            .checked_add(anchor.operation_bytes.len())
            .and_then(|size| size.checked_add(anchor.view_bytes.len()))
            .filter(|size| *size <= MAX_JJ_OBSERVATION_BATCH_BYTES)
            .ok_or_else(|| invalid("native baseline raw evidence byte limit exceeded"))?;
        if !seen.insert(&anchor.operation_id) || !heads.contains(&anchor.operation_id) {
            return Err(invalid("native baseline anchor identities invalid"));
        }
    }
    Ok(())
}
