use crate::cli::print_machine_json;
use crate::operations::jj::admission::NativeAdmissionExpectation;
use serde_json::json;

mod args;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod shared;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod write;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use read as write;
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
            "Experimental native jj observation commands; attribution is disabled.\n\
             git-ai debug jj status --journal PATH --json\n\
             git-ai debug jj receipt --journal PATH --source ID --admission ID --json\n\
             git-ai debug jj initialize --journal PATH --json\n\
             git-ai debug jj capture --journal PATH --json --expect-source ID --expect-initialization-receipt ID --expect-baseline ID --expect-generation N --expect-head OPERATION_ID [--expect-head OPERATION_ID ...]\n\
             Status and receipt require an existing current-schema journal and do not write observations.\n\
             Initialize and capture can create or migrate the journal before full repository policy checks.\n\
             Initialize retains the original cutoff on retry; capture requires an explicit expected cursor.\n\
             Capture accepts 1-32 distinct nonroot heads and canonical generation 0 through i64::MAX-1.\n\
             Initialization failure may leave an unavailable source seal. No automatic recovery or observer."
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
        args::Request::Initialize { journal } => write::initialize(journal),
        args::Request::Capture {
            journal,
            source,
            initialization_receipt,
            baseline,
            generation,
            heads,
        } => write::capture(
            journal,
            NativeAdmissionExpectation {
                source_id: source,
                initialization_receipt_id: initialization_receipt,
                baseline_id: baseline,
                generation,
                admitted_head_ids: &heads,
            },
        ),
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
