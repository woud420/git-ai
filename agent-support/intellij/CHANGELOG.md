# Git AI JetBrains Plugin Changelog

This changelog covers the Git AI plugin in this repository. The checkout has no
`intellij-v*` tags, so dated entries below identify source-version milestones;
they do not claim that this fork published a JetBrains Marketplace artifact.
Intermediate source-version changes remain available in the repository history.

## [Unreleased]

### Changed

- Replaced inherited IntelliJ Platform template documentation with the actual
  installation, support, attribution, privacy, development, and validation
  boundaries of this plugin.
- Documented that high-confidence detection currently covers GitHub Copilot and
  Junie only.

## [0.1.12] - 2026-06-08

### Changed

- Raised the source plugin version to `0.1.12`.

### Fixed

- Updated plugin-version discovery for current JetBrains Platform APIs and
  closed the manifest input stream after version lookup.

## [0.1.3] - 2026-01-27

### Added

- Introduced the Git AI plugin with `agent-v1` checkpoint support,
  GitHub Copilot and Junie detection, and local CLI discovery.
