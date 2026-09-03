# Checkpoint benchmarks

`benchmark_scoped_checkpoint_vs_ref.py` compares two release binaries without
treating unlike acknowledgement contracts as the same metric. Its default
`fork-vs-upstream-v1` profile compares a candidate with the pinned upstream
reference.

The harness records two latency lanes:

- `command_ack_ms`: process start through CLI return. This is reported for each
  binary, but the harness suppresses a cross-project ratio because the fork
  returns after applying the side effect, while the upstream reference returns
  after bounded in-memory receipt. The
  `fork-before-after-v1` profile compares this lane only when both snapshots use
  the fork's live-application acknowledgement boundary; crash durability remains
  a separate correctness guard.
- `family_sync_fence_ms`: process start through the repository-family
  synchronization response. This is the common product boundary used for the
  paired comparison.
- `material_observed_ms`: the fence plus exact checkpoint-index and blob-content
  validation. It includes harness parsing time and is reported as an oracle,
  not as the primary product latency.

After both daemons stop, each scenario also records `checkpoint_storage`:
logical bytes in checkpoint journals, content-addressed blob count and bytes,
and their total. This snapshot runs outside the latency and process-resource
samples. The benchmark's fixed 65-byte edits expose depth scaling; the Rust
journal contract tests separately guard against embedding large source bodies
in index records.

It creates separate homes, repositories, databases, and daemons under the
platform temporary directory. Unix socket names use a dedicated short `/tmp`
root to stay below platform pathname limits. Each fixture pins the same validated
real Git binary (never the git-ai shim), normalized config, deterministic origin,
and constant-size file edits. The config uses the legacy `allow_repositories`
spelling because both the pinned upstream and the fork recognize that key, then
reads the effective allowlist and Git path back through each binary. Before
measurement, a disposable repository with a different origin must produce no
checkpoint storage through a `sync.family` fence. Its daemon is stopped and a
fresh daemon is started so the denial control does not contaminate latency or
resource measurements. A second dirty tracked file must remain absent from every checkpoint,
so the oracle also verifies scoped behavior. After warmups, measured pairs alternate candidate-first and
baseline-first. Output includes p50/p95, the median paired difference, a
fixed-seed paired-bootstrap ratio and 95% confidence interval, binary/source/
lockfile digests, environment identity, CLI resource counters, and whole-run
daemon resource counters where macOS `/usr/bin/time -lp` is available. Resource
probes run after and outside the decision-latency samples.

By default the harness builds both clean, exact snapshots itself with the same
compiler and `cargo build --locked --offline --release`, then binds the source,
lockfile, toolchain, build policy, and resulting binary hashes in its output:

```bash
python3 scripts/benchmarks/checkpoint/benchmark_scoped_checkpoint_vs_ref.py \
  --candidate-source /path/to/candidate \
  --baseline-source /path/to/upstream \
  --baseline-ref 6fbc1ef0f4d40232315efc1b907e7ff5526dbea7 \
  --samples 30 \
  --warmups 5 \
  --resource-samples 20 \
  --prefill-depths 0,50,200 \
  --output /private/tmp/scoped-checkpoint-comparison.json
```

To measure an ENG-365 checkout against the committed ENG-364 implementation
under their shared command acknowledgement boundary, select the closed,
versioned before/after profile and pin the baseline checkout exactly:

```bash
python3 scripts/benchmarks/checkpoint/benchmark_scoped_checkpoint_vs_ref.py \
  --comparison-profile fork-before-after-v1 \
  --candidate-source /path/to/candidate \
  --baseline-source /path/to/eng-364-baseline \
  --baseline-ref a864901f9ea088704a8cf1c5d02cebb65c5a28a8 \
  --samples 30 \
  --warmups 5 \
  --resource-samples 20 \
  --prefill-depths 0,50,200 \
  --output /path/to/eng-365-before-after.json
```

The selected profile, both acknowledgement contract IDs, and a digest binding
the profile plus every measurement parameter are recorded in the result.
Paired fixtures are checked automatically; consumers comparing separate result
artifacts must also compare the top-level run-contract digests and rebaseline on
any mismatch. The before/after profile emits paired ratios, paired
differences, and bootstrap confidence intervals for both `command_ack_ms` and
`family_sync_fence_ms`; it does not infer storage or crash durability from those
latency measurements.

Runs with fewer than 20 samples are rejected unless
`--allow-small-sample` is supplied; those runs are explicitly marked as
non-decision evidence. Decision evidence requires exactly 30 samples, 5
warmups, 20 successfully parsed resource probes, depths 0/50/200, 10,000
bootstrap resamples, and bootstrap seed 364. `--debug-stages` enables existing daemon benchmark logs
and parses materialization/append phases, but also marks the result as
diagnostic because logging perturbs timing. Source worktrees must be clean
unless `--allow-dirty-sources` is supplied, which likewise downgrades the run.
`prefill-depth=0` still measures depths 5-34 with the defaults because warmup
checkpoints remain in the fixture; the JSON records the exact measured range.
The depth-200 decision lane ends at checkpoint 255. Run a separate non-decision
depth-250 diagnostic with five warmups when evaluating an implementation whose
first measured checkpoint at 256 performs periodic maintenance; report the
individual boundary sample and maximum rather than folding it into the ordinary
p50 gate.

For a quick, explicitly non-decision smoke run, supplying both
`--candidate-bin` and `--baseline-bin` skips the builds. The harness downgrades
that result because it cannot prove that externally supplied binaries correspond
to the source and toolchain.

The harness validates that each checkpoint index record and referenced blob
exist with exact content. It does not infer crash durability from latency or
from live materialization; that claim requires separate storage and recovery
tests for the exact snapshot under test.

The fresh-run materialization oracle accepts unversioned legacy records,
terminal-checksummed v1 records as emitted by the v1 writer, and compact v2
records. Rust's compatibility reader also accepts canonical-checksummed v1
records whose checksum is not terminal; that historical recovery form is
deliberately outside this benchmark oracle because a fresh measured run cannot
emit it. The Rust storage recovery tests remain authoritative for that path.

Run the deterministic contract tests with:

```bash
python3 -m unittest discover \
  -s scripts/benchmarks/checkpoint \
  -p 'test*.py'
```
