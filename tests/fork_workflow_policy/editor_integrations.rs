use super::{Path, collect_markdown_files, fs, local_markdown_targets};

#[test]
fn eng_380_visual_studio_docs_match_install_and_detection_behavior() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/visualstudio/README.md"))
        .expect("Visual Studio README must be readable");
    let design = fs::read_to_string(root.join("agent-support/visualstudio/DESIGN.md"))
        .expect("Visual Studio design must be readable");
    let detector = fs::read_to_string(
        root.join("agent-support/visualstudio/src/GitAiVS/Detection/CopilotEditDetector.cs"),
    )
    .expect("Visual Studio detector must be readable");

    for required in [
        "Experimental support",
        "`git-ai install-hooks` skips Visual Studio",
        "`git-ai install-hooks --visual-studio-extension`",
        "not download or install a VSIX",
        "Extensions > Manage Extensions",
        "Copilot Chat edits",
        "Inline completions",
    ] {
        assert!(
            readme.contains(required),
            "Visual Studio README is missing lifecycle boundary `{required}`"
        );
    }

    assert!(
        readme.contains("agent-support/visualstudio/src/GitAiVS/GitAiVS.csproj")
            && readme.contains("src/GitAiVS/bin/Release/")
            && !readme.contains("GitAiVS.sln"),
        "Visual Studio build instructions must name files and output paths that exist"
    );

    for required in [
        "src/operations/mdm/agents/visual_studio.rs",
        "Chat edits are the only currently evidenced AI detection path",
        "completions are not attributed",
        "`install_vsix()` returns `false`",
    ] {
        assert!(
            design.contains(required),
            "Visual Studio design is missing implementation boundary `{required}`"
        );
    }

    for stale in [
        "Auto-install via `git ai install-hooks`",
        "Stack trace detection for GitHub Copilot (inline + chat)",
        "stack trace analysis proved sufficient",
        "**File**: `src/mdm/agents/visual_studio.rs`",
        "Implementation.Copilot.*",
    ] {
        assert!(
            !design.contains(stale),
            "Visual Studio design retains unsupported claim `{stale}`"
        );
    }

    for prefix in [
        "GitHub.Copilot",
        "Microsoft.VisualStudio.Copilot",
        "Microsoft.VisualStudio.Conversations.UI.Internal.Copilot",
    ] {
        assert!(
            detector.contains(prefix) && design.contains(prefix),
            "Visual Studio design and detector disagree on prefix `{prefix}`"
        );
    }
}

#[test]
fn eng_384_intellij_docs_describe_the_plugin_instead_of_the_template() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs_root = root.join("agent-support/intellij");
    let readme =
        fs::read_to_string(docs_root.join("README.md")).expect("IntelliJ README must be readable");
    let changelog = fs::read_to_string(docs_root.join("CHANGELOG.md"))
        .expect("IntelliJ changelog must be readable");
    let conduct = fs::read_to_string(docs_root.join("CODE_OF_CONDUCT.md"))
        .expect("IntelliJ conduct note must be readable");
    let detector = fs::read_to_string(
        docs_root
            .join("src/main/kotlin/org/jetbrains/plugins/template/listener/StackTraceAnalyzer.kt"),
    )
    .expect("IntelliJ stack-trace analyzer must be readable");

    for required in [
        "# Git AI for JetBrains IDEs",
        "## Support status",
        "## Install",
        "## Uninstall",
        "## How attribution works",
        "## Privacy",
        "## Development",
        "## Validation",
        "## License",
        "GitHub Copilot",
        "Junie",
        "github-copilot-jetbrains",
        "notes_backend.kind",
        "git-ai install-hooks",
        "Settings > Plugins",
        "./gradlew buildPlugin",
        "./gradlew check",
        "Apache License 2.0",
    ] {
        assert!(
            readme.contains(required),
            "IntelliJ README is missing plugin-specific fact `{required}`"
        );
    }

    for pattern in [
        "com.github.copilot",
        "com.intellij.ml.llm.matterhorn.junie",
        "com.intellij.ml.llm.matterhorn",
    ] {
        assert!(
            detector.contains(pattern) && readme.contains(pattern),
            "IntelliJ README and detector disagree on package prefix `{pattern}`"
        );
    }

    for stale in [
        "IntelliJ Platform Plugin Template",
        "Use this template",
        "Template Cleanup",
        "Sample code",
        "MyPluginTest",
        "com.github.username.repository",
        "JetBrains Open Source and Community Code of Conduct",
        "github.com/JetBrains/intellij-platform-plugin-template",
    ] {
        assert!(
            ![&readme, &changelog, &conduct]
                .iter()
                .any(|contents| contents.contains(stale)),
            "IntelliJ documentation retains template text `{stale}`"
        );
    }

    assert_eq!(
        readme.matches("<!-- Plugin description -->").count(),
        1,
        "IntelliJ README must retain one Gradle description start marker"
    );
    assert_eq!(
        readme.matches("<!-- Plugin description end -->").count(),
        1,
        "IntelliJ README must retain one Gradle description end marker"
    );
    assert!(
        conduct.contains("[contribution guide](../../CONTRIBUTING.md)"),
        "IntelliJ conduct note must route contributors to the repository guide"
    );

    for required in [
        "# Git AI JetBrains Plugin Changelog",
        "## [Unreleased]",
        "## [0.1.12]",
        "## [0.1.3]",
        "GitHub Copilot and Junie",
    ] {
        assert!(
            changelog.contains(required),
            "IntelliJ changelog is missing project history `{required}`"
        );
    }

    let mut markdown_files = Vec::new();
    collect_markdown_files(&docs_root, &mut markdown_files);
    let mut broken_links = Vec::new();
    for path in markdown_files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for target in local_markdown_targets(&contents) {
            let target = target.split('#').next().unwrap_or_default();
            if target.is_empty() {
                continue;
            }
            let resolved = path
                .parent()
                .expect("Markdown file must have a parent")
                .join(target);
            if !resolved.exists() {
                broken_links.push(format!(
                    "{} -> {target}",
                    path.strip_prefix(root).unwrap_or(&path).display()
                ));
            }
        }
    }
    assert!(
        broken_links.is_empty(),
        "IntelliJ Markdown contains broken local links:\n{}",
        broken_links.join("\n")
    );
}

#[test]
fn eng_391_vscode_cursor_readme_matches_installer_lifecycle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/vscode/README.md"))
        .expect("VS Code README must be readable");
    let package = fs::read_to_string(root.join("agent-support/vscode/package.json"))
        .expect("VS Code package metadata must be readable");
    let constants = fs::read_to_string(root.join("agent-support/vscode/src/consts.ts"))
        .expect("VS Code constants must be readable");
    let vscode = fs::read_to_string(root.join("src/operations/mdm/agents/vscode.rs"))
        .expect("VS Code installer must be readable");
    let cursor = fs::read_to_string(root.join("src/operations/mdm/agents/cursor.rs"))
        .expect("Cursor installer must be readable");

    for required in [
        "## Support status",
        "VS Code 1.99.3 or newer",
        "Cursor 1.7 or newer",
        "git-ai 1.0.23 or newer",
        "git-ai.git-ai-vscode",
        "externally operated Marketplace",
        "~/.cursor/hooks.json",
        "chat.useHooks",
        "github.copilot.chat.otel.dbSpanExporter.enabled",
        "git-ai uninstall-hooks --dry-run=false",
        "does not remove the extension",
        "restart Cursor",
        "../../data-privacy.md",
    ] {
        assert!(
            readme.contains(required),
            "VS Code/Cursor README is missing lifecycle fact `{required}`"
        );
    }

    for stale in ["Restart VS Code", "latest release of the `git-ai` CLI"] {
        assert!(
            !readme.contains(stale),
            "VS Code/Cursor README retains stale instruction `{stale}`"
        );
    }

    for source_fact in [
        (package.as_str(), "\"vscode\": \">=1.99.3\""),
        (constants.as_str(), "MIN_GIT_AI_VERSION = \"1.0.23\""),
        (vscode.as_str(), "GIT_AI_VSCODE_EXTENSION_ID"),
        (vscode.as_str(), "update_vscode_chat_hook_settings"),
        (cursor.as_str(), "MIN_CURSOR_VERSION"),
        (cursor.as_str(), "hooks.json"),
    ] {
        assert!(
            source_fact.0.contains(source_fact.1),
            "installer/package source is missing documented fact `{}`",
            source_fact.1
        );
    }
}

#[test]
fn eng_392_privacy_docs_disclose_editor_telemetry_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let privacy =
        fs::read_to_string(root.join("data-privacy.md")).expect("privacy guide must be readable");
    let vscode = fs::read_to_string(root.join("agent-support/vscode/README.md"))
        .expect("VS Code README must be readable");
    let vscode_source = fs::read_to_string(root.join("agent-support/vscode/src/extension.ts"))
        .expect("VS Code extension source must be readable");
    let intellij = fs::read_to_string(root.join("agent-support/intellij/README.md"))
        .expect("IntelliJ README must be readable");
    let intellij_source = fs::read_to_string(root.join(
        "agent-support/intellij/src/main/kotlin/org/jetbrains/plugins/template/services/TelemetryService.kt",
    ))
    .expect("IntelliJ telemetry source must be readable");

    for required in [
        "CLI and daemon telemetry is off by default",
        "Bundled editor extension exception",
        "legacy `telemetry_oss` gate",
        "missing setting is not an opt-out",
        "https://us.i.posthog.com",
        "ingest.us.sentry.io",
        "externally operated",
    ] {
        assert!(
            privacy.contains(required),
            "privacy guide is missing editor telemetry boundary `{required}`"
        );
    }
    for required in [
        "## Telemetry",
        "vscode_extension_startup",
        "telemetry_oss",
        "https://us.i.posthog.com",
        "missing setting",
    ] {
        assert!(
            vscode.contains(required),
            "VS Code README is missing telemetry fact `{required}`"
        );
    }
    for identifier in [
        "telemetry_oss",
        "PostHog",
        "Sentry",
        "../../data-privacy.md",
    ] {
        assert!(
            intellij.contains(identifier),
            "IntelliJ telemetry disclosure omits {identifier}"
        );
    }
    for (source, fact) in [
        (vscode_source.as_str(), "config.telemetry_oss === \"off\""),
        (vscode_source.as_str(), "https://us.i.posthog.com"),
        (intellij_source.as_str(), "telemetry_oss"),
        (intellij_source.as_str(), "ingest.us.sentry.io"),
    ] {
        assert!(
            source.contains(fact),
            "editor source is missing fact `{fact}`"
        );
    }
}

#[test]
fn eng_399_opencode_docs_match_managed_plugin_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/opencode/README.md"))
        .expect("OpenCode README must be readable");
    let plugin = fs::read_to_string(root.join("agent-support/opencode/git-ai.ts"))
        .expect("OpenCode plugin must be readable");
    let installer = fs::read_to_string(root.join("src/operations/mdm/agents/opencode.rs"))
        .expect("OpenCode installer must be readable");

    let development_commands = readme
        .split("```bash")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        development_commands,
        ["make dev"],
        "exercise the checkout through its installed dev build"
    );
    let makefile = fs::read_to_string(root.join("Makefile"))
        .unwrap()
        .replace("\r\n", "\n");
    let dev_recipe = makefile
        .split("\ndev:\n")
        .nth(1)
        .unwrap()
        .split("\nclean:")
        .next()
        .unwrap();
    for script in ["scripts/dev.sh", "scripts/dev.ps1"] {
        assert!(dev_recipe.contains(script));
        assert!(root.join(script).is_file());
    }

    for required in [
        "## Support status",
        "`opencode` and `opencode2`",
        "~/.config/opencode/plugins/git-ai.ts",
        "~/.config/opencode/plugin/git-ai.ts",
        "git-ai uninstall-hooks --dry-run=false",
        "make build",
        "10-second",
        "tool input",
        "session ID",
        "local git-ai CLI",
        "does not make direct network requests",
        "../../data-privacy.md",
        "Apache License 2.0",
    ] {
        assert!(
            readme.contains(required),
            "OpenCode README is missing integration fact `{required}`"
        );
    }
    for stale in ["`cargo build`", "`cargo run -- install-hooks`"] {
        assert!(
            !readme.contains(stale),
            "OpenCode README retains unsupported development command `{stale}`"
        );
    }
    assert!(
        plugin.contains("untracked or AI-authored")
            && !plugin.contains("mark code changes as human or AI-authored"),
        "OpenCode plugin header must not turn the compatibility boundary into human evidence"
    );
    for source_fact in [
        "detect_binary_names: &[\"opencode\", \"opencode2\"]",
        ".join(\"plugins\")",
        ".join(\"plugin\")",
    ] {
        assert!(
            installer.contains(source_fact),
            "OpenCode installer is missing documented fact `{source_fact}`"
        );
    }
}

#[test]
fn eng_400_pi_docs_match_managed_extension_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/pi/README.md"))
        .expect("Pi README must be readable");
    let extension = fs::read_to_string(root.join("agent-support/pi/git-ai.ts"))
        .expect("Pi extension must be readable");
    let installer = fs::read_to_string(root.join("src/operations/mdm/agents/pi.rs"))
        .expect("Pi installer must be readable");

    for required in [
        "## Support status",
        "`pi`",
        "~/.pi/agent/extensions/git-ai.ts",
        "~/.pi/agent/git-ai.override.json",
        "git-ai uninstall-hooks --dry-run=false",
        "user-owned",
        "left in place",
        "session path",
        "session ID",
        "model",
        "tool input",
        "tool result",
        "dirty file contents",
        "Bash commands",
        "before and after",
        "local `git-ai` CLI",
        "does not make direct network requests",
        "../../data-privacy.md",
        "Apache License 2.0",
    ] {
        assert!(
            readme.contains(required),
            "Pi README is missing integration fact `{required}`"
        );
    }
    for source_fact in [
        "detect_binary_names: &[\"pi\"]",
        ".join(\"extensions\")",
        ".join(\"git-ai.ts\")",
    ] {
        assert!(
            installer.contains(source_fact),
            "Pi installer is missing documented fact `{source_fact}`"
        );
    }
    for source_fact in [
        "git-ai.override.json",
        "hook_event_name: 'before_command'",
        "hook_event_name: 'after_command'",
        "dirty_files: await readDirtyFiles(call.filepaths)",
        "tool_input: call.toolInput",
        "tool_result: {",
    ] {
        assert!(
            extension.contains(source_fact),
            "Pi extension is missing documented fact `{source_fact}`"
        );
    }
}
