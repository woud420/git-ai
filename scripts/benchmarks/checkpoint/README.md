# Checkpoint benchmarks

`benchmark_scoped_checkpoint_vs_ref.py` compares a candidate release binary
with the pinned upstream reference without treating unlike acknowledgement
contracts as the same metric.

The harness records two latency lanes:

- `command_ack_ms`: process start through CLI return. This is reported for each
  binary, but the harness suppresses a cross-project ratio because the fork
  returns after applying the side effect and fsyncing its checkpoint index,
  while the upstream reference returns after bounded in-memory receipt.
- `family_sync_fence_ms`: process start through the repository-family
  synchronization response. This is the common product boundary used for the
  paired comparison.
- `material_observed_ms`: the fence plus exact checkpoint-index and blob-content
  validation. It includes harness parsing time and is reported as an oracle,
  not as the primary product latency.

It creates separate homes, repositories, databases, and daemons under the
platform temporary directory. Unix socket names use a dedicated short `/tmp`
root to stay below platform pathname limits. Each fixture pins the same validated
real Git binary (never the git-ai shim), normalized config, and constant-size
file edits. A second dirty tracked file must remain absent from every checkpoint,
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
  --resource-samples 10 \
  --prefill-depths 0,50,200 \
  --output /private/tmp/scoped-checkpoint-comparison.json
```

Runs with fewer than 20 samples are rejected unless
`--allow-small-sample` is supplied; those runs are explicitly marked as
non-decision evidence. `--debug-stages` enables existing daemon benchmark logs
and parses materialization/append phases, but also marks the result as
diagnostic because logging perturbs timing. Source worktrees must be clean
unless `--allow-dirty-sources` is supplied, which likewise downgrades the run.
`prefill-depth=0` still measures depths 5-34 with the defaults because warmup
checkpoints remain in the fixture; the JSON records the exact measured range.

For a quick, explicitly non-decision smoke run, supplying both
`--candidate-bin` and `--baseline-bin` skips the builds. The harness downgrades
that result because it cannot prove that externally supplied binaries correspond
to the source and toolchain.

The fork explicitly fsyncs and atomically replaces `checkpoints.jsonl`, but its
referenced blob write currently has no explicit file or directory fsync. The
harness validates that the blob exists and has exact content; it does not label
the full live checkpoint crash-durable.

Run the deterministic contract tests with:

```bash
python3 -m unittest discover \
  -s scripts/benchmarks/checkpoint \
  -p 'test_benchmark_scoped_checkpoint_vs_ref.py'
```
