pub(super) fn print_help() {
    eprintln!("git-ai - git proxy with AI authorship tracking");
    eprintln!();
    eprintln!("Usage: git-ai <command> [args...]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  checkpoint         Checkpoint working changes and attribute author");
    eprintln!(
        "{}",
        crate::operations::commands::checkpoint_agent::presets::checkpoint_preset_help()
    );
    eprintln!(
        "    --hook-input <json|stdin>   JSON payload required by presets, or 'stdin' to read from stdin"
    );
    eprintln!("    human [pathspecs...]             Compatibility untracked boundary");
    eprintln!("    known_human [pathspecs...]       Evidence-backed human checkpoint");
    eprintln!("    mock_* [pathspecs...]            Test-only checkpoint presets");
    eprintln!("  log [args...]      Show commit log with AI authorship stats");
    eprintln!("                        Use --raw or --notes to include raw authorship note data");
    eprintln!("  blame <file>       Git blame with AI authorship overlay");
    eprintln!("    --json                 Output blame data as JSON");
    eprintln!("  diff <commit|range>  Show diff with AI authorship annotations");
    eprintln!("    <commit>              Diff from commit's parent to commit");
    eprintln!("    <commit1>..<commit2>  Diff between two commits");
    eprintln!("    --json                 Output in JSON format");
    eprintln!(
        "    --include-stats        Include commit_stats in JSON output (single commit only)"
    );
    eprintln!(
        "    --all-prompts          Include all prompts from commit note in JSON output (single commit only)"
    );
    eprintln!("  stats [commit]     Show AI authorship statistics for a commit");
    eprintln!("    --json                 Output in JSON format");
    eprintln!("  usage              Show local AI usage statistics");
    eprintln!("    --period <1d|3d|7d|30d>  Time window (default: 30d)");
    eprintln!("    --json                 Output in JSON format");
    eprintln!("  analyze [beta]      Analyze agent sessions and effectiveness");
    eprintln!("  status             Show uncommitted AI authorship status (debug)");
    eprintln!("    --json                 Output in JSON format");
    eprintln!(
        "    --diff-only            Report only current-diff stats, omitting the per-checkpoint breakdown"
    );
    eprintln!("  show <rev|range>   Display authorship logs for a revision or range");
    eprintln!("  show-prompt <id>   Display a prompt record by its ID");
    eprintln!("    --commit <rev>        Look in a specific commit only");
    eprintln!(
        "    --offset <n>          Skip n occurrences (0 = most recent, mutually exclusive with --commit)"
    );
    eprintln!("  config             View and manage git-ai configuration");
    eprintln!("                        Show all config as formatted JSON");
    eprintln!("    <key>                 Show specific config value (supports dot notation)");
    eprintln!("    set <key> <value>     Set a config value (arrays: single value = [value])");
    eprintln!("    --add <key> <value>   Add to array or upsert into object");
    eprintln!("    unset <key>           Remove config value (reverts to default)");
    eprintln!("  debug              Print support/debug diagnostics");
    eprintln!("  bg                 Run and control git-ai background service");
    eprintln!("  install-hooks      Configure Git Trace2 and supported agent/editor integrations");
    eprintln!("    --installer-env NAME=ABSOLUTE_PATH");
    eprintln!("                           Package-only user path handoff (repeatable)");
    eprintln!("    --skills               Also install agent skill files");
    eprintln!("    --visual-studio-extension");
    eprintln!(
        "                           Include Visual Studio detection and status checks on Windows"
    );
    eprintln!("                           This does not install a VSIX package");
    eprintln!(
        "  uninstall          Remove git-ai from this machine (hooks, git config, daemon, binaries; --purge for data)"
    );
    eprintln!("  uninstall-hooks    Remove git-ai hooks from all detected tools");
    eprintln!("  ci                 Continuous integration utilities");
    eprintln!("    github                 GitHub CI helpers");
    eprintln!("  git-path           Print the path to the underlying git executable");
    eprintln!("  await [beta]       Wait for the background service to finish all work");
    eprintln!("    --timeout <seconds>    Maximum time to wait (default: 30)");
    eprintln!(
        "  reingest          Redeliver retained metric events through the background service"
    );
    eprintln!("    --all                 Select every retained metric event");
    eprintln!("    --since <duration>    Select a recent duration such as 2h or 7d");
    eprintln!("    --from <time> --to <time>  Select a half-open RFC3339 time range");
    eprintln!("  upgrade            Check for updates and install if available");
    eprintln!("    --force               Reinstall latest version even if already up to date");
    eprintln!("  fetch-notes [remote] Synchronously fetch AI authorship notes");
    eprintln!("    --remote <name>       Explicit remote name (default: upstream or origin)");
    eprintln!("    --json                Output result as JSON");
    eprintln!("  login              Authenticate with Git AI");
    eprintln!("  logout             Clear stored credentials");
    eprintln!("  whoami             Show auth state and login identity");
    eprintln!("  version, -v, --version     Print the git-ai version");
    eprintln!("  help, -h, --help           Show this help message");
    eprintln!();
    std::process::exit(0);
}
