//! Token-usage aggregation: per-message and per-session (codex) accumulation,
//! model pricing estimates, and the week-over-week spend comparison.

use crate::metrics::attrs::attr_pos;
use crate::metrics::events::session_event_pos;
use crate::metrics::local_stats::SESSION_RAW_JSON_KEY;
use crate::metrics::local_stats::types::{TokenModelStat, TokenSummary, WowSpend};
use crate::metrics::pos_encoded::sparse_get_string;
use crate::metrics::types::MetricEvent;
use chrono::NaiveDate;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::buckets::ts_to_local;

/// Per-model token accumulator.
#[derive(Debug, Default, Clone)]
pub(super) struct TokenAccum {
    pub(super) input: u64,
    pub(super) output: u64,
    pub(super) cache_read: u64,
    pub(super) cache_creation: u64,
}

/// Per-session codex accumulator. Codex reports *cumulative* session totals on
/// each `token_count` event, so we track the max of each raw field. The model
/// name arrives on a separate event (`payload.model`), captured when seen.
#[derive(Debug, Default, Clone)]
pub(super) struct CodexSessionAccum {
    pub(super) model: Option<String>,
    /// Unix timestamp of the latest token-usage event seen for this session
    /// (WoW bucketing).
    pub(super) last_usage_ts: u32,
    /// Cumulative input tokens (includes cached).
    pub(super) input_tokens: u64,
    /// Cumulative cached input tokens (subset of input_tokens).
    pub(super) cached_input_tokens: u64,
    /// Cumulative output tokens (includes reasoning).
    pub(super) output_tokens: u64,
}

impl CodexSessionAccum {
    /// Map codex token fields onto the shared `TokenAccum` schema.
    ///
    /// Codex `input_tokens` *includes* cached tokens, so non-cached input is
    /// the difference. Codex has no cache-creation concept.
    pub(super) fn to_token_accum(&self) -> TokenAccum {
        TokenAccum {
            input: self.input_tokens.saturating_sub(self.cached_input_tokens),
            output: self.output_tokens,
            cache_read: self.cached_input_tokens,
            cache_creation: 0,
        }
    }
}

/// Per-million-token pricing for a model (USD).
struct ModelPricing {
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
}

/// Built-in pricing estimate, matched by substring of the model id.
/// Rates are public Anthropic list prices (USD per million tokens) and are
/// only an estimate — they go stale as pricing changes.
fn pricing_for(model: &str) -> Option<ModelPricing> {
    let m = model.to_lowercase();
    if m.contains("opus") {
        Some(ModelPricing {
            input: 15.0,
            output: 75.0,
            cache_write: 18.75,
            cache_read: 1.5,
        })
    } else if m.contains("sonnet") {
        Some(ModelPricing {
            input: 3.0,
            output: 15.0,
            cache_write: 3.75,
            cache_read: 0.3,
        })
    } else if m.contains("haiku") {
        Some(ModelPricing {
            input: 0.8,
            output: 4.0,
            cache_write: 1.0,
            cache_read: 0.08,
        })
    } else if m.contains("gpt") {
        // OpenAI GPT-5 family estimate; cache_write unused (codex reports no
        // cache-creation tokens).
        Some(ModelPricing {
            input: 1.25,
            output: 10.0,
            cache_write: 1.25,
            cache_read: 0.125,
        })
    } else {
        None
    }
}

fn estimate_cost(acc: &TokenAccum, pricing: &ModelPricing) -> f64 {
    (acc.input as f64 * pricing.input
        + acc.output as f64 * pricing.output
        + acc.cache_creation as f64 * pricing.cache_write
        + acc.cache_read as f64 * pricing.cache_read)
        / 1_000_000.0
}

/// Shorten a model id for display: strip a trailing "-YYYYMMDD" date snapshot
/// (e.g. "claude-haiku-4-5-20251001" -> "claude-haiku-4-5").
fn shorten_model(model: &str) -> String {
    match model.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) => {
            head.to_string()
        }
        _ => model.to_string(),
    }
}

/// Fold a set of message-usage entries into a per-model cost estimate (USD).
/// Used to compute each WoW half independently.
fn cost_for_message_slice(entries: impl Iterator<Item = (String, TokenAccum)>) -> f64 {
    let mut model_totals: HashMap<String, TokenAccum> = HashMap::new();
    for (model, acc) in entries {
        let e = model_totals.entry(model).or_default();
        e.input += acc.input;
        e.output += acc.output;
        e.cache_read += acc.cache_read;
        e.cache_creation += acc.cache_creation;
    }
    model_totals
        .iter()
        .filter_map(|(model, acc)| pricing_for(model).map(|p| estimate_cost(acc, &p)))
        .sum()
}

/// Returns the aggregate token summary plus a per-local-day spend map (USD),
/// derived from the same per-message / per-session data so the daily series
/// reconciles with the headline total.
pub(super) fn build_token_summary(
    message_usage: HashMap<String, (String, TokenAccum, u32, String)>,
    codex_sessions: HashMap<String, CodexSessionAccum>,
    now_ts: u32,
    since_ts: u32,
) -> (TokenSummary, BTreeMap<NaiveDate, f64>) {
    // Per-day spend, bucketed by the message/session timestamp's local date.
    let mut cost_by_day: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    // Week-over-week split: "this week" = last 7 days, "last week" = 7–14 days ago.
    // Only meaningful when the query window covers at least 14 days; otherwise
    // last-week events were never fetched and last_week_cost would be 0 by
    // omission rather than by fact.
    let this_week_start = now_ts.saturating_sub(7 * 24 * 3600);
    let last_week_start = now_ts.saturating_sub(14 * 24 * 3600);
    let wow_eligible = since_ts <= last_week_start;

    let mut this_week_msgs: Vec<(String, TokenAccum)> = Vec::new();
    let mut last_week_msgs: Vec<(String, TokenAccum)> = Vec::new();

    // Fold per-message (deduped, max) usage into per-model totals.
    // Key by shorten_model() so date-snapshot variants (e.g. claude-sonnet-4-6-20250101
    // and claude-sonnet-4-6-20250201) are folded into a single display row.
    let mut model_tokens: HashMap<String, TokenAccum> = HashMap::new();
    let mut model_session_ids: HashMap<String, HashSet<String>> = HashMap::new();
    for (_id, (model, acc, ts, sid)) in message_usage {
        let short = shorten_model(&model);

        if let Some(pricing) = pricing_for(&short) {
            *cost_by_day
                .entry(ts_to_local(ts).date_naive())
                .or_insert(0.0) += estimate_cost(&acc, &pricing);
        }

        let entry = model_tokens.entry(short.clone()).or_default();
        entry.input += acc.input;
        entry.output += acc.output;
        entry.cache_read += acc.cache_read;
        entry.cache_creation += acc.cache_creation;

        if !sid.is_empty() {
            model_session_ids
                .entry(short.clone())
                .or_default()
                .insert(sid);
        }

        if ts >= this_week_start {
            this_week_msgs.push((short, acc));
        } else if ts >= last_week_start {
            last_week_msgs.push((short, acc));
        }
    }

    // Fold per-session codex totals into per-model totals, mapping codex's
    // field semantics onto ours: codex input_tokens *includes* cached, so the
    // non-cached input is the difference; cached maps to cache_read; codex has
    // no cache-creation concept.
    let mut this_week_codex: Vec<(String, TokenAccum)> = Vec::new();
    let mut last_week_codex: Vec<(String, TokenAccum)> = Vec::new();

    for (sid, acc) in codex_sessions {
        let model = acc.model.clone().unwrap_or_else(|| "codex".to_string());
        let short = shorten_model(&model);
        let mapped = acc.to_token_accum();

        if let Some(pricing) = pricing_for(&short) {
            *cost_by_day
                .entry(ts_to_local(acc.last_usage_ts).date_naive())
                .or_insert(0.0) += estimate_cost(&mapped, &pricing);
        }

        let entry = model_tokens.entry(short.clone()).or_default();
        entry.input += mapped.input;
        entry.output += mapped.output;
        entry.cache_read += mapped.cache_read;
        model_session_ids
            .entry(short.clone())
            .or_default()
            .insert(sid);

        if acc.last_usage_ts >= this_week_start {
            this_week_codex.push((short, mapped));
        } else if acc.last_usage_ts >= last_week_start {
            last_week_codex.push((short, mapped));
        }
    }

    // Compute WoW spend from the two half-slices.
    let this_week_cost = cost_for_message_slice(this_week_msgs.into_iter().chain(this_week_codex));
    let last_week_cost = cost_for_message_slice(last_week_msgs.into_iter().chain(last_week_codex));

    let wow_spend = if wow_eligible && (this_week_cost > 0.0 || last_week_cost > 0.0) {
        let (change_pct, new_this_week) = if last_week_cost > 0.0 {
            (
                Some((this_week_cost - last_week_cost) / last_week_cost * 100.0),
                false,
            )
        } else {
            (None, true)
        };
        Some(WowSpend {
            this_week_usd: this_week_cost,
            last_week_usd: last_week_cost,
            change_pct,
            new_this_week,
        })
    } else {
        None
    };

    let mut summary = TokenSummary::default();
    let mut by_model: Vec<TokenModelStat> = Vec::new();

    for (model, acc) in model_tokens {
        // Skip placeholder/synthetic entries that carried no real token counts.
        if acc.input == 0 && acc.output == 0 && acc.cache_read == 0 && acc.cache_creation == 0 {
            continue;
        }

        summary.input += acc.input;
        summary.output += acc.output;
        summary.cache_read += acc.cache_read;
        summary.cache_creation += acc.cache_creation;

        let cost = pricing_for(&model).map(|p| estimate_cost(&acc, &p));
        if let Some(c) = cost {
            summary.estimated_cost_usd += c;
        }

        let cache_total = acc.cache_read + acc.cache_creation;
        let cache_hit_ratio = if cache_total > 0 {
            Some(acc.cache_read as f64 / cache_total as f64)
        } else {
            None
        };

        let sessions = model_session_ids
            .get(&model)
            .map(|s| s.len() as u32)
            .unwrap_or(0);
        by_model.push(TokenModelStat {
            model, // already shortened at insertion into model_tokens
            sessions,
            input: acc.input,
            output: acc.output,
            cache_read: acc.cache_read,
            cache_creation: acc.cache_creation,
            estimated_cost_usd: cost,
            cache_hit_ratio,
        });
    }

    by_model.sort_by_key(|m| Reverse(m.input + m.output + m.cache_read + m.cache_creation));
    summary.by_model = by_model;
    summary.wow_spend = wow_spend;
    (summary, cost_by_day)
}

/// Extract token usage from a session event's raw transcript JSON (position 0).
/// Only assistant messages carry usage. Keyed by message id, keeping the
/// field-wise max across re-emitted copies (streaming partials report lower
/// counts than the final message). `record_ts` is stored on first insertion
/// for week-over-week bucketing.
pub(super) fn aggregate_session_tokens(
    event: &MetricEvent,
    record_ts: u32,
    session_id: String,
    message_usage: &mut HashMap<String, (String, TokenAccum, u32, String)>,
) {
    debug_assert_eq!(session_event_pos::RAW_JSON, 0);
    let Some(raw) = event.values.get(SESSION_RAW_JSON_KEY) else {
        return;
    };
    let Some(message) = raw.get("message") else {
        return;
    };
    if message.get("role").and_then(|r| r.as_str()) != Some("assistant") {
        return;
    }
    let Some(usage) = message.get("usage") else {
        return;
    };
    let Some(id) = message.get("id").and_then(|i| i.as_str()) else {
        return;
    };

    let model = message
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();

    let get = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);

    let (stored_model, acc, _ts, stored_sid) =
        message_usage.entry(id.to_string()).or_insert_with(|| {
            (
                model.clone(),
                TokenAccum::default(),
                record_ts,
                session_id.clone(),
            )
        });
    // If the entry was created with an "unknown" placeholder model (e.g. from a
    // streaming partial that arrived before the final event), upgrade it now.
    if stored_model == "unknown" && model != "unknown" {
        *stored_model = model;
    }
    // Similarly, upgrade an empty session_id once a real one is available.
    if stored_sid.is_empty() && !session_id.is_empty() {
        *stored_sid = session_id;
    }
    // Field-wise max: input/cache are fixed per message; output grows while
    // streaming, so the final (largest) value is authoritative.
    acc.input = acc.input.max(get("input_tokens"));
    acc.output = acc.output.max(get("output_tokens"));
    acc.cache_read = acc.cache_read.max(get("cache_read_input_tokens"));
    acc.cache_creation = acc.cache_creation.max(get("cache_creation_input_tokens"));
}

/// Extract token usage from a codex session event. Codex emits `token_count`
/// events carrying cumulative `payload.info.total_token_usage`, and reports its
/// model on a separate event via `payload.model`. Both are keyed by session id;
/// cumulative totals are tracked as a per-session max.
pub(super) fn aggregate_codex_tokens(
    event: &MetricEvent,
    record_ts: u32,
    codex_sessions: &mut HashMap<String, CodexSessionAccum>,
) {
    let Some(session_id) = sparse_get_string(&event.attrs, attr_pos::SESSION_ID).flatten() else {
        return;
    };
    debug_assert_eq!(session_event_pos::RAW_JSON, 0);
    let Some(raw) = event.values.get(SESSION_RAW_JSON_KEY) else {
        return;
    };
    let Some(payload) = raw.get("payload") else {
        return;
    };

    let entry = codex_sessions.entry(session_id).or_default();

    // Capture the model name when it appears (not on token_count events).
    if let Some(model) = payload.get("model").and_then(|m| m.as_str())
        && entry.model.is_none()
    {
        entry.model = Some(model.to_string());
    }

    // Cumulative session totals; keep the running max.
    if let Some(usage) = payload.get("info").and_then(|i| i.get("total_token_usage")) {
        let get = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
        entry.last_usage_ts = entry.last_usage_ts.max(record_ts);
        entry.input_tokens = entry.input_tokens.max(get("input_tokens"));
        entry.cached_input_tokens = entry.cached_input_tokens.max(get("cached_input_tokens"));
        entry.output_tokens = entry.output_tokens.max(get("output_tokens"));
    }
}
