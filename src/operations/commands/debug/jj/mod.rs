use crate::cli::print_machine_json;
use serde_json::json;

mod args;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod output;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod read;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[path = "unsupported.rs"]
mod read;

struct Error {
    code: &'static str,
    message: String,
}

impl Error {
    fn new(code: &'static str, message: impl std::fmt::Display) -> Self {
        Self {
            code,
            message: message.to_string().chars().take(1024).collect(),
        }
    }
}

pub(super) fn handle(input: &[String]) -> i32 {
    if args::is_help(input) {
        println!(
            "Experimental native jj observation diagnostics; attribution is disabled.\n\
             git-ai debug jj status --journal PATH --json\n\
             git-ai debug jj receipt --journal PATH --source ID --admission ID --json\n\
             Requires an existing current-schema journal; does not initialize or capture history."
        );
        return 0;
    }
    let result = args::parse(input).and_then(|request| match request {
        args::Request::Status { journal } => read::status(journal),
        args::Request::Receipt {
            journal,
            source,
            admission,
        } => read::receipt(journal, source, admission),
    });
    match result {
        Ok(value) => {
            print_machine_json(&value);
            0
        }
        Err(error) => {
            print_machine_json(&json!({
                "schema_version": 1, "backend": "jj", "attribution_enabled": false,
                "error": {"code": error.code, "message": error.message}
            }));
            1
        }
    }
}
