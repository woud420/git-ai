use super::{config, package_manager, run_impl_with_url};

pub fn run_with_args(args: &[String]) {
    #[cfg(windows)]
    super::exit_if_invoked_via_git_extension();

    let mut force = false;
    let mut background = false;

    for arg in args {
        match arg.as_str() {
            "--force" => force = true,
            "--background" => background = true,
            _ => {
                eprintln!("Unknown argument: {}", arg);
                eprintln!("Usage: git-ai upgrade [--force]");
                std::process::exit(1);
            }
        }
    }

    if let Some(manager) = package_manager::current() {
        if !background {
            eprintln!("{}", manager.upgrade_instruction());
            std::process::exit(1);
        }
        return;
    }

    let config = config::Config::fresh();
    let channel = config.update_channel();
    let skip_install = background && config.auto_updates_disabled();
    let _ = run_impl_with_url(force, config.api_base_url(), channel, skip_install);
}
