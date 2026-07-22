pub(super) fn print_config_help() {
    println!("git-ai config - View and manage git-ai configuration");
    println!();
    println!("Usage:");
    println!("  git-ai config                Show all config as formatted JSON");
    println!("  git-ai config <key>          Show specific config value");
    println!("  git-ai config set <key> <value>          Set a config value");
    println!("  git-ai config set <key> <value> --add    Add to array (extends existing)");
    println!("  git-ai config --add <key> <value>        Add to array or upsert into object");
    println!("  git-ai config unset <key>    Remove config value (reverts to default)");
    println!();
    println!("Configuration Keys:");
    println!("  git_path                     Path to git binary");
    println!("  exclude_prompts_in_repositories  Repos to exclude prompts from (array)");
    println!(
        "  allowed_repositories         Repositories where collection is enabled (array; empty = collect nothing)"
    );
    println!("  exclude_repositories         Excluded repos (array)");
    println!("  telemetry                    Master telemetry switch (on/off; default off)");
    println!("  telemetry_oss                Legacy OSS telemetry setting (on/off)");
    println!("  telemetry_enterprise_dsn     Enterprise telemetry DSN");
    println!("  disable_version_checks       Disable version checks (bool)");
    println!("  disable_auto_updates         Disable auto updates (bool)");
    println!("  update_channel               Update channel (latest/next)");
    println!("  feature_flags                Feature flags (object)");
    println!("  api_base_url                 API base URL (default: https://usegitai.com)");
    println!("  api_key                      API key for X-API-Key header");
    println!("  author.name                  git-ai author display name override");
    println!("  author.email                 git-ai author email override");
    println!("  prompt_storage               Prompt storage mode (default/notes/local)");
    println!("  include_prompts_in_repositories  Repos to include for prompt storage (array)");
    println!("  default_prompt_storage       Fallback storage mode for non-included repos");
    println!("  quiet                        Suppress chart output after commits (bool)");
    println!("  allow_superuser              Allow running git-ai as root/superuser (bool)");
    println!(
        "  transcript_streaming_lookback_days  Days to look back when sweeping transcripts (0 = unlimited)"
    );
    println!("  max_checkpoint_file_size_bytes      Per-file checkpoint content limit in bytes");
    println!("  max_checkpoint_total_size_bytes     Per-checkpoint content limit in bytes");
    println!("  max_checkpoint_total_lines          Per-checkpoint content limit in lines");
    println!("  custom_attributes            Custom telemetry attributes, string->string (object)");
    println!("  git_ai_hooks                 Hook name -> shell commands map (object)");
    println!("  codex_hooks_format           Codex hook install format (config_toml/hooks_json)");
    println!("  notes_backend.kind           Notes backend kind (git_notes/http)");
    println!("  notes_backend.backend_url    Notes backend base URL. Required when kind=http.");
    println!(
        "                               May include a path prefix; endpoints are appended to it."
    );
    println!(
        "                               e.g. \"https://app.example.com/api/gitai\" -> requests are"
    );
    println!("                               sent to \"<base>/worker/notes/upload\" and");
    println!("                               \"<base>/worker/notes/?commits=...\".");
    println!();
    println!("Repository Patterns:");
    println!("  For exclude/allow/exclude_prompts_in_repositories, you can provide:");
    println!("    - A glob pattern: \"*\", \"https://github.com/org/*\"");
    println!("    - A URL/git protocol: \"git@github.com:org/repo.git\"");
    println!("    - A file path: \".\" or \"/path/to/repo\" (resolves to repo's remotes)");
    println!();
    println!("Examples:");
    println!("  git-ai config exclude_repositories");
    println!("  git-ai config set disable_auto_updates true");
    println!("  git-ai config set author.name \"Alice Example\"");
    println!("  git-ai config set author.email alice@example.com");
    println!("  git-ai config set exclude_repositories \"private/*\"");
    println!("  git-ai config set exclude_repositories .         # Uses current repo's remotes");
    println!("  git-ai config --add exclude_repositories \"temp/*\"");
    println!("  git-ai config --add allowed_repositories ~/projects/my-repo");
    println!("  git-ai config --add feature_flags.my_flag true");
    println!("  git-ai config --add git_ai_hooks.post_notes_updated \"./my-hook.sh\"");
    println!("  git-ai config set codex_hooks_format hooks_json");
    println!("  git-ai config set allow_superuser true");
    println!("  git-ai config set transcript_streaming_lookback_days 1");
    println!("  git-ai config set custom_attributes '{{\"team\":\"platform\"}}'");
    println!("  git-ai config --add custom_attributes.team platform");
    println!("  git-ai config unset exclude_repositories");
    println!();
    std::process::exit(0);
}
