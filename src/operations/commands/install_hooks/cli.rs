use std::collections::HashMap;

use crate::error::GitAiError;
use super::installer_environment::InstallerEnvironment;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct InstallOptions {
    pub(super) dry_run: bool,
    pub(super) verbose: bool,
    pub(super) install_skills: bool,
    pub(super) include_visual_studio_extension: bool,
    pub(super) api_base: Option<String>,
    pub(super) api_key: Option<String>,
    pub(super) installer_environment: InstallerEnvironment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InstallAction {
    Help,
    Install(InstallOptions),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstallCommandOutcome {
    Help,
    Installed(HashMap<String, String>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct UninstallOptions {
    pub(super) dry_run: bool,
    pub(super) verbose: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UninstallAction {
    Help,
    Uninstall(UninstallOptions),
}

pub(crate) enum UninstallCommandOutcome {
    Help,
    Uninstalled(HashMap<String, String>),
}

pub(super) fn parse_uninstall_action(args: &[String]) -> Result<UninstallAction, GitAiError> {
    let mut options = UninstallOptions::default();
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => return Ok(UninstallAction::Help),
            "--dry-run" | "--dry-run=true" => options.dry_run = true,
            "--dry-run=false" => options.dry_run = false,
            "--verbose" | "-v" => options.verbose = true,
            unknown => {
                return Err(GitAiError::Generic(format!(
                    "unknown uninstall-hooks option '{unknown}'; run 'git-ai uninstall-hooks --help' for usage"
                )));
            }
        }
    }
    Ok(UninstallAction::Uninstall(options))
}

pub(crate) fn print_uninstall_help() {
    println!("Usage: git-ai uninstall-hooks [options]");
    println!();
    println!("Remove managed hooks and skills across all supported agent/editor integrations.");
    println!("Without --dry-run, removal is applied immediately.");
    println!();
    println!("Options:");
    println!("  --dry-run[=true|false]       Preview changes, or explicitly apply them with false");
    println!("  --verbose, -v                Show configuration diffs");
    println!("  --help, -h                    Show this help message");
    println!();
    println!(
        "Use 'git-ai uninstall --help' for full daemon, Git configuration, and binary removal."
    );
}

pub(super) fn parse_install_action(args: &[String]) -> Result<InstallAction, GitAiError> {
    let mut options = InstallOptions::default();

    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(InstallAction::Help),
            "--dry-run" | "--dry-run=true" => options.dry_run = true,
            "--dry-run=false" => options.dry_run = false,
            "--verbose" | "-v" => options.verbose = true,
            "--skills" => options.install_skills = true,
            "--visual-studio-extension" => options.include_visual_studio_extension = true,
            value if value.starts_with("--installer-env=") => {
                options.installer_environment.insert(&value[16..])?;
            }
            "--installer-env" => {
                let value = args.next().ok_or_else(|| {
                    GitAiError::Generic("missing value for --installer-env".to_string())
                })?;
                options.installer_environment.insert(value)?;
            }
            value if value.starts_with("--api-base=") => {
                options.api_base = Some(required_value("--api-base", &value[11..])?);
            }
            "--api-base" => {
                options.api_base = Some(next_value("--api-base", &mut args)?);
            }
            value if value.starts_with("--api-key=") => {
                options.api_key = Some(required_value("--api-key", &value[10..])?);
            }
            "--api-key" => {
                options.api_key = Some(next_value("--api-key", &mut args)?);
            }
            unknown => {
                return Err(GitAiError::Generic(format!(
                    "unknown install option '{unknown}'; run 'git-ai install --help' for usage"
                )));
            }
        }
    }

    Ok(InstallAction::Install(options))
}

pub(crate) fn print_install_help(command: &str) {
    let alias = if command == "install" {
        "install-hooks"
    } else {
        "install"
    };
    println!("Usage: git-ai {command} [options]");
    println!("Alias: git-ai {alias}");
    println!();
    println!("Install git-ai hooks and configure supported agent and editor integrations.");
    println!();
    println!("Options:");
    println!("  --dry-run[=true|false]       Preview changes, or explicitly apply them with false");
    println!("  --verbose, -v                Show configuration diffs");
    println!("  --skills                     Also install agent skill files");
    println!("  --installer-env NAME=ABSOLUTE_PATH");
    println!("                               Package-only user path handoff (repeatable)");
    println!(
        "  --visual-studio-extension    Include Visual Studio detection and status checks on Windows"
    );
    println!("                               This does not install a VSIX package");
    println!("  --api-base <url>              Save an API base URL in git-ai configuration");
    println!("  --api-key <key>               Save an API key in git-ai configuration");
    println!("                               Use --api-key=<key> for a key starting with '-'");
    println!("  --help, -h                    Show this help message");
}

fn next_value(option: &str, args: &mut std::slice::Iter<'_, String>) -> Result<String, GitAiError> {
    let value = args
        .next()
        .map(String::as_str)
        .filter(|value| !value.trim_start().starts_with('-'))
        .unwrap_or_default();
    required_value(option, value)
}

fn required_value(option: &str, value: &str) -> Result<String, GitAiError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(GitAiError::Generic(format!("missing value for {option}")));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::commands::install_hooks::{
        VISUAL_STUDIO_INSTALLER_ID, should_include_installer,
    };

    fn parsed_install_options(args: &[String]) -> InstallOptions {
        let InstallAction::Install(options) = parse_install_action(args).unwrap() else {
            panic!("install arguments unexpectedly requested help");
        };
        options
    }

    #[test]
    fn defaults_visual_studio_extension_to_disabled() {
        let options = parsed_install_options(&[]);
        assert!(!options.include_visual_studio_extension);
        assert!(!should_include_installer(
            VISUAL_STUDIO_INSTALLER_ID,
            &options
        ));
        assert!(should_include_installer("vscode", &options));
    }

    #[test]
    fn accepts_supported_flags_and_aliases() {
        let args = vec![
            "--dry-run".to_string(),
            "--visual-studio-extension".to_string(),
            "--skills".to_string(),
            "-v".to_string(),
        ];
        let options = parsed_install_options(&args);
        assert!(options.dry_run);
        assert!(options.verbose);
        assert!(options.install_skills);
        assert!(options.include_visual_studio_extension);
        assert!(should_include_installer(
            VISUAL_STUDIO_INSTALLER_ID,
            &options
        ));
    }

    #[test]
    fn accepts_explicit_dry_run_false() {
        let options = parsed_install_options(&["--dry-run=false".to_string()]);
        assert!(!options.dry_run);
    }

    #[test]
    fn accepts_package_api_configuration() {
        let args = vec![
            "--api-base=https://enterprise.example".to_string(),
            "--api-key".to_string(),
            "sk-enterprise-key".to_string(),
        ];
        let options = parsed_install_options(&args);
        assert_eq!(
            options.api_base.as_deref(),
            Some("https://enterprise.example")
        );
        assert_eq!(options.api_key.as_deref(), Some("sk-enterprise-key"));
    }

    #[test]
    fn rejects_missing_package_api_value() {
        let args = vec!["--api-base".to_string()];
        let err = parse_install_action(&args).unwrap_err();
        assert!(err.to_string().contains("missing value for --api-base"));
    }

    #[test]
    fn eng_389_rejects_empty_or_option_shaped_api_values() {
        for option in ["--api-base", "--api-key"] {
            for value in ["", "  ", "--help", "-h", "--dry-run", "--skils", " --help"] {
                let args = [option.to_string(), value.to_string()];
                assert!(
                    parse_install_action(&args).is_err(),
                    "{option} accepted an empty value or consumed another option"
                );
            }
            for value in ["", "  "] {
                assert!(parse_install_action(&[format!("{option}={value}")]).is_err());
            }
        }
    }

    #[test]
    fn eng_389_explicit_values_do_not_hide_following_safety_flags() {
        let options = parsed_install_options(&[
            "--api-base".to_string(),
            "https://api.example".to_string(),
            "--api-key=--literal-key".to_string(),
            "--dry-run".to_string(),
        ]);
        assert!(options.dry_run);
        assert_eq!(options.api_base.as_deref(), Some("https://api.example"));
        assert_eq!(options.api_key.as_deref(), Some("--literal-key"));
        assert_eq!(
            parse_install_action(&["--api-key=test-key".to_string(), "--help".to_string()])
                .unwrap(),
            InstallAction::Help
        );
    }

    #[test]
    fn recognizes_help_as_an_action() {
        for help in ["--help", "-h"] {
            let args = vec![help.to_string()];
            assert!(matches!(
                parse_install_action(&args).unwrap(),
                InstallAction::Help
            ));
        }
    }

    #[test]
    fn rejects_unknown_option() {
        let args = vec!["--skils".to_string()];
        let err = parse_install_action(&args).unwrap_err();
        assert!(err.to_string().contains("unknown install option '--skils'"));
    }

    #[test]
    fn eng_408_uninstall_parsing_separates_help_preview_and_apply() {
        for flag in ["--help", "-h"] {
            assert_eq!(
                parse_uninstall_action(&[flag.to_string()]).unwrap(),
                UninstallAction::Help
            );
        }
        for (args, dry_run, verbose) in [
            (vec![], false, false),
            (vec!["--dry-run", "-v"], true, true),
            (vec!["--dry-run=true", "--verbose"], true, true),
            (vec!["--dry-run", "--dry-run=false"], false, false),
            (vec!["--dry-run=false", "--dry-run"], true, false),
        ] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert_eq!(
                parse_uninstall_action(&args).unwrap(),
                UninstallAction::Uninstall(UninstallOptions { dry_run, verbose })
            );
        }
        for flag in ["--dryrun", "--dry-run=tru", "--skills", "unexpected"] {
            assert!(parse_uninstall_action(&[flag.to_string()]).is_err());
        }
    }
}
