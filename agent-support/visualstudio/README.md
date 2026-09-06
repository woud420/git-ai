# git-ai Extension for Visual Studio

A Visual Studio extension that tracks AI-generated code using [git-ai](https://github.com/woud420/git-ai#install-and-quick-start).

## Install

Until this fork publishes release artifacts, build and install the CLI from
[this fork's source](https://github.com/woud420/git-ai#install-and-quick-start).
Then install the extension manually:

1. **Install the extension** from the [Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=git-ai.git-ai-visualstudio), or search for `git-ai` in Extensions > Manage Extensions.
2. **Install [`git-ai`](https://github.com/woud420/git-ai)** Follow this fork's [source installation instructions](https://github.com/woud420/git-ai#install-and-quick-start) for your platform.
3. **Restart Visual Studio**

## Requirements

- Visual Studio 2022 (17.0+)
- git-ai CLI >= 1.0.23

## Debug logging

The extension logs detection events to the Visual Studio Output window (Debug pane). Look for lines prefixed with `[git-ai]` to see:

- Which files are being tracked
- Whether edits were detected as AI or human
- Checkpoint success/failure status

## Development

### Build

```bash
dotnet build src/GitAiVS/GitAiVS.csproj
```

Or open `GitAiVS.sln` in Visual Studio and build from the IDE.

### Debug

1. Open `GitAiVS.sln` in Visual Studio 2022
2. Set `GitAiVS` as the startup project
3. Press F5 to launch an Experimental Instance with the extension loaded

### Package

```bash
dotnet build src/GitAiVS/GitAiVS.csproj -c Release
```

The `.vsix` file will be in `src/GitAiVS/bin/Release/`.

## License

MIT
