use super::{ChannelRelease, ReleasesResponse};
use crate::clients::api::client::ApiContext;
use crate::config::UpdateChannel;
use crate::operations::git::repository::resolve_api_author_identity;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub(super) fn releases_endpoint() -> &'static str {
    "/worker/releases"
}

pub(super) fn verify_sha256(content: &[u8], expected_hash: &str) -> Result<(), String> {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let actual_hash = format!("{:x}", hasher.finalize());

    if actual_hash.eq_ignore_ascii_case(expected_hash) {
        Ok(())
    } else {
        Err(format!(
            "Checksum mismatch: expected {}, got {}",
            expected_hash, actual_hash
        ))
    }
}

/// Parse SHA256SUMS file content into a map of filename → hash.
/// Format: `<hash>  <filename>` (two spaces between hash and filename)
pub(super) fn parse_checksums(content: &str) -> HashMap<String, String> {
    let mut checksums = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: "<hash>  <filename>" (two spaces)
        if let Some((hash, filename)) = line.split_once("  ") {
            checksums.insert(filename.to_string(), hash.to_string());
        }
    }

    checksums
}

/// Fetch SHA256SUMS from the releases API and verify against expected checksum.
pub(super) fn fetch_and_verify_checksums(
    api_base_url: &str,
    channel: &str,
    expected_checksum: &str,
) -> Result<HashMap<String, String>, String> {
    let endpoint = format!("/worker/releases/{}/download/SHA256SUMS", channel);

    let (_agent, request) =
        ApiContext::http_get(&format!("{}{}", api_base_url, endpoint), Some(30));
    let response = crate::clients::http::send(request)
        .map_err(|e| format!("Failed to fetch SHA256SUMS: {}", e))?;

    if response.status_code != 200 {
        return Err(format!(
            "Failed to fetch SHA256SUMS: HTTP {}",
            response.status_code
        ));
    }

    let content = response.as_bytes();

    verify_sha256(content, expected_checksum)
        .map_err(|e| format!("SHA256SUMS verification failed: {}", e))?;

    let content_str = std::str::from_utf8(content)
        .map_err(|e| format!("SHA256SUMS is not valid UTF-8: {}", e))?;

    Ok(parse_checksums(content_str))
}

/// Fetch install script from the releases API and verify against checksums.
pub(super) fn fetch_and_verify_install_script(
    api_base_url: &str,
    channel: &str,
    checksums: &HashMap<String, String>,
) -> Result<String, String> {
    #[cfg(windows)]
    let script_name = "install.ps1";
    #[cfg(not(windows))]
    let script_name = "install.sh";

    let expected_checksum = checksums
        .get(script_name)
        .ok_or_else(|| format!("Checksum for {} not found in SHA256SUMS", script_name))?;

    let endpoint = format!("/worker/releases/{}/download/{}", channel, script_name);

    let (_agent, request) =
        ApiContext::http_get(&format!("{}{}", api_base_url, endpoint), Some(30));
    let response = crate::clients::http::send(request)
        .map_err(|e| format!("Failed to fetch {}: {}", script_name, e))?;

    if response.status_code != 200 {
        return Err(format!(
            "Failed to fetch {}: HTTP {}",
            script_name, response.status_code
        ));
    }

    let content = response.as_bytes();

    verify_sha256(content, expected_checksum)
        .map_err(|e| format!("{} verification failed: {}", script_name, e))?;

    let script = std::str::from_utf8(content)
        .map_err(|e| format!("{} is not valid UTF-8: {}", script_name, e))?;

    Ok(script.to_string())
}

pub(super) fn fetch_release_for_channel(
    api_base_url: &str,
    channel: UpdateChannel,
) -> Result<ChannelRelease, String> {
    #[cfg(test)]
    if let Some(result) = try_mock_releases(api_base_url, channel) {
        return result;
    }
    let url = Some(api_base_url.to_string());
    let context = ApiContext::new(url, resolve_api_author_identity).with_timeout(5);
    let response = context
        .get(releases_endpoint())
        .map_err(|e| format!("Failed to check for updates: {}", e))?;

    let body = response
        .as_str()
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    let releases: ReleasesResponse = serde_json::from_str(body)
        .map_err(|e| format!("Failed to parse release response: {}", e))?;

    release_from_response(releases, channel)
}

pub(super) fn release_from_response(
    releases: ReleasesResponse,
    channel: UpdateChannel,
) -> Result<ChannelRelease, String> {
    let channel_name = channel.as_str();

    let channel_info = releases
        .channels
        .get(channel_name)
        .ok_or_else(|| format!("Channel '{}' not found in releases", channel_name))?;

    let tag = channel_info.version.trim().to_string();
    if tag.is_empty() {
        return Err("Release tag not found in response".to_string());
    }

    let semver = semver_from_tag(&tag);
    if semver.is_empty() {
        return Err(format!("Unable to parse semver from tag '{}'", tag));
    }

    let checksum = channel_info.checksum.trim().to_string();
    if checksum.is_empty() {
        return Err("Checksum not found in response".to_string());
    }

    Ok(ChannelRelease {
        tag,
        semver,
        checksum,
    })
}

#[cfg(test)]
pub(super) fn try_mock_releases(
    base: &str,
    channel: UpdateChannel,
) -> Option<Result<ChannelRelease, String>> {
    let json = base.strip_prefix("mock://")?;
    Some(
        serde_json::from_str::<ReleasesResponse>(json)
            .map_err(|e| format!("Invalid mock releases payload: {}", e))
            .and_then(|releases| release_from_response(releases, channel)),
    )
}

pub(super) fn semver_from_tag(tag: &str) -> String {
    let trimmed = tag
        .trim()
        .trim_start_matches("enterprise-")
        .trim_start_matches('v');
    trimmed.split(['-', '+']).next().unwrap_or("").to_string()
}
