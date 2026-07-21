# Agent Checkpoint Interface

The compatibility surface between AI coding agents (and IDE extensions) and
git-ai. Agents integrate by invoking the checkpoint command around edits;
everything else (attribution, notes, stats) derives from these calls.

## Invocation

```
git-ai checkpoint <preset> [--hook-input <json>] [file …]
```

- `<preset>` selects the agent adapter (`operations/commands/checkpoint_agent/
  presets/`): `claude`, `codex`, `cursor`, `github-copilot`, `gemini`,
  `cline`, `continue-cli`, `amp`, `windsurf`, `opencode`, `pi`, `ai_tab`,
  `firebender`, plus `human` (untracked), `known_human` (real human edits,
  IDE extensions only) and the test presets `mock_ai` / `mock_known_human`.
- Hook input arrives via `--hook-input` or stdin; UTF-8 and UTF-16 LE/BE are
  accepted (BOM/heuristic detection in `cli/git_ai_handlers.rs:618-675`).
  Each preset parses its agent's native hook JSON.
- Positional file paths scope the checkpoint; absent paths mean the preset
  decides (typically from the hook payload).

## Semantics agents rely on

- **Pre/post pairing**: presets fire an untracked (`human`) checkpoint before
  the agent edit and an `ai_agent` checkpoint after; the delta between them
  is what gets attributed to the agent/session/model in the hook input.
- Parsed hook events (preset output): `PreFileEdit`, `PostFileEdit`,
  `PreBashCall`, `PostBashCall`, `KnownHumanEdit`, `UntrackedEdit`
  (`presets/mod.rs:39-98`).
- The orchestrator builds `CheckpointRequest{trace_id, checkpoint_kind,
  agent_id{tool,id,model}, files, stream_source, metadata}` and delivers via
  the daemon control socket (`ControlRequest::CheckpointRun`); the daemon
  applies it inside the per-repo-family serialized flow.
- Exit code 0 even when skipped (unknown repo, repository not in
  `allowed_repositories`) — agents must never fail a user's edit because
  attribution was declined. Skips print a one-line reason to stderr.

## Note format (downstream surface)

Attribution lands as authorship notes, schema `authorship/3.0.0`
(`specs/git_ai_standard_v3.0.0.md`; serializer
`model/authorship_log_serialization.rs`): an attestation section (file path,
then `hash line-ranges` lines) + `---` + JSON metadata (prompts / humans /
sessions maps). Hash forms: bare 16-hex = prompt, `h_` + 14 hex = known
human, `s_…::t_…` = session/trace. Old-format notes remain readable
(sessions-cutover compatibility paths).

## Stability rules

Preset names, the checkpoint CLI form, hook-input tolerance (encodings,
unknown fields ignored), the pre/post pairing contract, and exit-code
semantics are stable. New presets and new optional hook fields are additive.
Removing a preset or changing pairing semantics is a breaking change to
every integrated agent — do not.
