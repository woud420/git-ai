//! Binary dispatch — `argv[0]` decides git-proxy vs git-ai mode; this wiring is load-bearing.

mod fail;
pub mod git_ai_handlers;
pub mod git_handlers;
mod hook_input;
mod machine_json;
