use crate::cli::hook_input::{decode_hook_input_bytes, strip_utf8_bom};
use crate::operations::commands::checkpoint_agent::orchestrator::CheckpointAuthorizationDenial;
use std::io::Read;

pub(super) fn handle_checkpoint(args: &[String]) {
    let perf = std::env::var("GIT_AI_DEBUG_PERFORMANCE").is_ok_and(|v| !v.is_empty() && v != "0");
    let t0 = std::time::Instant::now();

    let mut hook_input = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--hook-input" => {
                if i + 1 < args.len() {
                    hook_input = Some(strip_utf8_bom(args[i + 1].clone()));
                    if hook_input.as_ref().unwrap() == "stdin" {
                        let mut stdin = std::io::stdin();
                        let mut buffer = Vec::new();
                        if let Err(e) = stdin.read_to_end(&mut buffer) {
                            eprintln!("Failed to read stdin for hook input: {}", e);
                            std::process::exit(0);
                        }
                        let buffer = match decode_hook_input_bytes(buffer) {
                            Ok(buffer) => buffer,
                            Err(e) => {
                                eprintln!("Failed to decode stdin for hook input: {}", e);
                                std::process::exit(0);
                            }
                        };
                        if buffer.trim().is_empty() {
                            eprintln!("No hook input provided (via --hook-input or stdin).");
                            std::process::exit(0);
                        }
                        hook_input = Some(strip_utf8_bom(buffer));
                    } else if hook_input.as_ref().unwrap().trim().is_empty() {
                        eprintln!("Error: --hook-input requires a value");
                        std::process::exit(0);
                    }
                    i += 2;
                } else {
                    eprintln!("Error: --hook-input requires a value or 'stdin' to read from stdin");
                    std::process::exit(0);
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    if perf {
        eprintln!(
            "[perf] checkpoint: arg_parse={:.1}ms",
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }

    let (preset_name, file_args): (&str, &[String]) = if args.is_empty() {
        ("human", &[])
    } else if args[0] == "--" {
        ("human", &args[1..])
    } else if crate::operations::commands::checkpoint_agent::presets::resolve_preset(
        args[0].as_str(),
    )
    .is_err()
    {
        eprintln!("Usage: git-ai checkpoint <preset> [--hook-input <json|stdin>] [files...]");
        std::process::exit(0);
    } else {
        (args[0].as_str(), &args[1..])
    };

    let effective_hook_input = hook_input.unwrap_or_else(|| {
        synthesize_hook_input_from_cli_args(preset_name, file_args).unwrap_or_else(|denial| {
            eprintln!("{}", denial.user_message());
            std::process::exit(0);
        })
    });

    if perf {
        eprintln!(
            "[perf] checkpoint: synth_hook_input={:.1}ms",
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }

    let t_orchestrator = std::time::Instant::now();
    let requests =
        match crate::operations::commands::checkpoint_agent::orchestrator::execute_preset_checkpoint(
            preset_name,
            &effective_hook_input,
        ) {
            Ok(
                crate::operations::commands::checkpoint_agent::orchestrator::
                    CheckpointPresetOutcome::Authorized(requests),
            ) => requests,
            Ok(
                crate::operations::commands::checkpoint_agent::orchestrator::
                    CheckpointPresetOutcome::Denied(denial),
            ) => {
                eprintln!("{}", denial.user_message());
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("{} preset error: {}", preset_name, e);
                std::process::exit(0);
            }
        };

    if perf {
        eprintln!(
            "[perf] checkpoint: orchestrator={:.1}ms (requests={}, files={})",
            t_orchestrator.elapsed().as_secs_f64() * 1000.0,
            requests.len(),
            requests.iter().map(|r| r.files.len()).sum::<usize>(),
        );
    }

    if requests.is_empty() {
        std::process::exit(0);
    }

    for request in &requests {
        for file in &request.files {
            if !file.path.is_absolute() {
                eprintln!("Error: file path must be absolute: {}", file.path.display());
                std::process::exit(0);
            }
        }
    }

    let t_daemon_config = std::time::Instant::now();
    let daemon_config = crate::operations::daemon::DaemonConfig::from_env_or_default_paths()
        .map_err(|e| e.to_string());

    let config = match daemon_config {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Background worker unavailable: {}", e);
            std::process::exit(0);
        }
    };

    if perf {
        eprintln!(
            "[perf] checkpoint: daemon_config={:.1}ms",
            t_daemon_config.elapsed().as_secs_f64() * 1000.0
        );
    }

    let deliveries = crate::model::checkpoint_delivery::CheckpointDelivery::from_requests(requests);
    if let Some(error) = deliveries
        .iter()
        .find_map(|delivery| delivery.validate().err())
    {
        eprintln!("Checkpoint delivery unavailable: {}", error);
        std::process::exit(0);
    }
    let t_delivery = std::time::Instant::now();
    let report =
        crate::operations::commands::checkpoint_agent::delivery_runtime::
            deliver_authorized_checkpoint_batch(&config, &deliveries);
    if perf {
        eprintln!(
            "[perf] checkpoint: delivery={:.1}ms",
            t_delivery.elapsed().as_secs_f64() * 1000.0,
        );
    }
    if report.published > 0 {
        eprintln!(
            "Background worker unavailable; saved checkpoint for delivery when it is available."
        );
    }
    if !report.publication_failures.is_empty() {
        eprintln!(
            "Checkpoint could not be delivered or saved; run `git-ai debug` for background service diagnostics."
        );
    }

    if std::env::var_os("GIT_AI_TEST_DB_PATH").is_some() {
        println!(
            "checkpoint_requests={}",
            report.acknowledged.saturating_add(report.published)
        );
    }

    if perf {
        eprintln!(
            "[perf] checkpoint: total={:.1}ms",
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
}

pub(super) fn synthesize_hook_input_from_cli_args(
    preset_name: &str,
    remaining_args: &[String],
) -> Result<String, CheckpointAuthorizationDenial> {
    Ok(match preset_name {
        "human" | "mock_ai" | "mock_known_human" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let mut paths: Vec<String> = remaining_args
                .iter()
                .filter(|a| !a.starts_with("--"))
                .map(|s| {
                    let p = std::path::Path::new(s.as_str());
                    if p.is_absolute() {
                        s.clone()
                    } else {
                        cwd.join(p).to_string_lossy().to_string()
                    }
                })
                .collect();
            if paths.is_empty() {
                crate::operations::commands::checkpoint_agent::orchestrator::
                    authorize_checkpoint_status_discovery(&cwd)?;
                paths = discover_dirty_files_from_status(&cwd);
            }
            serde_json::json!({
                "file_paths": paths,
                "cwd": cwd.to_string_lossy(),
            })
            .to_string()
        }
        "known_human" => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let mut editor = "unknown".to_string();
            let mut editor_version = "unknown".to_string();
            let mut extension_version = "unknown".to_string();
            let mut files: Vec<String> = Vec::new();
            let mut i = 0usize;
            while i < remaining_args.len() {
                match remaining_args[i].as_str() {
                    "--editor" if i + 1 < remaining_args.len() => {
                        editor = remaining_args[i + 1].clone();
                        i += 2;
                    }
                    "--editor-version" if i + 1 < remaining_args.len() => {
                        editor_version = remaining_args[i + 1].clone();
                        i += 2;
                    }
                    "--extension-version" if i + 1 < remaining_args.len() => {
                        extension_version = remaining_args[i + 1].clone();
                        i += 2;
                    }
                    "--" => {
                        files.extend(remaining_args[i + 1..].iter().map(|s| {
                            let p = std::path::Path::new(s.as_str());
                            if p.is_absolute() {
                                s.clone()
                            } else {
                                cwd.join(p).to_string_lossy().to_string()
                            }
                        }));
                        break;
                    }
                    arg if !arg.starts_with("--") => {
                        let p = std::path::Path::new(arg);
                        if p.is_absolute() {
                            files.push(arg.to_string());
                        } else {
                            files.push(cwd.join(p).to_string_lossy().to_string());
                        }
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
            serde_json::json!({
                "editor": editor,
                "editor_version": editor_version,
                "extension_version": extension_version,
                "cwd": cwd.to_string_lossy(),
                "edited_filepaths": files,
            })
            .to_string()
        }
        _ => String::new(),
    })
}

pub(super) fn discover_dirty_files_from_status(cwd: &std::path::Path) -> Vec<String> {
    let repo_root =
        crate::operations::git::repository::discover_repository_in_path_no_git_exec(cwd)
            .ok()
            .and_then(|r| r.workdir().ok())
            .unwrap_or_else(|| cwd.to_path_buf());

    let args = vec![
        "--no-optional-locks".to_string(),
        "-C".to_string(),
        cwd.to_string_lossy().to_string(),
        "status".to_string(),
        "--porcelain".to_string(),
        "-uall".to_string(),
    ];
    let output = crate::clients::git_cli::exec_git(&args).ok();
    let Some(output) = output else {
        return vec![];
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let raw_file = line[3..].trim();
            if raw_file.is_empty() {
                return None;
            }
            let unescaped = crate::operations::git::path_format::unescape_git_path(raw_file);
            let mut file = unescaped.as_str();
            // Renames show as "old_name -> new_name"; take only the new name
            if let Some(arrow_pos) = file.find(" -> ") {
                file = &file[arrow_pos + 4..];
            }
            let p = std::path::Path::new(file);
            if p.is_absolute() {
                Some(file.to_string())
            } else {
                Some(repo_root.join(p).to_string_lossy().to_string())
            }
        })
        .collect()
}
