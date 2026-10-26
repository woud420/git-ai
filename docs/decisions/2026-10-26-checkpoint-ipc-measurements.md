# Checkpoint IPC measurements

Status: historical
Date: 2026-10-26

## Decision

Keep JSON as the default and retain Cap’n Proto as an opt-in checkpoint transport.
Cap’n Proto substantially reduces codec time, but the release CLI experiment did
not demonstrate faster small checkpoints. The direct socket experiment showed
modest gains for the large and escaping-heavy fixtures on this machine. These
results support further targeted evaluation, rather than a default change.

The maintained protocol and configuration authority is
[Checkpoint IPC](../architecture/checkpoint-ipc.md). This record describes one
measurement run, not a permanent performance guarantee.

## Environment and source identity

All measurements ran on one Apple M4 Pro, 14 logical CPUs, 24 GiB RAM, arm64,
macOS 15.6. Cargo and rustc were observed as 1.93.0; rustc used LLVM 21.1.8.
Builds were offline and optimized. The clean release CLI builds also used
`--locked`. Power state was uncontrolled.

| Role | Clean source commit | Release executable SHA-256 |
| --- | --- | --- |
| Before: JSON | `29288e324cec3d802d176f340a893b1745cace6c` | `5356895ff4a3a1eae1a3cf18a533e11c9ff81ff09ab23755881a2d20de674360` |
| After: Cap’n Proto opt-in | `6f2a71a3b41fd59ff65b579cf8e94f109dde076d` | `d262133489cd5956b99fb2d284ee88d5571ebaa43d5d0478d8ec96fb6e407bfb` |

The before revision adds benchmarks over main without changing production code.
The after revision adds the transport and removes the full delivery clone from
live JSON serialization. Therefore the release before/after result measures that
combined change. The same-binary socket comparison isolates transport selection:
both senders borrow their deliveries and use the exact after executable above.

The optimized integration-test executable was built from the P2 benchmark
working tree over the after commit. Its five compiled benchmark sources have
SHA-256 `f02c0907becce0734cf5ef86ddaa40ec719648bd5032576b8a316f3f94044b9c`;
the executable SHA-256 is
`75e021c8b37f4b24e60e930a1571b31ab1ff70e8c1728b0df942814a1c85c979`.
The optimized test build enables test-support; the separate production release
daemon was built without that feature. The report explicitly records this dirty
benchmark tree. The release CLI sources
were clean before and after their builds and measurements. The data artifacts
retain lockfile, compiler, build-manifest, executable, and harness digests.

## Codec costs

Each cell below is a nearest-rank p50 over 30 per-operation batch means. Each
batch runs for at least 10 ms, after 100 ms warmup per codec/fixture. All codecs
validate the delivery; owned decode and round trip construct the same Rust DTOs.
These payload sizes exclude control framing. Cap’n Proto uses unpacked encoding.
The fixtures capture the full listed contents; only a first-line sample counter
changes between deliveries. All use one deterministic AI session/model, two
metadata entries, and no stream source. They do not cover active transcript
streaming, metadata-heavy deliveries, concurrent clients, or adversarial input.

| Captured contents | Codec | Encoded bytes | Encode µs | Owned decode µs | Owned round trip µs |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 KiB / 1 file | JSON | 1,741 | 1.068 | 1.976 | 3.103 |
| 1 KiB / 1 file | Existing CBOR | 1,615 | 0.951 | 1.962 | 2.952 |
| 1 KiB / 1 file | Cap’n Proto | 1,664 | 0.401 | 0.964 | 1.404 |
| 1 MiB / 1 file | JSON | 1,085,415 | 478.690 | 491.908 | 1033.966 |
| 1 MiB / 1 file | Existing CBOR | 1,049,169 | 80.880 | 87.789 | 187.006 |
| 1 MiB / 1 file | Cap’n Proto | 1,049,232 | 48.417 | 32.493 | 128.616 |
| 256 KiB / 64 files | JSON | 286,360 | 118.420 | 123.859 | 247.966 |
| 256 KiB / 64 files | Existing CBOR | 276,427 | 28.519 | 37.136 | 66.236 |
| 256 KiB / 64 files | Cap’n Proto | 280,744 | 29.648 | 22.470 | 54.822 |
| 65,534 B / escaping UTF-8 | JSON | 77,895 | 59.457 | 127.832 | 195.033 |
| 65,534 B / escaping UTF-8 | Existing CBOR | 66,160 | 3.116 | 32.190 | 47.183 |
| 65,534 B / escaping UTF-8 | Cap’n Proto | 66,216 | 9.730 | 19.853 | 36.255 |

For 1 MiB, the Cap’n Proto round trip is 8.04× faster than JSON and 1.45× faster
than existing CBOR. Its payload is 3.33% smaller than JSON, and slightly larger
than CBOR. These are codec and payload observations, not live checkpoint ratios.
CBOR already captures much of the codec benefit; no live CBOR transport was
implemented or measured in this experiment.

Delivery cloning was measured separately. Its p50 was 0.557 µs for 1 KiB,
13.000 µs for 1 MiB, 12.428 µs for 64 files, and 1.133 µs for escaping UTF-8.
The full encode/decode/round-trip p95s, raw batch means, and iteration counts are
in the data artifact. No allocations or codec RSS were measured. The coordinator
consumes owned values, so this implementation does not establish zero-copy
processing across actors.

## Same-binary socket application acknowledgement

Each fixture uses two matched dedicated daemons running the same release binary.
There are five warmups followed by 30 pairs, alternating JSON-first and
Cap’n-Proto-first. Timing includes encoding, connection, Cap’n Proto negotiation,
and live application ACK. It excludes CLI startup, file writes, delivery setup,
and storage inspection. After every ACK, an immediate storage read verifies
exactly one application of that delivery ID and exact captured contents.

| Captured contents | JSON p50 / p95 ms | Cap’n Proto p50 / p95 ms | Paired median ratio [95% interval] | Paired median difference ms |
| --- | ---: | ---: | ---: | ---: |
| 1 KiB / 1 file | 21.726 / 22.788 | 21.691 / 21.940 | 0.9932 [0.9916, 1.0055] | -0.147 |
| 1 MiB / 1 file | 41.298 / 45.909 | 39.766 / 43.056 | 0.9764 [0.9335, 0.9942] | -0.979 |
| 256 KiB / 64 files | 36.161 / 75.368 | 36.170 / 42.384 | 0.9839 [0.9477, 1.0020] | -0.491 |
| 65,534 B / escaping UTF-8 | 22.961 / 23.702 | 22.703 / 23.915 | 0.9858 [0.9747, 0.9997] | -0.324 |

Ratios are Cap’n Proto / JSON; lower is faster. The paired statistic is the median
of the 30 within-pair ratios, not the ratio of the two marginal p50s. Differences
are separately the median of within-pair Cap’n-Proto-minus-JSON times. The data
retain raw pair orders and times, plus difference intervals.

Intervals use the existing `paired_bootstrap_ratio` and
`paired_bootstrap_difference` helpers: 10,000 paired-index resamples, a fresh
`random.Random(364)` for each fixture/statistic, `randrange` index selection,
conventional median, and nearest-rank interval endpoints. They are
exploratory descriptions of this host/run, with no multiplicity correction.
The large fixture has a 2.36% paired median reduction and the escaping fixture
1.42%; their intervals exclude 1 in this run. Small and many-file intervals
include 1. The noisy many-file p95 is retained rather than treated as a proven
tail-latency improvement. None of these observations establishes a general
cross-machine or cross-platform gain.

## Release CLI before/after

The existing isolated release harness built both clean snapshots itself with
`cargo build --locked --offline --release --bin git-ai`. The run qualified as
`decision_evidence`: 30 measured pairs, five warmups, 20 parsed resource probes
per variant/depth, prefill depths 0/50/200, and 10,000 bootstrap resamples with
base seed 364 (scenario seed = 364 + prefill depth). Both sides use the fork's
live-application ACK contract. Family sync is
a separate completion boundary.

The fixture is a fixed 65-byte scoped edit. Actual measured journal depths are
5–34, 55–84, and 205–234, because warmups remain in the fixture. Candidate native
configuration readback reported `checkpoint_capnp=true` at every depth. The older
baseline reported the flag as unsupported. This readback is configuration
evidence, not passive observation of wire bytes.

| Prefill | Boundary | Before p50 / p95 ms | After p50 / p95 ms | Paired median ratio [95% interval] |
| ---: | --- | ---: | ---: | ---: |
| 0 | CLI application ACK | 47.990 / 49.262 | 48.039 / 49.194 | 0.9994 [0.9817, 1.0156] |
| 0 | Family sync fence | 48.328 / 49.563 | 48.347 / 49.521 | 0.9999 [0.9821, 1.0131] |
| 50 | CLI application ACK | 47.142 / 48.227 | 47.146 / 51.139 | 1.0000 [0.9980, 1.0059] |
| 50 | Family sync fence | 47.456 / 48.578 | 47.482 / 51.475 | 1.0004 [0.9979, 1.0069] |
| 200 | CLI application ACK | 47.964 / 51.780 | 47.941 / 49.783 | 0.9990 [0.9879, 1.0004] |
| 200 | Family sync fence | 48.288 / 52.080 | 48.279 / 50.093 | 0.9985 [0.9883, 1.0018] |

All six intervals include 1. This run provides no demonstrated small-edit CLI
speedup. It does not test large-payload CLI latency. Resource counters are
reported descriptively for the small fixture; they do not establish a memory
improvement for large payloads. Checkpoint journal/blob totals match between
variants at every depth: 39,655, 75,705, and 183,855 bytes respectively.
The storage oracle verifies exact content and scoped-file exclusion. No crash
was simulated, so the timing/oracle results make no new crash-durability claim.

## Reproduction and retained evidence

Use the maintained commands in the
[IPC benchmark guide](../../scripts/benchmarks/checkpoint/IPC.md), with the exact
source pins above. Select Cargo, rustc, and rustdoc 1.93.0 explicitly and verify
what the GNU Make recipe environment resolves before building. The codec/socket
runs use `make test CARGO_TEST_ARGS="--release --test integration"` with their
respective ignored benchmark filters. Supply the exact prebuilt release daemon
through `GIT_AI_TEST_BINARY_PATH` for socket measurements.

For the release comparison, select `--comparison-profile fork-before-after-v1`,
`--checkpoint-capnp`, `--samples 30 --warmups 5 --resource-samples 20`, and
`--prefill-depths 0,50,200`. Leave source builds enabled; supplying external
binaries would downgrade decision qualification. Run each measurement separately
without competing local tests/builds.

Reproduce and check the stored socket intervals with the existing statistics
helpers:

```bash
python3 scripts/benchmarks/checkpoint/derive_ipc_socket_statistics.py \
  scripts/benchmarks/checkpoint/results/2026-10-26-capnp-codec-socket.json
```

Sanitized artifacts retain raw timings, iteration/pair order, distributions,
uncertainty, resource summaries, correctness observations, and source/binary
identities. They omit local user and temporary paths:

- [Release CLI data](../../scripts/benchmarks/checkpoint/results/2026-10-26-capnp-cli.json).
- [Codec and socket data](../../scripts/benchmarks/checkpoint/results/2026-10-26-capnp-codec-socket.json).

JSON remains the compatibility default. Cap’n Proto can be selected for further
workload evaluation with `GIT_AI_CHECKPOINT_CAPNP=true`. The existing negotiated
old-daemon fallback remains limited to an explicit unknown-method refusal before
body transfer; ambiguous submission continues through the existing same-ID
outbox/replay path. Trace2, actor ownership, and persistence formats are outside
this transport experiment.
