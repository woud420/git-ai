use crate::operations::workspace_context::{ContextError, discover};
use std::path::Path;

pub(super) fn handle(args: &[String]) -> i32 {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!(
            "git-ai debug context --json - Discover Git/jj workspace paths without enabling attribution"
        );
        return 0;
    }
    let result = if args.len() == 1 && args[0] == "--json" {
        std::env::current_dir()
            .map_err(|error| ContextError::invalid(Path::new("."), error))
            .and_then(|cwd| discover(&cwd))
            .and_then(|context| {
                serde_json::to_string_pretty(&context).map_err(|error| ContextError {
                    code: "unsupported_path_encoding",
                    message: format!("Workspace paths cannot be represented in JSON: {error}"),
                })
            })
    } else {
        Err(ContextError {
            code: "usage",
            message: "Usage: git-ai debug context --json".to_string(),
        })
    };
    match result {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(error) => {
            println!("{}", serde_json::json!({ "error": error }));
            1
        }
    }
}
