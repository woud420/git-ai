use super::{Agent, invalid_config};
use crate::error::GitAiError;
use serde_json::{Value as Json, json};
use std::path::Path;
use toml::Value as Toml;

pub(super) fn update(
    agent: Agent,
    before: &str,
    owned: Option<&str>,
    desired: Option<&str>,
) -> Result<(String, Option<String>), GitAiError> {
    match agent {
        Agent::Codex => codex(before, owned, desired),
        Agent::Claude => claude(before, owned, desired),
    }
}

fn codex(
    before: &str,
    owned: Option<&str>,
    desired: Option<&str>,
) -> Result<(String, Option<String>), GitAiError> {
    let error = |message: &str| invalid_config(Path::new("config.toml"), message);
    let mut config: Toml = toml::from_str(before).map_err(|e| error(&format!("{e}")))?;
    let original = config.clone();
    let proxy = config
        .get_mut("features")
        .and_then(|v| v.get_mut("network_proxy"));
    let Some(proxy) = proxy else {
        if desired.is_some() {
            return Err(error(
                "enable the Codex network proxy manually before allowing the trace socket",
            ));
        }
        return Ok((before.into(), None));
    };
    if desired.is_some() {
        if proxy.as_bool() == Some(true) {
            *proxy = Toml::Table(toml::map::Map::from_iter([(
                "enabled".into(),
                Toml::Boolean(true),
            )]));
        }
        if proxy.get("enabled").and_then(Toml::as_bool) != Some(true) {
            return Err(error(
                "enable the Codex network proxy manually before allowing the trace socket",
            ));
        }
    }
    let Some(proxy) = proxy.as_table_mut() else {
        return Ok((before.into(), None));
    };
    if desired.is_some() {
        proxy
            .entry("unix_sockets")
            .or_insert_with(|| Toml::Table(Default::default()));
    }
    let Some(sockets) = proxy.get_mut("unix_sockets") else {
        return Ok((before.into(), None));
    };
    let sockets = sockets
        .as_table_mut()
        .ok_or_else(|| error("network proxy unix_sockets must be a table"))?;
    if let Some(owned) = owned
        && Some(owned) != desired
        && sockets.get(owned).and_then(Toml::as_str) == Some("allow")
    {
        sockets.remove(owned);
    }
    let mut next_owned = None;
    if let Some(desired) = desired {
        match sockets.get(desired).and_then(Toml::as_str) {
            Some("allow") => {
                if owned == Some(desired) {
                    next_owned = Some(desired.into());
                }
            }
            Some("deny") => {
                return Err(error(
                    "the active trace socket is explicitly denied; change the permission manually",
                ));
            }
            _ if sockets.contains_key(desired) => {
                return Err(error("socket permission must be allow or deny"));
            }
            _ => {
                sockets.insert(desired.into(), Toml::String("allow".into()));
                next_owned = Some(desired.into());
            }
        }
    }
    let after = if config == original {
        before.into()
    } else {
        toml::to_string_pretty(&config).map_err(|e| error(&format!("{e}")))?
    };
    Ok((after, next_owned))
}

fn claude(
    before: &str,
    owned: Option<&str>,
    desired: Option<&str>,
) -> Result<(String, Option<String>), GitAiError> {
    let error = |message: &str| invalid_config(Path::new("settings.json"), message);
    let mut config: Json = serde_json::from_str(before)?;
    let original = config.clone();
    let sandbox = config.get_mut("sandbox");
    let Some(sandbox) = sandbox else {
        if desired.is_some() {
            return Err(error(
                "enable the Claude sandbox manually before allowing the trace socket",
            ));
        }
        return Ok((before.into(), None));
    };
    if desired.is_some() && sandbox.get("enabled").and_then(Json::as_bool) != Some(true) {
        return Err(error(
            "enable the Claude sandbox manually before allowing the trace socket",
        ));
    }
    let sandbox = sandbox
        .as_object_mut()
        .ok_or_else(|| error("sandbox must be an object"))?;
    if desired.is_some() {
        sandbox.entry("network").or_insert_with(|| json!({}));
    }
    let Some(network) = sandbox.get_mut("network") else {
        return Ok((before.into(), None));
    };
    let network = network
        .as_object_mut()
        .ok_or_else(|| error("sandbox.network must be an object"))?;
    if desired.is_some() {
        network
            .entry("allowUnixSockets")
            .or_insert_with(|| json!([]));
    }
    let Some(sockets) = network.get_mut("allowUnixSockets") else {
        return Ok((before.into(), None));
    };
    let sockets = sockets
        .as_array_mut()
        .ok_or_else(|| error("allowUnixSockets must be an array of paths"))?;
    if sockets.iter().any(|v| !v.is_string()) {
        return Err(error("allowUnixSockets must be an array of paths"));
    }
    if let Some(owned) = owned
        && Some(owned) != desired
    {
        sockets.retain(|v| v.as_str() != Some(owned));
    }
    let mut next_owned = None;
    if let Some(desired) = desired {
        if !sockets.iter().any(|v| v.as_str() == Some(desired)) {
            sockets.push(json!(desired));
            next_owned = Some(desired.into());
        } else if owned == Some(desired) {
            next_owned = Some(desired.into());
        }
    }
    let after = if config == original {
        before.into()
    } else {
        serde_json::to_string_pretty(&config)?
    };
    Ok((after, next_owned))
}
