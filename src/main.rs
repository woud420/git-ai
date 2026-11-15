use git_ai::cli::environment::{
    SuperuserCheckResult, check_superuser_guard, print_superuser_warning,
};
use git_ai::{cli, operations};
use std::ffi::OsString;

/// `git-ai` subcommands take UTF-8 arguments. Only the git proxy forwards
/// arbitrary bytes.
fn utf8_args_or_exit(args: Vec<OsString>) -> Vec<String> {
    args.into_iter()
        .map(|arg| {
            arg.into_string().unwrap_or_else(|_| {
                eprintln!("error: invalid UTF-8 was detected in one or more arguments");
                std::process::exit(2);
            })
        })
        .collect()
}

fn is_superuser_exempt_command(args: &[String]) -> bool {
    let first = match args.first() {
        Some(a) => a.as_str(),
        None => return true,
    };
    matches!(
        first,
        "help"
            | "--help"
            | "-h"
            | "version"
            | "--version"
            | "-v"
            | "upgrade"
            | "debug"
            | "uninstall-hooks"
    ) || (first == "bg" || first == "d" || first == "daemon")
        && args
            .get(1)
            .is_some_and(|s| s == "run" || s == "status" || s == "shutdown")
}

fn main() {
    git_ai::observability::wltrace::initialize_from_env();

    // Get the binary name that was called; the rest is passed on untouched.
    let mut argv = std::env::args_os();
    let binary_name = argv
        .next()
        .and_then(|arg| arg.into_string().ok())
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or("git-ai".to_string());

    if operations::commands::git_hook_handlers::is_git_hook_binary_name(&binary_name) {
        eprintln!(
            "git-ai: the git core hooks feature has been sunset.\n\
             To remove the deprecated git-ai hook symlinks from this repository, run:\n\
             \n\
             \x20 git-ai git-hooks remove\n"
        );
        std::process::exit(0);
    }

    let args: Vec<OsString> = argv.collect();

    #[cfg(debug_assertions)]
    {
        if std::env::var("GIT_AI").as_deref() == Ok("git") {
            cli::git_handlers::handle_git(&args);
            return;
        }
    }

    if binary_name == "git-ai" || binary_name == "git-ai.exe" {
        let args = utf8_args_or_exit(args);
        // Block elevated privileges to prevent creating root-owned files
        // that break normal-user daemon startup. Only applies to direct
        // `git-ai` commands (not the git proxy, which must stay transparent).
        // Exempt commands that must work regardless (upgrade, daemon run, help, etc.).
        if !is_superuser_exempt_command(&args) {
            match check_superuser_guard() {
                SuperuserCheckResult::WarnFutureBlock => print_superuser_warning(),
                SuperuserCheckResult::AllowedWithWarning => {
                    eprintln!(
                        "[git-ai] warning: running as superuser (GIT_AI_ALLOW_SUPERUSER is set)"
                    );
                }
                SuperuserCheckResult::Allowed => {}
            }
        }
        cli::git_ai_handlers::handle_git_ai(&args);
        std::process::exit(0);
    }

    cli::git_handlers::handle_git(&args);
}
