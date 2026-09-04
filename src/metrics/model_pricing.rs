//! Offline model pricing from a checked-in models.dev catalog snapshot.
//!
//! Runtime lookup never performs network or user-cache I/O. Maintainers refresh
//! the embedded data explicitly with `scripts/refresh_model_pricing.sh`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

const EMBEDDED_SNAPSHOT: &str = include_str!("models_dev_pricing_snapshot.json");

#[cfg(test)]
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";

#[cfg(test)]
/// Only first-party providers may define canonical model IDs; reseller rates
/// can otherwise shadow the same ID with different pricing.
const PROVIDER_ALLOWLIST: [&str; 8] = [
    "anthropic",
    "openai",
    "google",
    "xai",
    "deepseek",
    "mistral",
    "moonshotai",
    "zai",
];

const FAMILY_FALLBACK_TOKENS: [&str; 7] =
    ["opus", "sonnet", "haiku", "fable", "gpt", "gemini", "grok"];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct ModelPricing {
    pub(crate) input: f64,
    pub(crate) output: f64,
    #[serde(default)]
    pub(crate) cache_read: f64,
    #[serde(default)]
    pub(crate) cache_write: f64,
}

struct PricingCatalog {
    entries: BTreeMap<String, ModelPricing>,
}

impl PricingCatalog {
    fn from_snapshot_json(json: &str) -> Result<Self, String> {
        let entries = serde_json::from_str(json).map_err(|error| error.to_string())?;
        Self::from_entries(entries)
    }

    fn from_entries(entries: BTreeMap<String, ModelPricing>) -> Result<Self, String> {
        if entries.is_empty() {
            return Err("pricing catalog has no entries".to_string());
        }

        let mut normalized = BTreeMap::new();
        for (model_id, pricing) in entries {
            let model_id = model_id.trim().to_lowercase();
            if model_id.is_empty() {
                return Err("pricing catalog contains an empty model id".to_string());
            }
            validate_pricing(&model_id, pricing)?;
            if normalized.insert(model_id.clone(), pricing).is_some() {
                return Err(format!(
                    "pricing catalog contains duplicate model id {model_id}"
                ));
            }
        }
        Ok(Self {
            entries: normalized,
        })
    }

    fn pricing_for(&self, model: &str) -> Option<&ModelPricing> {
        let model = model.to_lowercase();
        if let Some(pricing) = self.entries.get(&model) {
            return Some(pricing);
        }

        self.entries
            .iter()
            .filter(|(id, _)| contains_at_token_boundary(&model, id))
            .max_by_key(|(id, _)| id.len())
            .map(|(_, pricing)| pricing)
            .or_else(|| self.family_fallback(&model))
    }

    /// The median family rate avoids both zero-cost estimates for new model
    /// versions and legacy/mini outliers at either end of the price range.
    fn family_fallback(&self, model: &str) -> Option<&ModelPricing> {
        let token = FAMILY_FALLBACK_TOKENS
            .iter()
            .find(|token| contains_at_token_boundary(model, token))?;
        let mut family: Vec<(&String, &ModelPricing)> = self
            .entries
            .iter()
            .filter(|(id, _)| contains_at_token_boundary(id, token))
            .collect();
        family.sort_by(|(id_a, a), (id_b, b)| {
            a.input
                .total_cmp(&b.input)
                .then(a.output.total_cmp(&b.output))
                .then(id_a.cmp(id_b))
        });
        family.get(family.len() / 2).map(|(_, pricing)| *pricing)
    }
}

fn validate_pricing(model_id: &str, pricing: ModelPricing) -> Result<(), String> {
    for (field, value) in [
        ("input", pricing.input),
        ("output", pricing.output),
        ("cache_read", pricing.cache_read),
        ("cache_write", pricing.cache_write),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(format!(
                "pricing catalog entry {model_id} has invalid {field} rate"
            ));
        }
    }
    Ok(())
}

fn contains_at_token_boundary(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }

    (0..=haystack.len() - needle.len()).any(|start| {
        let end = start + needle.len();
        haystack[start..end] == *needle
            && (start == 0 || !haystack[start - 1].is_ascii_alphanumeric())
            && (end == haystack.len() || !haystack[end].is_ascii_alphanumeric())
    })
}

pub(crate) fn pricing_for(model: &str) -> Option<&'static ModelPricing> {
    static MEMO: OnceLock<Mutex<HashMap<String, Option<&'static ModelPricing>>>> = OnceLock::new();
    let memo = MEMO.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = memo.lock()
        && let Some(pricing) = cache.get(model)
    {
        return *pricing;
    }

    let pricing = catalog().pricing_for(model);
    if let Ok(mut cache) = memo.lock() {
        cache.insert(model.to_string(), pricing);
    }
    pricing
}

fn catalog() -> &'static PricingCatalog {
    static CATALOG: OnceLock<PricingCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        PricingCatalog::from_snapshot_json(EMBEDDED_SNAPSHOT)
            .expect("embedded models.dev pricing snapshot must be valid")
    })
}

#[cfg(test)]
fn trim_catalog(api_json: &str) -> Result<BTreeMap<String, ModelPricing>, String> {
    let providers: serde_json::Value =
        serde_json::from_str(api_json).map_err(|error| error.to_string())?;
    let providers = providers
        .as_object()
        .ok_or_else(|| "models.dev catalog root must be an object".to_string())?;
    let mut entries = BTreeMap::new();

    for provider_name in PROVIDER_ALLOWLIST {
        let Some(provider) = providers.get(provider_name) else {
            continue;
        };
        let models = provider
            .get("models")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| format!("provider {provider_name} must contain a models object"))?;
        for (model_id, model) in models {
            let Some(cost) = model.get("cost") else {
                continue;
            };
            let pricing: ModelPricing = serde_json::from_value(cost.clone()).map_err(|error| {
                format!("invalid pricing for {provider_name}/{model_id}: {error}")
            })?;
            let model_id = model_id.to_lowercase();
            validate_pricing(&model_id, pricing)?;
            if let Some(existing) = entries.insert(model_id.clone(), pricing)
                && existing != pricing
            {
                return Err(format!("conflicting pricing for model id {model_id}"));
            }
        }
    }

    if entries.is_empty() {
        return Err("no priced models found in models.dev catalog".to_string());
    }
    Ok(entries)
}

#[cfg(test)]
fn render_snapshot(entries: &BTreeMap<String, ModelPricing>) -> Result<String, String> {
    let mut json = serde_json::to_string_pretty(entries).map_err(|error| error.to_string())?;
    json.push('\n');
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_snapshot_covers_current_agent_models() {
        let catalog = catalog();
        for model in [
            "claude-fable-5",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
            "gpt-5.6-sol",
        ] {
            assert!(
                catalog.pricing_for(model).is_some(),
                "snapshot must price {model}"
            );
        }
    }

    #[test]
    fn lookup_prefers_exact_and_longest_boundary_matches() {
        let catalog = catalog();
        assert_eq!(
            catalog.pricing_for("Claude-Fable-5"),
            catalog.pricing_for("claude-fable-5")
        );
        assert_eq!(
            catalog.pricing_for("us.anthropic.claude-fable-5-20260607"),
            catalog.pricing_for("claude-fable-5")
        );
        assert_ne!(
            catalog.pricing_for("gpt-5.6-sol"),
            catalog.pricing_for("gpt-5")
        );
    }

    #[test]
    fn lookup_uses_family_fallback_without_matching_partial_tokens() {
        let catalog = catalog();
        assert!(catalog.pricing_for("claude-3-5-sonnet-20241022").is_some());
        assert!(catalog.pricing_for("claude-opus-4-9").is_some());
        assert_eq!(catalog.pricing_for("somegpt-5"), None);
        assert_eq!(catalog.pricing_for("totally-unknown-model"), None);
    }

    #[test]
    fn trim_catalog_validates_and_normalizes_pricing_schema() {
        let input = serde_json::json!({
            "anthropic": {
                "models": {
                    "Claude-Test-1": {
                        "cost": {
                            "input": 1.0,
                            "output": 2.0,
                            "cache_read": 0.1,
                            "cache_write": 1.25
                        }
                    },
                    "claude-no-cost": {}
                }
            },
            "some-reseller": {
                "models": {
                    "claude-test-1": {
                        "cost": {"input": 99.0, "output": 99.0}
                    }
                }
            }
        });

        let entries = trim_catalog(&input.to_string()).unwrap();
        assert_eq!(entries.keys().collect::<Vec<_>>(), vec!["claude-test-1"]);
        assert_eq!(entries["claude-test-1"].cache_read, 0.1);
        assert_eq!(entries["claude-test-1"].cache_write, 1.25);

        let partial = serde_json::json!({
            "openai": {
                "models": {
                    "gpt-partial": {"cost": {"input": 1.0}}
                }
            }
        });
        assert!(trim_catalog(&partial.to_string()).is_err());

        let negative = serde_json::json!({
            "openai": {
                "models": {
                    "gpt-negative": {"cost": {"input": -1.0, "output": 1.0}}
                }
            }
        });
        assert!(trim_catalog(&negative.to_string()).is_err());
    }

    #[test]
    fn snapshot_rendering_is_sorted_and_newline_terminated() {
        let entries = BTreeMap::from([
            (
                "z-model".to_string(),
                ModelPricing {
                    input: 2.0,
                    output: 4.0,
                    cache_read: 0.0,
                    cache_write: 0.0,
                },
            ),
            (
                "a-model".to_string(),
                ModelPricing {
                    input: 1.0,
                    output: 3.0,
                    cache_read: 0.1,
                    cache_write: 0.0,
                },
            ),
        ]);

        let rendered = render_snapshot(&entries).unwrap();
        assert!(rendered.find("a-model").unwrap() < rendered.find("z-model").unwrap());
        assert!(rendered.ends_with('\n'));
        assert_eq!(
            PricingCatalog::from_snapshot_json(&rendered)
                .unwrap()
                .entries,
            entries
        );
    }

    #[test]
    #[ignore]
    fn regenerate_models_dev_pricing_snapshot() {
        let agent = crate::clients::http::build_agent(Some(30));
        let response = crate::clients::http::send(agent.get(MODELS_DEV_API_URL))
            .expect("fetching models.dev catalog must succeed");
        assert_eq!(response.status_code, 200, "models.dev must return HTTP 200");
        let entries = trim_catalog(response.as_str().expect("catalog must be UTF-8"))
            .expect("models.dev catalog must match the expected schema");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/metrics/models_dev_pricing_snapshot.json");
        std::fs::write(path, render_snapshot(&entries).unwrap())
            .expect("pricing snapshot must be writable");
    }
}
