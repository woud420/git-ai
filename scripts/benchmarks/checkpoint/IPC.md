# Checkpoint IPC benchmarks

The ignored integration benchmarks use real `CheckpointDelivery` values for
1 KiB, 1 MiB, 64-file, and JSON-escaping/UTF-8 fixtures. They do not alter the
installed runtime. JSON, existing outbox CBOR, and typed Cap'n Proto include delivery validation
on encoding and decoding. Codec measurements cover delivery payloads, excluding
control framing, and construct the existing owned Rust types on decode. The
CBOR measurements call the existing outbox encoder/decoder, including its size
limit. Clone measurements separately expose the cost of copying a delivery.

Run the codec benchmark in an optimized integration-test process:

```bash
GIT_AI_CHECKPOINT_IPC_BENCH_OUTPUT=/private/tmp/checkpoint-ipc-codecs.json \
  make test CARGO_TEST_ARGS="--release --test integration" \
  TEST_FILTER=benchmark_checkpoint_ipc_codecs \
  EXTRA_TEST_BINARY_ARGS="--ignored" NO_CAPTURE=true TEST_THREADS=1
```

Run the socket benchmark separately, with a release daemon built from the same
snapshot and its exact binary path supplied to the prescribed harness:

```bash
cargo build --locked --release --bin git-ai --features test-support
GIT_AI_TEST_BINARY_PATH=/absolute/path/to/target/release/git-ai \
GIT_AI_CHECKPOINT_IPC_BENCH_OUTPUT=/private/tmp/checkpoint-ipc-socket.json \
  make test CARGO_TEST_ARGS="--release --test integration" \
  TEST_FILTER=benchmark_checkpoint_ipc_socket_ack \
  EXTRA_TEST_BINARY_ARGS="--ignored" NO_CAPTURE=true TEST_THREADS=1
```

The socket benchmark requires `GIT_AI_TEST_BINARY_PATH` and uses the prescribed
`TestRepo` harness. Both transports run the same selected executable, against
two independent dedicated daemons with matched content and checkpoint depth.
Measured pairs alternate JSON-first and Cap'n-Proto-first. Both senders borrow
the delivery, and Cap'n Proto timing includes its readiness negotiation.
Delivery IDs differ between lanes while retaining equal length. Raw paired
ratios, differences, and order accompany each transport's p50/p95.

The `GIT_AI_TEST_BINARY_PATH` override selects a prebuilt executable and avoids a
nested build. Other `TestRepo` tests default to building a **debug daemon** and
respect `CARGO_TARGET_DIR`. Reports record the selected binary's absolute path,
hash, version response, observed debug-assertion marker, and whether an override was
used. Preserve the build command and source identity beside reports rather than
inferring optimization settings from the executable's path.
Samples cover direct control-socket request through application ACK;
they exclude CLI process startup. Setup, file writes, delivery construction,
and the exact-ID/blob-content oracle run outside measured samples. Immediately
after every ACK, a pre-captured working-log handle reads storage directly,
without a synchronization fence, and verifies that the delivery was applied
exactly once with the exact captured contents for every file.

Use `benchmark_scoped_checkpoint_vs_ref.py` with
`--comparison-profile fork-before-after-v1` for release CLI-to-daemon comparisons
at the common application-ACK and family-sync boundaries; see the neighboring
README. Neither this codec benchmark nor the direct socket benchmark substitutes
for that CLI measurement or demonstrates crash durability.

To request Cap'n Proto in the candidate release CLI/daemon while retaining a
matched baseline environment, pass `--checkpoint-capnp` to the release harness:

```bash
python3 scripts/benchmarks/checkpoint/benchmark_scoped_checkpoint_vs_ref.py \
  --comparison-profile fork-before-after-v1 \
  --checkpoint-capnp \
  --candidate-source /path/to/clean/candidate \
  --baseline-source /path/to/clean/baseline \
  --baseline-ref exact-baseline-commit \
  --samples 30 --warmups 5 --resource-samples 20 \
  --prefill-depths 0,50,200 \
  --output /private/tmp/checkpoint-ipc-release-before-after.json
```

The option explicitly sets `GIT_AI_CHECKPOINT_CAPNP=true` for both isolated
variants. Ambient feature flags remain excluded. Older binaries may ignore the
new flag. Before timing, native `config feature_flags` readback must expose
`checkpoint_capnp=true` in the candidate. A baseline without that key is recorded
as unsupported; a recognized false value rejects either variant's experiment.
Reports retain these effective-configuration statuses per variant and depth.
This readback does not observe negotiated wire bytes. The option is bound in the protocol,
parameters, fixture contract, and each normalized fixture environment identity.
Matching opt-in environments retain their existing comparability checks;
default/opt-in results require rebaselining. Effective-configuration evidence is
kept separate from fixture-equality digests because the two implementations may
support different wire formats. Decision qualification also requires the native
candidate readback and a known baseline support state. All existing source/build,
acknowledgement, resource, and storage-oracle requirements remain.
The release harness still measures its existing fixed 65-byte edit fixture;
its process-resource counters do not establish large-payload memory behavior.

Defaults are 30 samples, 100 ms of codec warmup per codec/fixture, at least 10 ms
per codec sample, and 5 socket warmups. Codec and operation order alternate to
reduce order effects. Reports retain raw per-operation batch means, iteration
counts, nearest-rank p50/p95, encoded byte counts, source revision/diff identity,
and executable hashes. They do not measure allocations or RSS. A supplied
`GIT_AI_CHECKPOINT_IPC_BENCH_OUTPUT` path receives the JSON report; the same report
is printed with the `CHECKPOINT_IPC_BENCHMARK_JSON=` prefix. Run the two benchmarks
separately when writing reports so one output does not replace the other.

For a quick diagnostic run, override `GIT_AI_CHECKPOINT_IPC_BENCH_SAMPLES=3`,
`GIT_AI_CHECKPOINT_IPC_BENCH_WARMUP_MS=1`,
`GIT_AI_CHECKPOINT_IPC_BENCH_SAMPLE_MS=1`, and
`GIT_AI_CHECKPOINT_IPC_BENCH_SOCKET_WARMUPS=1`. Compare full reports with identical
settings and fixture sizes, and keep the source/binary identities beside any
reported timing. These microbenchmarks report observed distributions; they do
not impose a machine-dependent timing assertion or claim statistical significance.

The [2026-10-26 measurement record](../../../docs/decisions/2026-10-26-checkpoint-ipc-measurements.md)
contains the measured distributions, sanitized raw data, limitations, and the
decision to keep JSON as the default.
