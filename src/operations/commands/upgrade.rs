mod models;
use crate::clients::api::client::ApiContext;
use crate::config::{self, UpdateChannel};
use crate::observability::log_message;
use crate::operations::git::repository::resolve_api_author_identity;
pub use models::DaemonUpdateCheckResult;
use models::{ChannelRelease, ReleasesResponse, UpdateCache, UpgradeAction};
#[cfg(windows)]
use models::{ProcessEntry32W, WINDOWS_MAX_PATH, WindowsHandle};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
const TH32CS_SNAPPROCESS: u32 = 0x00000002;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: WindowsHandle = (-1isize) as WindowsHandle;

#[cfg(windows)]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> WindowsHandle;
    fn Process32FirstW(snapshot: WindowsHandle, entry: *mut ProcessEntry32W) -> i32;
    fn Process32NextW(snapshot: WindowsHandle, entry: *mut ProcessEntry32W) -> i32;
    fn CloseHandle(handle: WindowsHandle) -> i32;
}

const UPDATE_CHECK_INTERVAL_HOURS: u64 = 24;
const GIT_AI_RELEASE_ENV: &str = "GIT_AI_RELEASE_TAG";
#[cfg(windows)]
const GIT_AI_RESTART_DAEMON_AFTER_INSTALL_ENV: &str = "GIT_AI_RESTART_DAEMON_AFTER_INSTALL";
const GIT_AI_DAEMON_UPGRADE_ENV: &str = "GIT_AI_DAEMON_UPGRADE";
const BACKGROUND_SPAWN_THROTTLE_SECS: u64 = 60;
const ENV_BACKGROUND_UPGRADE_WORKER: &str = "GIT_AI_BACKGROUND_UPGRADE_WORKER";

static UPDATE_NOTICE_EMITTED: AtomicBool = AtomicBool::new(false);
static LAST_BACKGROUND_SPAWN: AtomicU64 = AtomicU64::new(0);

impl UpgradeAction {
    fn to_string(&self) -> &str {
        match self {
            UpgradeAction::UpgradeAvailable => "upgrade_available",
            UpgradeAction::AlreadyLatest => "already_latest",
            UpgradeAction::RunningNewerVersion => "running_newer_version",
            UpgradeAction::ForceReinstall => "force_reinstall",
        }
    }
}

impl UpdateCache {
    fn new(channel: UpdateChannel) -> Self {
        Self {
            last_checked_at: 0,
            available_tag: None,
            available_semver: None,
            channel: channel.as_str().to_string(),
        }
    }

    fn update_available(&self) -> bool {
        self.available_semver.is_some()
    }

    fn matches_channel(&self, channel: UpdateChannel) -> bool {
        self.channel == channel.as_str()
    }
}

fn get_update_check_cache_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Ok(test_cache_dir) = std::env::var("GIT_AI_TEST_CACHE_DIR") {
            return Some(PathBuf::from(test_cache_dir).join("update_check"));
        }
    }

    crate::config::update_check_path()
}

fn read_update_cache() -> Option<UpdateCache> {
    let path = get_update_check_cache_path()?;
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_update_cache(cache: &UpdateCache) {
    if let Some(path) = get_update_check_cache_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec(cache) {
            let _ = fs::write(path, json);
        }
    }
}

fn current_timestamp() -> u64 {
    crate::model::clock::now_secs()
}

#[cfg(windows)]
fn exit_if_invoked_via_git_extension() {
    if should_block_git_extension_upgrade(
        parent_process_name().as_deref(),
        std::env::var(ENV_BACKGROUND_UPGRADE_WORKER).as_deref() == Ok("1"),
    ) {
        eprintln!(
            "error: `git ai upgrade` is not supported on Windows. Run `git-ai upgrade` instead."
        );
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn should_block_git_extension_upgrade(
    parent_process_name: Option<&str>,
    is_background_worker: bool,
) -> bool {
    !is_background_worker && parent_process_name.is_some_and(is_git_process_name)
}

#[cfg(windows)]
fn is_git_process_name(name: &str) -> bool {
    std::path::Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| {
            file_name.eq_ignore_ascii_case("git") || file_name.eq_ignore_ascii_case("git.exe")
        })
}

#[cfg(windows)]
fn parent_process_name() -> Option<String> {
    struct SnapshotGuard(WindowsHandle);

    impl Drop for SnapshotGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let _snapshot_guard = SnapshotGuard(snapshot);

    let current_pid = std::process::id();
    let parent_pid = find_parent_pid(snapshot, current_pid)?;
    process_name_for_pid(snapshot, parent_pid)
}

#[cfg(windows)]
fn find_parent_pid(snapshot: WindowsHandle, current_pid: u32) -> Option<u32> {
    let mut entry = windows_process_entry_template();
    if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
        return None;
    }

    loop {
        if entry.th32_process_id == current_pid {
            return Some(entry.th32_parent_process_id);
        }
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            return None;
        }
    }
}

#[cfg(windows)]
fn process_name_for_pid(snapshot: WindowsHandle, pid: u32) -> Option<String> {
    let mut entry = windows_process_entry_template();
    if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
        return None;
    }

    loop {
        if entry.th32_process_id == pid {
            let len = entry
                .sz_exe_file
                .iter()
                .position(|&ch| ch == 0)
                .unwrap_or(entry.sz_exe_file.len());
            return Some(String::from_utf16_lossy(&entry.sz_exe_file[..len]));
        }
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            return None;
        }
    }
}

#[cfg(windows)]
fn windows_process_entry_template() -> ProcessEntry32W {
    ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; WINDOWS_MAX_PATH],
    }
}

fn should_check_for_updates(channel: UpdateChannel, cache: Option<&UpdateCache>) -> bool {
    let now = current_timestamp();
    match cache {
        Some(cache) if cache.last_checked_at > 0 => {
            // If cache doesn't match the channel, we should check for updates
            if !cache.matches_channel(channel) {
                return true;
            }
            let elapsed = now.saturating_sub(cache.last_checked_at);
            elapsed > UPDATE_CHECK_INTERVAL_HOURS * 3600
        }
        _ => true,
    }
}

fn semver_from_tag(tag: &str) -> String {
    let trimmed = tag
        .trim()
        .trim_start_matches("enterprise-")
        .trim_start_matches('v');
    trimmed.split(['-', '+']).next().unwrap_or("").to_string()
}

fn determine_action(force: bool, release: &ChannelRelease, current_version: &str) -> UpgradeAction {
    if force {
        return UpgradeAction::ForceReinstall;
    }

    if release.semver == current_version {
        UpgradeAction::AlreadyLatest
    } else if is_newer_version(&release.semver, current_version) {
        UpgradeAction::UpgradeAvailable
    } else {
        UpgradeAction::RunningNewerVersion
    }
}

fn persist_update_state(channel: UpdateChannel, release: Option<&ChannelRelease>) {
    let mut cache = UpdateCache::new(channel);
    cache.last_checked_at = current_timestamp();
    if let Some(release) = release {
        cache.available_tag = Some(release.tag.clone());
        cache.available_semver = Some(release.semver.clone());
    }
    write_update_cache(&cache);
}

pub(crate) fn clear_cached_update_state() {
    let channel = config::Config::fresh().update_channel();
    persist_update_state(channel, None);
}

fn releases_endpoint() -> &'static str {
    "/worker/releases"
}

fn verify_sha256(content: &[u8], expected_hash: &str) -> Result<(), String> {
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
fn parse_checksums(content: &str) -> HashMap<String, String> {
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
fn fetch_and_verify_checksums(
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
fn fetch_and_verify_install_script(
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

fn fetch_release_for_channel(
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

fn release_from_response(
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
fn try_mock_releases(base: &str, channel: UpdateChannel) -> Option<Result<ChannelRelease, String>> {
    let json = base.strip_prefix("mock://")?;
    Some(
        serde_json::from_str::<ReleasesResponse>(json)
            .map_err(|e| format!("Invalid mock releases payload: {}", e))
            .and_then(|releases| release_from_response(releases, channel)),
    )
}

fn run_install_script(script_content: &str, tag: &str, silent: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        if let Ok(daemon_config) =
            crate::operations::daemon::DaemonConfig::from_env_or_default_paths()
        {
            // Best effort: stop the daemon before we hand off to the detached installer.
            // The install script also has a fallback kill path so old released binaries
            // can still recover, but stopping here makes upgrades complete sooner.
            let _ = crate::operations::commands::daemon::stop_daemon(
                &daemon_config,
                Duration::from_secs(10),
            );
        }

        // On Windows, we need to run the installer detached because the current git-ai
        // binary and shims are in use and need to be replaced. The installer will wait
        // for the files to be released before proceeding.
        let pid = std::process::id();
        let log_dir = dirs::home_dir()
            .ok_or_else(|| "Could not determine home directory".to_string())?
            .join(".git-ai")
            .join("upgrade-logs");

        // Ensure the log directory exists
        fs::create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log directory: {}", e))?;

        let log_file = log_dir.join(format!("upgrade-{}.log", pid));
        let log_path_str = log_file.to_string_lossy().to_string();

        // Write the install script to a temp file
        let script_path = log_dir.join(format!("install-{}.ps1", pid));
        fs::write(&script_path, script_content)
            .map_err(|e| format!("Failed to write install script: {}", e))?;
        let script_path_str = script_path.to_string_lossy().to_string();

        // Create log file with initial message
        fs::write(&log_file, format!("Starting upgrade at PID {}\n", pid))
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        // PowerShell wrapper that executes the script file with logging
        let ps_wrapper = format!(
            "$logFile = '{}'; \
             Start-Transcript -Path $logFile -Append -Force | Out-Null; \
             Write-Host 'Running verified install script...'; \
             try {{ \
                  $ErrorActionPreference = 'Continue'; \
                  & '{}'; \
                  Write-Host 'Install script completed'; \
              }} catch {{ \
                  Write-Host \"Error: $_\"; \
                  Write-Host \"Stack trace: $($_.ScriptStackTrace)\"; \
              }} finally {{ \
                  if ($env:{} -eq '1') {{ \
                      $daemonExe = Join-Path $HOME '.git-ai\\bin\\git-ai.exe'; \
                      if (Test-Path $daemonExe) {{ try {{ & $daemonExe bg start *> $null }} catch {{ }} }} \
                  }}; \
                  Stop-Transcript | Out-Null; \
                  Remove-Item -Path '{}' -Force -ErrorAction SilentlyContinue; \
              }}",
            log_path_str, script_path_str, GIT_AI_RESTART_DAEMON_AFTER_INSTALL_ENV, script_path_str
        );

        let spawn_powershell = |exe: &str| -> std::io::Result<std::process::Child> {
            let mut cmd = Command::new(exe);
            cmd.arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-Command")
                .arg(&ps_wrapper)
                .env(GIT_AI_RELEASE_ENV, tag);

            // Hide the spawned console to prevent any host/UI bleed-through
            cmd.creation_flags(crate::process_spawn::CREATE_NO_WINDOW);

            if silent {
                cmd.env(GIT_AI_RESTART_DAEMON_AFTER_INSTALL_ENV, "1");
                cmd.env(GIT_AI_DAEMON_UPGRADE_ENV, "1");
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }

            cmd.spawn()
        };

        let spawn_result = spawn_powershell("pwsh").or_else(|_| spawn_powershell("powershell"));

        match spawn_result {
            Ok(_) => {
                if !silent {
                    println!(
                        "\x1b[1;33mNote: The installation is running in the background on Windows.\x1b[0m"
                    );
                    println!(
                        "This allows the current git-ai process to exit and release file locks."
                    );
                    println!("Check the log file for progress: {}", log_path_str);
                    println!(
                        "The installer will stop lingering git-ai background processes if needed, but active git commands can still delay completion."
                    );
                }
                Ok(())
            }
            Err(e) => Err(format!("Failed to run installation script: {}", e)),
        }
    }

    #[cfg(not(windows))]
    {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Write script to ~/.git-ai/tmp/ to avoid /tmp noexec or permission issues.
        // Fall back to the system temp dir if the home-based path is unavailable.
        let temp_dir = crate::config::git_ai_dir_path()
            .map(|p| p.join("tmp"))
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp directory: {}", e))?;
        let script_path = temp_dir.join(format!("git-ai-install-{}.sh", std::process::id()));

        // Write and make executable
        let mut file = fs::File::create(&script_path)
            .map_err(|e| format!("Failed to create temp script file: {}", e))?;
        file.write_all(script_content.as_bytes())
            .map_err(|e| format!("Failed to write install script: {}", e))?;
        drop(file);

        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to make script executable: {}", e))?;

        let script_path_str = script_path.to_string_lossy().to_string();

        let mut cmd = Command::new("bash");
        cmd.arg(&script_path_str).env(GIT_AI_RELEASE_ENV, tag);

        if silent {
            cmd.env(GIT_AI_DAEMON_UPGRADE_ENV, "1");
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }

        let result = match cmd.status() {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "Installation script failed with exit code: {:?}",
                        status.code()
                    ))
                }
            }
            Err(e) => Err(format!("Failed to run installation script: {}", e)),
        };

        // Clean up temp script
        let _ = fs::remove_file(&script_path);

        result
    }
}

pub fn run_with_args(args: &[String]) {
    #[cfg(windows)]
    exit_if_invoked_via_git_extension();

    let mut force = false;
    let mut background = false;

    for arg in args {
        match arg.as_str() {
            "--force" => force = true,
            "--background" => background = true, // Undocumented flag for internal use when spawning background process
            _ => {
                eprintln!("Unknown argument: {}", arg);
                eprintln!("Usage: git-ai upgrade [--force]");
                std::process::exit(1);
            }
        }
    }

    run_impl(force, background);
}

fn run_impl(force: bool, background: bool) {
    let config = config::Config::fresh();
    let channel = config.update_channel();
    let skip_install = background && config.auto_updates_disabled();
    let _ = run_impl_with_url(force, config.api_base_url(), channel, skip_install);
}

fn run_impl_with_url(
    force: bool,
    api_base_url: &str,
    channel: UpdateChannel,
    skip_install: bool,
) -> UpgradeAction {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("Checking for updates (channel: {})...", channel.as_str());

    let release = match fetch_release_for_channel(api_base_url, channel) {
        Ok(release) => release,
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    };

    println!("Current version: v{}", current_version);
    println!(
        "Available {} version: v{} (tag {})",
        channel.as_str(),
        release.semver,
        release.tag
    );
    println!();

    let action = determine_action(force, &release, current_version);
    let cache_release = matches!(action, UpgradeAction::UpgradeAvailable);
    persist_update_state(channel, cache_release.then_some(&release));

    log_message(
        "checked_for_update",
        "info",
        Some(serde_json::json!({
            "current_version": current_version,
            "api_base_url": api_base_url,
            "channel": channel.as_str(),
            "result": action.to_string()
        })),
    );

    match action {
        UpgradeAction::AlreadyLatest => {
            println!("You are already on the latest version!");
            println!();
            println!("To reinstall anyway, run:");
            println!("  \x1b[1;36mgit-ai upgrade --force\x1b[0m");
            return action;
        }
        UpgradeAction::RunningNewerVersion => {
            println!("You are running a newer version than the selected release channel.");
            println!("(This usually means you're running a development build)");
            println!();
            println!("To reinstall the selected release anyway, run:");
            println!("  \x1b[1;36mgit-ai upgrade --force\x1b[0m");
            return action;
        }
        UpgradeAction::ForceReinstall => {
            println!(
                "\x1b[1;33mForce mode enabled - reinstalling {}\x1b[0m",
                release.tag
            );
        }
        UpgradeAction::UpgradeAvailable => {
            println!("\x1b[1;33mA new version is available!\x1b[0m");
        }
    }
    println!();

    if skip_install {
        return action;
    }

    println!("Fetching and verifying release artifacts...");

    // Fetch and verify SHA256SUMS against the release's master checksum
    let checksums =
        match fetch_and_verify_checksums(api_base_url, channel.as_str(), &release.checksum) {
            Ok(checksums) => {
                println!("\x1b[1;32m✓\x1b[0m SHA256SUMS verified");
                checksums
            }
            Err(err) => {
                eprintln!("Failed to fetch/verify checksums: {}", err);
                std::process::exit(1);
            }
        };

    // Fetch and verify the install script
    let script_content =
        match fetch_and_verify_install_script(api_base_url, channel.as_str(), &checksums) {
            Ok(content) => {
                #[cfg(windows)]
                println!("\x1b[1;32m✓\x1b[0m install.ps1 verified");
                #[cfg(not(windows))]
                println!("\x1b[1;32m✓\x1b[0m install.sh verified");
                content
            }
            Err(err) => {
                eprintln!("Failed to fetch/verify install script: {}", err);
                std::process::exit(1);
            }
        };

    println!();
    println!("Running installation script...");
    println!();

    match run_install_script(&script_content, &release.tag, false) {
        Ok(()) => {
            // On Windows, we spawn the installer in the background and can't verify success
            #[cfg(not(windows))]
            {
                println!("\x1b[1;32m✓\x1b[0m Successfully installed {}!", release.tag);
            }

            log_message(
                "upgraded",
                "info",
                Some(serde_json::json!({
                    "release_tag": release.tag,
                    "current_version": current_version,
                    "api_base_url": api_base_url,
                    "channel": channel.as_str()
                })),
            );
        }
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    }

    action
}

fn print_cached_notice(cache: &UpdateCache) {
    if cache.available_semver.is_none() || cache.available_tag.is_none() {
        return;
    }

    if !std::io::stdout().is_terminal() {
        // Don't print the version check notice if stdout is not a terminal/interactive shell
        return;
    }

    if UPDATE_NOTICE_EMITTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let available_version = cache.available_semver.as_deref().unwrap_or("");

    eprintln!();
    eprintln!(
        "\x1b[1;33mA new version of git-ai is available: \x1b[1;32mv{}\x1b[0m → \x1b[1;32mv{}\x1b[0m",
        current_version, available_version
    );
    eprintln!(
        "\x1b[1;33mRun \x1b[1;36mgit-ai upgrade\x1b[0m \x1b[1;33mto upgrade to the latest version.\x1b[0m"
    );
    eprintln!();
}

pub fn maybe_schedule_background_update_check() {
    let config = config::Config::get();
    if config.version_checks_disabled() {
        return;
    }

    let channel = config.update_channel();
    let cache = read_update_cache();

    if config.auto_updates_disabled()
        && let Some(cache) = cache.as_ref()
        && cache.matches_channel(channel)
        && cache.update_available()
    {
        print_cached_notice(cache);
    }

    if !should_check_for_updates(channel, cache.as_ref()) {
        return;
    }

    let now = current_timestamp();
    let last_spawn = LAST_BACKGROUND_SPAWN.load(Ordering::SeqCst);
    if now.saturating_sub(last_spawn) < BACKGROUND_SPAWN_THROTTLE_SECS {
        return;
    }

    if spawn_background_upgrade_process() {
        LAST_BACKGROUND_SPAWN.store(now, Ordering::SeqCst);
    }
}

fn spawn_background_upgrade_process() -> bool {
    crate::cli::git_ai_exe::spawn_internal_git_ai_subcommand(
        "upgrade",
        &["--background"],
        ENV_BACKGROUND_UPGRADE_WORKER,
        &[],
    )
}

/// Install a previously-detected update.
///
/// Designed for use by the daemon process **after** a clean shutdown.  Reads
/// the on-disk update cache (written earlier by `check_for_update_available`)
/// to decide whether an update is pending, bypassing the 24-hour time guard.
/// Uses `Config::fresh()` (not the `OnceLock` singleton) so the daemon
/// respects runtime config changes (e.g. disabling auto-updates).
///
/// Returns `Ok(UpdateReady)` if the install script ran, `Ok(NoUpdate)` if
/// no pending update was found or updates are disabled.
pub fn check_and_install_update_if_available() -> Result<DaemonUpdateCheckResult, String> {
    let config = config::Config::fresh();
    if config.version_checks_disabled() || config.auto_updates_disabled() {
        return Ok(DaemonUpdateCheckResult::NoUpdate);
    }

    let channel = config.update_channel();
    let api_base_url = config.api_base_url();

    // Read the cache that check_for_update_available() populated earlier.
    // We intentionally skip should_check_for_updates() here because the
    // hourly check loop already confirmed an update is available and
    // persisted that fact — re-checking the 24h guard would always say
    // "too soon" and the install would never run.
    let cache = read_update_cache();
    let has_pending_update = cache
        .as_ref()
        .is_some_and(|c| c.matches_channel(channel) && c.update_available());

    if !has_pending_update {
        return Ok(DaemonUpdateCheckResult::NoUpdate);
    }

    // Re-fetch the release to get the tag needed for the installer.
    let release = fetch_release_for_channel(api_base_url, channel)?;
    let current_version = env!("CARGO_PKG_VERSION");
    let action = determine_action(false, &release, current_version);

    if action != UpgradeAction::UpgradeAvailable {
        // Cache was stale or version changed between check and install.
        persist_update_state(channel, None);
        return Ok(DaemonUpdateCheckResult::NoUpdate);
    }

    log_message(
        "daemon_installing_update",
        "info",
        Some(serde_json::json!({
            "current_version": current_version,
            "release_tag": release.tag,
            "api_base_url": api_base_url,
            "channel": channel.as_str()
        })),
    );

    // Fetch, verify, and run the install script silently.
    let checksums = fetch_and_verify_checksums(api_base_url, channel.as_str(), &release.checksum)?;
    let script_content =
        fetch_and_verify_install_script(api_base_url, channel.as_str(), &checksums)?;
    run_install_script(&script_content, &release.tag, true)?;

    // Clear the cached update now that we've installed it.
    persist_update_state(channel, None);

    log_message(
        "daemon_upgraded",
        "info",
        Some(serde_json::json!({
            "release_tag": release.tag,
            "current_version": current_version,
            "api_base_url": api_base_url,
            "channel": channel.as_str()
        })),
    );

    Ok(DaemonUpdateCheckResult::UpdateReady)
}

/// Check whether a newer version is available without installing it.
///
/// Like `check_and_install_update_if_available` but only queries the releases API
/// and updates the local cache. Returns `DaemonUpdateCheckResult::UpdateReady` when
/// the channel has a newer version than the running binary.
pub fn check_for_update_available() -> Result<DaemonUpdateCheckResult, String> {
    let config = config::Config::fresh();
    if config.version_checks_disabled() {
        return Ok(DaemonUpdateCheckResult::NoUpdate);
    }

    let channel = config.update_channel();
    let api_base_url = config.api_base_url();
    let cache = read_update_cache();

    if !should_check_for_updates(channel, cache.as_ref()) {
        // Even if it's not time to re-check, an earlier check may have found an update.
        if let Some(ref c) = cache
            && c.matches_channel(channel)
            && c.update_available()
            && !config.auto_updates_disabled()
        {
            return Ok(DaemonUpdateCheckResult::UpdateReady);
        }
        return Ok(DaemonUpdateCheckResult::NoUpdate);
    }

    let release = fetch_release_for_channel(api_base_url, channel)?;
    let current_version = env!("CARGO_PKG_VERSION");
    let action = determine_action(false, &release, current_version);
    let cache_release = matches!(action, UpgradeAction::UpgradeAvailable);
    persist_update_state(channel, cache_release.then_some(&release));

    log_message(
        "checked_for_update",
        "info",
        Some(serde_json::json!({
            "current_version": current_version,
            "api_base_url": api_base_url,
            "channel": channel.as_str(),
            "result": action.to_string()
        })),
    );

    if action == UpgradeAction::UpgradeAvailable && !config.auto_updates_disabled() {
        Ok(DaemonUpdateCheckResult::UpdateReady)
    } else {
        Ok(DaemonUpdateCheckResult::NoUpdate)
    }
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse_version =
        |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse::<u32>().ok()).collect() };

    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);

    for i in 0..latest_parts.len().max(current_parts.len()) {
        let latest_part = latest_parts.get(i).copied().unwrap_or(0);
        let current_part = current_parts.get(i).copied().unwrap_or(0);

        if latest_part > current_part {
            return true;
        } else if latest_part < current_part {
            return false;
        }
    }

    false
}

#[cfg(test)]
mod tests;
