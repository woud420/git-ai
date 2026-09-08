use super::SKIP_TRACE2_CHECKS_FLAG;

pub(super) fn print_debug_help() {
    eprintln!("git-ai debug - Print diagnostic information for troubleshooting");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  git-ai debug [--skip-trace2-checks]");
    eprintln!("  git-ai debug context --json");
    eprintln!("  git-ai debug jj status --journal PATH --json");
    eprintln!("  git-ai debug jj receipt --journal PATH --source ID --admission ID --json");
    eprintln!("  git-ai debug --help");
    eprintln!();
    eprintln!("Options:");
    eprintln!(
        "  {}  Skip per-git Trace2 config and Trace2 file self-checks",
        SKIP_TRACE2_CHECKS_FLAG
    );
}
