# git-ai Extension for Visual Studio

## Experimental support

This source-only Windows extension records high-confidence GitHub Copilot Chat
edits through [this fork of git-ai](https://github.com/woud420/git-ai). It is not
a complete Copilot attribution integration:

- Copilot Chat edits are attributed only when the edit stack contains a known,
  Copilot-specific namespace.
- Inline completions use generic Visual Studio suggestion infrastructure and
  are not safely distinguishable from human input, so they remain unattributed.
- Disk-based edits from external agents are not detected.
- Manual saves produce evidence-backed `known_human` checkpoints.

There is currently no published VSIX artifact for this fork. Build and install
the extension from source before evaluating it.

## Requirements

- Windows
- Visual Studio major version 17 or 18 with the core editor and .NET Framework
  4.8
- The Visual Studio extension development workload when building from source
- git-ai CLI 1.0.23 or newer, built and installed from
  [this fork's source](https://github.com/woud420/git-ai#install-and-quick-start)

## Install

Run this command from the repository root in a Visual Studio Developer
PowerShell:

```powershell
dotnet build agent-support/visualstudio/src/GitAiVS/GitAiVS.csproj -c Release
```

The build writes the package under
`agent-support/visualstudio/src/GitAiVS/bin/Release/`. Open the generated
`.vsix`, choose the Visual Studio instance, complete the installer, and restart
Visual Studio.

### CLI installer behavior

`git-ai install-hooks` skips Visual Studio by default. The explicit
`git-ai install-hooks --visual-studio-extension` opt-in asks git-ai to detect
supported Visual Studio installations and check extension status, but it does
not download or install a VSIX. The current Rust installer prints a Marketplace
URL as its manual fallback; for this fork's source-only distribution, use the
source-built package above.

## Verify and troubleshoot

Open View > Output, select the Debug pane, and look for `[git-ai]` messages.
They report binary discovery, Copilot-specific stack matches, checkpoint
results, and save handling. Then use:

```powershell
git-ai status
git-ai log
```

The first command shows current attribution; after a commit, the second reads
the authorship record through the configured storage backend.

## Uninstall

`git-ai uninstall-hooks` can detect the extension but cannot remove it. In
Visual Studio, open Extensions > Manage Extensions, find `git-ai`, choose
Uninstall, close Visual Studio when prompted, and let the VSIX installer finish.

## Development

Build a debug package from the repository root:

```powershell
dotnet build agent-support/visualstudio/src/GitAiVS/GitAiVS.csproj
```

Run the unit tests:

```powershell
dotnet test agent-support/visualstudio/src/GitAiVS.Tests/GitAiVS.Tests.csproj
```

For interactive debugging, open
`agent-support/visualstudio/src/GitAiVS/GitAiVS.csproj` in Visual Studio, set
`GitAiVS` as the startup project, and press F5. The project launches an
experimental Visual Studio instance through its configured start action.

## License

MIT; see the extension's bundled
[LICENSE](src/GitAiVS/LICENSE).
