mod config_toml;
mod hooks_json;
#[cfg(test)]
mod hooks_json_tests;
#[cfg(test)]
mod install_tests;
mod installer;
#[cfg(test)]
mod tests;

const CODEX_CHECKPOINT_CMD: &str = "checkpoint codex --hook-input stdin";
const CODEX_HOOK_EVENTS: [&str; 3] = ["PreToolUse", "PostToolUse", "Stop"];

pub struct CodexInstaller;
