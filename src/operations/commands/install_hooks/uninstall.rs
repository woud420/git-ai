use super::*;

pub(super) async fn async_run_uninstall(
    params: &HookInstallerParams,
    dry_run: bool,
    verbose: bool,
) -> Result<HashMap<String, InstallStatus>, GitAiError> {
    let mut any_checked = false;
    let mut has_changes = false;
    let mut statuses: HashMap<String, InstallStatus> = HashMap::new();

    // Uninstall skills first (these are global, not per-agent, silently)
    if let Ok(result) = skills_installer::uninstall_skills(dry_run, verbose) {
        if result.changed {
            has_changes = true;
            statuses.insert("skills".to_string(), InstallStatus::Installed);
        } else {
            statuses.insert("skills".to_string(), InstallStatus::AlreadyInstalled);
        }
    }

    // === Coding Agents ===
    println!("\n\x1b[1mCoding Agents\x1b[0m");

    let installers = get_all_installers();

    for installer in installers {
        let name = installer.name();
        let id = installer.id();

        // Check if tool is installed
        match installer.check_hooks(params) {
            Ok(check_result) => {
                let owned_socket = crate::operations::mdm::sandbox_socket::has_ownership(id);
                if !check_result.tool_installed && !owned_socket {
                    statuses.insert(id.to_string(), InstallStatus::NotFound);
                    continue;
                }
                // `hooks_installed` only recognises the current hook layout, so
                // it cannot rule out hooks left behind by an older one.
                let hooks_detected = check_result.hooks_installed || owned_socket;

                // Uninstall hooks
                let spinner = Spinner::new(&format!("{}: removing hooks", name));
                spinner.start();

                match installer.uninstall_hooks(params, dry_run) {
                    Ok(None) if !hooks_detected => {
                        spinner.finish_and_clear();
                        statuses.insert(id.to_string(), InstallStatus::NotFound);
                        continue;
                    }
                    Ok(Some(diff)) => {
                        if dry_run {
                            spinner.pending(&format!("{}: Pending removal", name));
                        } else {
                            spinner.success(&format!("{}: Hooks removed", name));
                        }
                        if verbose {
                            println!();
                            print_diff(&diff);
                        }
                        has_changes = true;
                        statuses.insert(id.to_string(), InstallStatus::Installed);
                    }
                    Ok(None) => {
                        spinner.success(&format!("{}: No hooks to remove", name));
                        statuses.insert(id.to_string(), InstallStatus::AlreadyInstalled);
                    }
                    Err(e) => {
                        spinner.error(&format!("{}: Failed to remove hooks", name));
                        eprintln!("  Error: {}", e);
                        statuses.insert(id.to_string(), InstallStatus::Failed);
                    }
                }
                any_checked = true;

                // Uninstall extras
                match installer.uninstall_extras(params, dry_run) {
                    Ok(results) => {
                        for result in results {
                            if result.changed {
                                has_changes = true;
                            }
                            if !result.message.is_empty() {
                                let extra_spinner = Spinner::new(&result.message);
                                extra_spinner.start();
                                if result.changed {
                                    extra_spinner.success(&result.message);
                                } else {
                                    extra_spinner.pending(&result.message);
                                }
                            }
                            if verbose && let Some(diff) = result.diff {
                                println!();
                                print_diff(&diff);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  Error uninstalling extras for {}: {}", name, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("  Error checking {}: {}", name, e);
                statuses.insert(id.to_string(), InstallStatus::Failed);
            }
        }
    }

    if !any_checked {
        println!("No git-ai hooks found to uninstall.");
    } else if has_changes && dry_run {
        println!("\n\x1b[33m⚠ Dry-run mode (default). No changes were made.\x1b[0m");
        println!("To apply these changes, run:");
        println!("\x1b[1m  git-ai uninstall-hooks --dry-run=false\x1b[0m");
    } else if !has_changes {
        println!("All git-ai hooks have been removed.");
    }

    Ok(statuses)
}

#[cfg(all(test, not(windows)))]
mod tests {
    use crate::operations::mdm::test_env::with_temp_home;
    use serial_test::serial;
    use std::fs;

    /// `check_hooks` reports `hooks_installed` for the current hook layout
    /// only; hooks written by an older layout must still be removed.
    #[test]
    #[serial]
    fn uninstall_removes_hooks_from_layouts_check_hooks_does_not_recognise() {
        const OLD_LAYOUTS: [(&str, &str, &str); 2] = [
            // Current Windsurf layout hooks five events; this has one.
            (
                "windsurf",
                ".codeium/windsurf/hooks.json",
                r#"{"hooks":{"pre_write_code":[{"command":"git-ai checkpoint windsurf --hook-input stdin"}]}}"#,
            ),
            // Current Firebender layout has no `matcher`.
            (
                "firebender",
                ".firebender/hooks.json",
                r#"{"hooks":{"preToolUse":[{"matcher":"Write","command":"git-ai checkpoint firebender --hook-input stdin"}]}}"#,
            ),
        ];

        with_temp_home(|home| {
            for (_, path, content) in OLD_LAYOUTS {
                let path = home.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, content).unwrap();
            }

            let statuses = super::super::run_uninstall(&["--dry-run".to_string()]).unwrap();

            for (id, ..) in OLD_LAYOUTS {
                assert_eq!(
                    statuses.get(id).map(String::as_str),
                    Some("installed"),
                    "{id}: old-layout hooks were not offered for removal"
                );
            }
            // Installed tools with no git-ai hooks at all stay silent.
            fs::create_dir_all(home.join(".factory")).unwrap();
            let statuses = super::super::run_uninstall(&["--dry-run".to_string()]).unwrap();
            assert_eq!(statuses.get("droid").map(String::as_str), Some("not_found"));
        });
    }
}
