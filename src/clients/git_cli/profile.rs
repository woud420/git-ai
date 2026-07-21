//! Argument-shaping helpers for internal `git` subprocesses: managed-hook
//! suppression and per-purpose diff/parse profiles. These transformations are
//! applied by the `exec_git*` / `spawn_git*` helpers in the parent module.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

// Keep a thread-local depth for low-overhead checks on the active thread and a process-global
// depth so internal git spawned from background threads inherits suppression state.
thread_local! {
    static INTERNAL_GIT_HOOKS_DISABLED_DEPTH: Cell<usize> = const { Cell::new(0) };
}
static INTERNAL_GIT_HOOKS_DISABLED_DEPTH_GLOBAL: AtomicUsize = AtomicUsize::new(0);

pub struct InternalGitHooksGuard;

impl Drop for InternalGitHooksGuard {
    fn drop(&mut self) {
        INTERNAL_GIT_HOOKS_DISABLED_DEPTH.with(|depth| {
            let current = depth.get();
            if current > 0 {
                depth.set(current - 1);
            }
        });
        INTERNAL_GIT_HOOKS_DISABLED_DEPTH_GLOBAL.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Disable managed git hooks for internal `git` subprocesses executed through `exec_git*`.
/// Use this guard around higher-level operations that already execute hook logic explicitly.
pub fn disable_internal_git_hooks() -> InternalGitHooksGuard {
    INTERNAL_GIT_HOOKS_DISABLED_DEPTH.with(|depth| depth.set(depth.get() + 1));
    INTERNAL_GIT_HOOKS_DISABLED_DEPTH_GLOBAL.fetch_add(1, Ordering::Relaxed);
    InternalGitHooksGuard
}

fn should_disable_internal_git_hooks() -> bool {
    INTERNAL_GIT_HOOKS_DISABLED_DEPTH.with(|depth| depth.get() > 0)
        || INTERNAL_GIT_HOOKS_DISABLED_DEPTH_GLOBAL.load(Ordering::Relaxed) > 0
}

#[cfg(windows)]
fn null_hooks_path() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_hooks_path() -> &'static str {
    "/dev/null"
}

#[doc(hidden)]
pub fn args_with_disabled_hooks_if_needed(args: &[String]) -> Vec<String> {
    if !should_disable_internal_git_hooks() {
        return args.to_vec();
    }

    // Respect explicit hook-path overrides if a caller already set one.
    let already_overrides_hooks = args
        .windows(2)
        .any(|pair| pair[0] == "-c" && pair[1].starts_with("core.hooksPath="))
        || args.iter().any(|arg| {
            arg.starts_with("-ccore.hooksPath=") || arg.starts_with("--config=core.hooksPath=")
        });

    if already_overrides_hooks {
        return args.to_vec();
    }

    let mut out = Vec::with_capacity(args.len() + 2);
    out.push("-c".to_string());
    out.push(format!("core.hooksPath={}", null_hooks_path()));
    out.extend(args.iter().cloned());
    out
}

fn first_git_subcommand_index(args: &[String]) -> Option<usize> {
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        if !arg.starts_with('-') {
            return Some(index);
        }

        let takes_value = matches!(
            arg.as_str(),
            "-C" | "-c"
                | "--git-dir"
                | "--work-tree"
                | "--namespace"
                | "--super-prefix"
                | "--config-env"
        );

        index += if takes_value { 2 } else { 1 };
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalGitProfile {
    General,
    PatchParse,
    NumstatParse,
    RawDiffParse,
}

fn strip_profile_conflicts(args: Vec<String>, profile: InternalGitProfile) -> Vec<String> {
    if profile == InternalGitProfile::General {
        return args;
    }

    let Some(command_index) = first_git_subcommand_index(&args) else {
        return args;
    };

    let should_drop = |arg: &str| -> bool {
        match profile {
            InternalGitProfile::General => false,
            InternalGitProfile::PatchParse => {
                arg == "--ext-diff"
                    || arg == "--textconv"
                    || arg == "--relative"
                    || arg.starts_with("--relative=")
                    || arg == "--color"
                    || arg.starts_with("--color=")
                    || arg == "--no-prefix"
                    || arg == "--src-prefix"
                    || arg == "--dst-prefix"
                    || arg.starts_with("--src-prefix=")
                    || arg.starts_with("--dst-prefix=")
                    || arg.starts_with("--diff-algorithm=")
                    || arg == "--no-indent-heuristic"
                    || arg.starts_with("--inter-hunk-context=")
            }
            InternalGitProfile::NumstatParse => {
                arg == "--ext-diff"
                    || arg == "--textconv"
                    || arg == "--relative"
                    || arg.starts_with("--relative=")
                    || arg == "--color"
                    || arg.starts_with("--color=")
                    || arg == "--find-renames"
                    || arg.starts_with("--find-renames=")
                    || arg == "--find-copies"
                    || arg.starts_with("--find-copies=")
                    || arg == "--find-copies-harder"
                    || arg == "-M"
                    || arg.starts_with("-M")
                    || arg == "-C"
                    || arg.starts_with("-C")
            }
            InternalGitProfile::RawDiffParse => {
                arg == "--ext-diff"
                    || arg == "--textconv"
                    || arg == "--relative"
                    || arg.starts_with("--relative=")
                    || arg == "--color"
                    || arg.starts_with("--color=")
            }
        }
    };

    let mut out = Vec::with_capacity(args.len());
    out.extend(args[..=command_index].iter().cloned());

    let mut index = command_index + 1;
    while index < args.len() {
        if args[index] == "--" {
            out.extend(args[index..].iter().cloned());
            return out;
        }

        let drop_current = should_drop(&args[index]);
        if !drop_current {
            out.push(args[index].clone());
            index += 1;
            continue;
        }

        // Handle split-arg forms we intentionally strip (e.g. --src-prefix X).
        if matches!(profile, InternalGitProfile::PatchParse)
            && (args[index] == "--src-prefix" || args[index] == "--dst-prefix")
        {
            index += 1;
            if index < args.len() && args[index] != "--" {
                index += 1;
            }
            continue;
        }

        index += 1;
    }

    out
}

fn profile_options(profile: InternalGitProfile) -> &'static [&'static str] {
    match profile {
        InternalGitProfile::General => &[],
        InternalGitProfile::PatchParse => &[
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--no-relative",
            "--no-color",
            "--diff-algorithm=default",
            "--indent-heuristic",
            "--inter-hunk-context=0",
        ],
        InternalGitProfile::NumstatParse => &[
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-relative",
            "--no-renames",
        ],
        InternalGitProfile::RawDiffParse => &[
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-relative",
        ],
    }
}

#[doc(hidden)]
pub fn args_with_internal_git_profile(args: &[String], profile: InternalGitProfile) -> Vec<String> {
    if profile == InternalGitProfile::General {
        return args.to_vec();
    }

    let args = strip_profile_conflicts(args.to_vec(), profile);
    let Some(command_index) = first_git_subcommand_index(&args) else {
        return args;
    };

    let options = profile_options(profile);
    if options.is_empty() {
        return args;
    }

    let mut out = Vec::with_capacity(args.len() + options.len());
    out.extend(args[..=command_index].iter().cloned());
    for option in options {
        if !args.iter().any(|arg| arg == option) {
            out.push((*option).to_string());
        }
    }
    out.extend(args[command_index + 1..].iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_internal_git_hooks_guard_applies_to_spawned_threads() {
        let args = vec!["status".to_string()];
        let _guard = disable_internal_git_hooks();

        let spawned_args = args.clone();
        let forwarded =
            std::thread::spawn(move || args_with_disabled_hooks_if_needed(&spawned_args))
                .join()
                .expect("thread should join");

        assert_eq!(forwarded[0], "-c");
        assert!(forwarded[1].starts_with("core.hooksPath="));
    }

    #[test]
    fn patch_profile_applies_canonical_machine_parse_flags() {
        let args = vec!["diff".to_string(), "HEAD^".to_string(), "HEAD".to_string()];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::PatchParse);

        assert!(rewritten.iter().any(|arg| arg == "--no-ext-diff"));
        assert!(rewritten.iter().any(|arg| arg == "--no-textconv"));
        assert!(rewritten.iter().any(|arg| arg == "--src-prefix=a/"));
        assert!(rewritten.iter().any(|arg| arg == "--dst-prefix=b/"));
        assert!(rewritten.iter().any(|arg| arg == "--no-relative"));
        assert!(rewritten.iter().any(|arg| arg == "--no-color"));
        assert!(
            rewritten
                .iter()
                .any(|arg| arg == "--diff-algorithm=default")
        );
        assert!(rewritten.iter().any(|arg| arg == "--indent-heuristic"));
        assert!(rewritten.iter().any(|arg| arg == "--inter-hunk-context=0"));
    }

    #[test]
    fn numstat_profile_disables_renames_and_external_renderers() {
        let args = vec![
            "diff".to_string(),
            "--numstat".to_string(),
            "HEAD^".to_string(),
            "HEAD".to_string(),
        ];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::NumstatParse);
        assert!(rewritten.iter().any(|arg| arg == "--no-ext-diff"));
        assert!(rewritten.iter().any(|arg| arg == "--no-textconv"));
        assert!(rewritten.iter().any(|arg| arg == "--no-color"));
        assert!(rewritten.iter().any(|arg| arg == "--no-relative"));
        assert!(rewritten.iter().any(|arg| arg == "--no-renames"));
    }

    #[test]
    fn numstat_profile_strips_short_rename_and_copy_flags() {
        let args = vec![
            "diff".to_string(),
            "--numstat".to_string(),
            "-M90%".to_string(),
            "-C".to_string(),
            "-C75%".to_string(),
            "HEAD^".to_string(),
            "HEAD".to_string(),
        ];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::NumstatParse);
        assert!(!rewritten.iter().any(|arg| arg == "-C"));
        assert!(!rewritten.iter().any(|arg| arg.starts_with("-M")));
        assert!(!rewritten.iter().any(|arg| arg.starts_with("-C")));
        assert!(rewritten.iter().any(|arg| arg == "--no-renames"));
    }

    #[test]
    fn general_profile_is_noop() {
        let args = vec!["status".to_string(), "--porcelain=v2".to_string()];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::General);
        assert_eq!(rewritten, args);
    }

    #[test]
    fn patch_profile_strips_conflicting_ext_diff_and_color_flags() {
        let args = vec![
            "diff".to_string(),
            "--ext-diff".to_string(),
            "--color=always".to_string(),
            "HEAD".to_string(),
        ];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::PatchParse);

        assert!(rewritten.iter().any(|arg| arg == "--no-ext-diff"));
        assert!(!rewritten.iter().any(|arg| arg == "--ext-diff"));
        assert!(!rewritten.iter().any(|arg| arg.starts_with("--color")));
        assert!(rewritten.iter().any(|arg| arg == "--no-color"));
    }

    #[test]
    fn patch_profile_strips_split_prefix_args() {
        let args = vec![
            "diff".to_string(),
            "--src-prefix".to_string(),
            "SRC/".to_string(),
            "--dst-prefix".to_string(),
            "DST/".to_string(),
            "HEAD^".to_string(),
            "HEAD".to_string(),
        ];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::PatchParse);

        assert!(!rewritten.iter().any(|arg| arg == "--src-prefix"));
        assert!(!rewritten.iter().any(|arg| arg == "--dst-prefix"));
        assert!(!rewritten.iter().any(|arg| arg == "SRC/"));
        assert!(!rewritten.iter().any(|arg| arg == "DST/"));
        assert!(rewritten.iter().any(|arg| arg == "--src-prefix=a/"));
        assert!(rewritten.iter().any(|arg| arg == "--dst-prefix=b/"));
    }

    #[test]
    fn profile_rewrite_does_not_strip_pathspec_tokens_after_double_dash() {
        let args = vec![
            "diff".to_string(),
            "--color=always".to_string(),
            "HEAD^".to_string(),
            "HEAD".to_string(),
            "--".to_string(),
            "--color".to_string(),
            "--relative".to_string(),
            "file.txt".to_string(),
        ];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::PatchParse);
        let separator = rewritten
            .iter()
            .position(|arg| arg == "--")
            .expect("rewritten args should keep pathspec separator");
        assert_eq!(
            rewritten[separator + 1..],
            [
                "--color".to_string(),
                "--relative".to_string(),
                "file.txt".to_string()
            ]
        );
    }

    #[test]
    fn raw_diff_profile_keeps_rename_flags_untouched() {
        let args = vec![
            "diff".to_string(),
            "--raw".to_string(),
            "-z".to_string(),
            "-M".to_string(),
            "HEAD^".to_string(),
            "HEAD".to_string(),
        ];
        let rewritten = args_with_internal_git_profile(&args, InternalGitProfile::RawDiffParse);
        assert!(rewritten.iter().any(|arg| arg == "-M"));
        assert!(rewritten.iter().any(|arg| arg == "--no-ext-diff"));
        assert!(rewritten.iter().any(|arg| arg == "--no-textconv"));
        assert!(rewritten.iter().any(|arg| arg == "--no-color"));
        assert!(rewritten.iter().any(|arg| arg == "--no-relative"));
    }
}
