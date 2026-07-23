//! Wire helpers for git-ai's internal machine-JSON commands (e.g.
//! `effective-ignore-patterns`, `blame-analysis`, `fetch-authorship-notes`):
//! parsing the single `--json <payload>` argument shape and emitting
//! single-line JSON success/error output. These commands' output shapes are
//! a public contract — see `docs/contracts/cli-output.md`.

use crate::cli::hook_input::strip_utf8_bom;

pub(crate) fn parse_machine_json_arg(args: &[String], command: &str) -> Result<String, String> {
    if args.len() != 2 || args[0] != "--json" {
        return Err(format!("Usage: git-ai {} --json '<json-payload>'", command));
    }

    let payload = strip_utf8_bom(args[1].clone());
    if payload.trim().is_empty() {
        return Err("JSON payload cannot be empty".to_string());
    }

    Ok(payload)
}

pub(crate) fn emit_machine_json_error(message: impl AsRef<str>) -> ! {
    let payload = serde_json::json!({ "error": message.as_ref() });
    if let Ok(json) = serde_json::to_string(&payload) {
        eprintln!("{}", json);
    } else {
        eprintln!(r#"{{"error":"failed to serialize error payload"}}"#);
    }
    std::process::exit(1);
}

pub(crate) fn print_machine_json(value: &serde_json::Value) {
    match serde_json::to_string(value) {
        Ok(json) => println!("{}", json),
        Err(e) => emit_machine_json_error(format!("Failed to serialize JSON output: {}", e)),
    }
}
