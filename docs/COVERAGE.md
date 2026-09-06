# Code Coverage

## Current status

The repository has Rust coverage tooling, local GNU Make targets, and a GitHub
Actions workflow. The workflow is currently **manual-only**: its sole trigger
is `workflow_dispatch`. It does not run automatically on pull requests or pushes,
so it is not a required PR gate.

Automatic coverage was disabled because daemon session timeouts under
`llvm-cov` instrumentation made roughly half of the runs flaky. The workflow
should re-enable automatic enforcement only after the instrumented daemon tests
have reliable timeout or retry behavior. This limitation does not disable local
coverage measurement or manual workflow runs.

## Threshold

The configured line-coverage threshold is 50%. The 50% threshold applies only
when the manual workflow or `make coverage-check` is actually run. A normal PR
or push can pass its automatic checks without evaluating coverage.

The workflow passes its `COVERAGE_THRESHOLD` value to
`cargo llvm-cov --fail-under-lines`. The Makefile uses the same default and
allows a one-run override:

```bash
make coverage-check
make coverage-check COVERAGE_THRESHOLD=55
```

The threshold originated by rounding a measured 54.10% line-coverage result
down to the nearest multiple of five. It is a checked-in baseline, not a value
that changes automatically on every run.

## Local coverage

Install the coverage tool and Rust component once:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

Run the target that matches the output you need:

| Command | Result |
| --- | --- |
| `make coverage` | Terminal summary |
| `make coverage-html` | HTML report opened in the default browser |
| `make coverage-lcov` | `lcov.info` for IDEs and other tools |
| `make coverage-check` | Terminal run that fails below `COVERAGE_THRESHOLD` |

All four targets exclude `tests/**`, `benches/**`, and `examples/**` from the
calculation. The manually dispatched GitHub workflow uses the same exclusions
and additionally skips the `performance_regression` test.

## Manual GitHub workflow

Start **Coverage** from the repository's Actions page with **Run workflow**.
During that dispatched run, the 50% threshold is enforced. HTML and LCOV upload
steps use `if: always()`, so they are attempted even if the threshold step
fails, and uploaded artifacts are retained for 30 days. This report behavior
applies only to a coverage workflow run, not to every CI run.

## Updating the threshold

From the repository root, run:

```bash
./scripts/update-coverage-threshold.sh
```

The script measures current coverage, rounds it down to the nearest multiple of
five, and updates both `.github/workflows/coverage.yml` and the Makefile's
`COVERAGE_THRESHOLD` default. Review the measured result and both file changes
before committing them.

## Coverage expectations

- Add behavior-focused tests for new code and regressions.
- Use reports to find meaningful untested paths rather than optimizing only for
  the percentage.
- Run the manual workflow or local threshold check when coverage risk is
  material.
- Do not describe the 50% threshold as an automatic merge gate until the
  workflow triggers have actually been restored.
