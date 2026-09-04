# Keep The Line Gutter Default

Status: proposed; accepting this change records the decision

Tracking issue: ENG-325

## Context

The VS Code extension offers three `gitai.blameMode` values:

- `off` hides attribution gutter decorations;
- `line` decorates the current line's attributed prompt; and
- `all` decorates every AI-attributed line in the active file.

The fork's extension manifest and configuration fallback both default to
`line`. Upstream PR #2218 changed its manifest default to `off`, making
attribution display opt-in for users without an explicit setting. The choice is
presentation policy: it does not change checkpoint collection, persisted
authorship notes, or the underlying attribution available to commands.

Explicit selections are stored in VS Code's global settings. Keeping the
current default does not overwrite an existing `off`, `line`, or `all` choice
and requires no settings migration.

## Tradeoffs

### Privacy

Line gutters visualize attribution that is already available to the local
extension. The default does not collect or persist additional provenance. It
does make the existence of AI attribution visible on screen, which can matter
during screen sharing; users can select `off` at any time.

### Distraction and performance

`line` is the bounded middle setting: it avoids the persistent visual weight
and full-file decoration work of `all`, while still adding a contextual marker
on the active line. Users who prefer an undecorated editor can opt out globally.

### Discoverability

An `off` default makes the extension's primary review surface invisible until
a user finds the setting or toggle command. `line` demonstrates the feature
without filling the file with annotations and makes the adjacent `off` and
`all` choices discoverable through the status-bar toggle.

## Decision

Keep `line` as the default for users who have not selected a mode. Preserve all
explicit existing settings and keep `off` as an immediate, global opt-out.

No implementation or migration issue is required because this is the fork's
current behavior. Future packaging or onboarding changes must not reset an
explicit `gitai.blameMode` value.

Classify upstream's opt-in-default change as an intentional product-policy
difference in future reconciliation audits.

## Consequences

- New users see a restrained attribution signal on the current line without
  enabling full-file gutters.
- Users may need to choose `off` before privacy-sensitive screen sharing or to
  remove all attribution decorations.
- Existing users retain their configured mode through extension updates.
- The fork carries a deliberate default difference from upstream, while the
  setting values and user controls remain compatible.

Reconsider the default if measured onboarding feedback shows that current-line
decorations create material distraction or privacy incidents. Any future
change must include extension tests, release notes, and explicit verification
that saved user settings are not overwritten.
