# Git AI for JetBrains IDEs

<!-- Plugin description -->
Git AI records checkpoint evidence for edits made in JetBrains IDEs so the
installed `git-ai` CLI can preserve line-level authorship through commits and
supported Git rewrites. The current detector recognizes GitHub Copilot and
Junie edits from high-confidence stack-trace package prefixes; other agents and
ambiguous matches are not labeled as AI.

The plugin requires a locally installed `git-ai` CLI and an explicitly allowed
repository. This fork currently distributes the plugin from source rather than
as a fork-owned JetBrains Marketplace release.
<!-- Plugin description end -->

## Support status

The plugin is experimental. Version `0.1.12` is built with Java 21 against
IntelliJ IDEA `2025.3.3`, declares a minimum IDE build of `233` (2023.3), and
does not declare a maximum build. IDE compatibility does not imply that every
AI assistant is detected.

AI checkpoints are currently limited to these high-confidence package-prefix
matches in
[`StackTraceAnalyzer.kt`](src/main/kotlin/org/jetbrains/plugins/template/listener/StackTraceAnalyzer.kt):

| Agent | Authorship source | High-confidence package prefixes |
| --- | --- | --- |
| GitHub Copilot | `github-copilot-jetbrains` | `com.github.copilot` |
| Junie | `junie` | `com.intellij.ml.llm.matterhorn.junie`, `com.intellij.ml.llm.matterhorn` |

Class-name matches such as `copilot`, `junie`, `matterhorn`, or `embark` have
medium confidence. They may appear in diagnostic logs but do not create AI
checkpoints. Edits by other assistants also remain unattributed unless another
supported integration supplies checkpoint evidence.

## Install

First install the CLI from this checkout by following the repository's
[source installation instructions](../../README.md#install-and-quick-start).
The plugin requires `git-ai` version `1.0.23` or newer. Allow each repository
that should collect checkpoint evidence:

```bash
git-ai config --add allowed_repositories "$PWD"
```

The root installer calls `git-ai install-hooks`. For a detected compatible
JetBrains IDE, that command tries the IDE's `installPlugins` command and then
the JetBrains Marketplace. Because this fork does not currently publish its
own Marketplace artifact, those routes do not guarantee the plugin from this
checkout. To inspect or use this exact source, build it locally:

```bash
cd agent-support/intellij
./gradlew buildPlugin
```

In the IDE, open **Settings > Plugins**, choose **Install Plugin from Disk**
from the gear menu, select the ZIP under `build/distributions/`, and restart
the IDE. Rebuild and reinstall that ZIP when testing source changes.

## Uninstall

Open **Settings > Plugins > Installed**, select **Git AI**, choose
**Uninstall**, and restart the IDE. `git-ai uninstall` reports that JetBrains
plugins require manual removal; it does not delete the plugin from an IDE.

To remove the CLI and its hooks as well, follow the repository's
[uninstall instructions](../../README.md#uninstall). Removing the CLI does not
remove this plugin or repository-local `.git/ai` data.

## How attribution works

At startup the plugin registers document and virtual-file-system listeners:

1. Before a high-confidence Copilot or Junie document edit, it sends the
   `agent-v1` compatibility pre-edit checkpoint. That preserves the pre-edit
   state as untracked evidence; it is not a claim of known-human authorship.
2. After the edit, a 300 ms debounce batches changes and sends an `ai_agent`
   checkpoint with the detected authorship source and current file contents.
3. A short-lived VFS sweep can capture a subsequent disk refresh only for a
   file already associated with a high-confidence detected agent.
4. Ordinary IDE saves send a separate `known_human` checkpoint after a 500 ms
   debounce.

The plugin does not infer authorship from source-code content. If detection is
absent or ambiguous, it fails closed and does not label the edit as AI.

The CLI writes working checkpoint state below `.git/ai`, then persists commit
authorship through the configured `notes_backend.kind`. Production defaults to
local SQLite; Git Notes and HTTP are explicit alternatives. See the
[checkpoint interface](../../docs/contracts/checkpoint-interface.md) and
[persistence model](../../docs/contracts/persistence-model.md) for the exact
contracts.

## Privacy

Checkpoint requests contain file paths and current file contents so the local
CLI can calculate line attribution. The plugin also writes diagnostic data to
the JetBrains IDE log, including paths, short edit fragments, and checkpoint
input or command failures. Treat IDE logs as potentially source-sensitive.

The plugin contains PostHog analytics and Sentry error reporting. In the
current implementation they initialize unless `telemetry_oss` is exactly
`"off"` in `~/.git-ai/config.json`; a missing setting is not treated as an
opt-out by the plugin. Set it before starting the IDE to disable both:

```json
{
  "telemetry_oss": "off"
}
```

PostHog events require the CLI-created `~/.git-ai/internal/distinct_id` and
include plugin, IDE, OS, and error-status metadata. Sentry receives explicitly
reported plugin errors and exceptions with plugin stack frames. The telemetry
service does not deliberately attach source or prompt contents, but error
events can include truncated command output and searched paths. See the
repository [data-privacy guide](../../data-privacy.md) for the CLI and backend
boundaries.

## Development

Requirements:

- JDK 21 (the Gradle toolchain can provision it through Foojay)
- the checked-in Gradle wrapper
- network access for the IntelliJ Platform, Copilot, Junie, and build
  dependencies on the first run

Run a sandboxed IDE with the plugin:

```bash
cd agent-support/intellij
./gradlew runIde
```

The sandbox is retained in `.sandbox/` so manually installed test plugins
survive rebuilds. `SENTRY_AUTH_TOKEN` is optional for local builds; when set,
it enables Sentry source-context upload during the build.

Key implementation surfaces are:

- [`StackTraceAnalyzer.kt`](src/main/kotlin/org/jetbrains/plugins/template/listener/StackTraceAnalyzer.kt): agent signatures and confidence
- [`DocumentChangeListener.kt`](src/main/kotlin/org/jetbrains/plugins/template/listener/DocumentChangeListener.kt): pre/post AI checkpoints
- [`DocumentSaveListener.kt`](src/main/kotlin/org/jetbrains/plugins/template/listener/DocumentSaveListener.kt): known-human save checkpoints
- [`VfsRefreshListener.kt`](src/main/kotlin/org/jetbrains/plugins/template/listener/VfsRefreshListener.kt): bounded refresh sweeps
- [`GitAiService.kt`](src/main/kotlin/org/jetbrains/plugins/template/services/GitAiService.kt): CLI discovery and checkpoint subprocesses
- [`TelemetryService.kt`](src/main/kotlin/org/jetbrains/plugins/template/services/TelemetryService.kt): analytics, error reporting, and opt-out behavior

## Validation

Run the checks from `agent-support/intellij`:

```bash
./gradlew check
./gradlew buildPlugin
./gradlew verifyPlugin
```

`check` runs the Kotlin tests and coverage report, `buildPlugin` produces the
installable ZIP, and `verifyPlugin` checks compatibility against the Gradle
configuration's recommended IDE set. The optional UI workflow starts
`./gradlew runIdeForUiTests` before running `./gradlew test`.

## License

The plugin source is available under the
[Apache License 2.0](LICENSE), matching the repository's root license.
