use std::collections::HashMap;

use crate::error::GitAiError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct InstallOptions {
    pub(super) dry_run: bool,
    pub(super) verbose: bool,
    pub(super) install_skills: bool,
    pub(super) include_visual_studio_extension: bool,
    pub(super) api_base: Option<String>,
    pub(super) api_key: Option<String>,
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
            value if value.starts_with("--api-base=") => {
                options.api_base = non_empty_value(&value[11..]);
            }
            "--api-base" => {
                let value = args.next().ok_or_else(|| {
                    GitAiError::Generic("missing value for --api-base".to_string())
                })?;
                options.api_base = non_empty_value(value);
            }
            value if value.starts_with("--api-key=") => {
                options.api_key = non_empty_value(&value[10..]);
            }
            "--api-key" => {
                let value = args.next().ok_or_else(|| {
                    GitAiError::Generic("missing value for --api-key".to_string())
                })?;
                options.api_key = non_empty_value(value);
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
    println!(
        "  --visual-studio-extension    Include Visual Studio detection and status checks on Windows"
    );
    println!("                               This does not install a VSIX package");
    println!("  --api-base <url>              Save an API base URL in git-ai configuration");
    println!("  --api-key <key>               Save an API key in git-ai configuration");
    println!("  --help, -h                    Show this help message");
}

fn non_empty_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
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
}
