# git-ai Plugin for OpenCode

A plugin that integrates [git-ai](https://github.com/woud420/git-ai) with [OpenCode](https://opencode.ai) to automatically track AI-generated code.

## Overview

This plugin hooks into OpenCode's tool execution lifecycle to separate
pre-existing, unattributed changes from edits made by the active AI session. It
uses the `tool.execute.before` and `tool.execute.after` events to:

1. Create a compatibility `human` checkpoint before AI edits, establishing an
   untracked boundary without claiming human authorship
2. Create an AI checkpoint after AI edits (marking the changes as AI-authored with model information)

OpenCode does not emit `known_human` checkpoints. That evidence-backed category
is reserved for editor integrations that can identify actual human input.

## Installation

The plugin is automatically installed by `git-ai install-hooks`.

Build `git-ai` (`cargo build`) and then run the `git-ai install-hooks` or `cargo run -- install-hooks` command to test the entire flow of installing and using the plugin.

## Requirements

- [git-ai](https://github.com/woud420/git-ai#install-and-quick-start) must be built and installed from this fork's source, then configured through `git-ai install-hooks`; the plugin uses the absolute binary path injected at install time
- [OpenCode](https://opencode.ai) with plugin support

## How It Works

The plugin intercepts file editing operations (`edit`, `write`, `patch`, `multiedit`, and `apply_patch`) and:

1. **Before AI edit**: Creates a compatibility `human` checkpoint so changes
   since the last checkpoint remain untracked and are excluded from the AI delta
2. **After AI edit**: Creates an AI checkpoint with:
   - Model information (provider/model ID)
   - Session/conversation ID
   - List of edited file paths

If `git-ai` cannot be launched or the file is not in a git repository, the plugin gracefully skips checkpoint creation without breaking OpenCode functionality. Set `GIT_AI_OPENCODE_DEBUG=1` or `GIT_AI_DEBUG=1` before launching OpenCode to log skipped checkpoint details.

## Development

### Type Checking

Run type checking:
```bash
npm run type-check
```

### Dependencies

Install dependencies:
```bash
npm install
```

## See Also

- [git-ai Documentation](https://github.com/woud420/git-ai)
- [OpenCode Plugin Documentation](https://opencode.ai/docs/plugins/)
