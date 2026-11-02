# Checkpoint IPC

The CLI sends captured checkpoint deliveries to the daemon's existing control
socket. JSON remains the default. Set the `checkpoint_capnp` feature flag in
configuration, or `GIT_AI_CHECKPOINT_CAPNP=true` in the client environment, to
select the Cap'n Proto transport. Both debug and release defaults are false.
Other control methods and Git's trace2 JSON stream retain their existing format.

The flag selects the client transport only. A daemon always accepts
`checkpoint.deliver.capnp.v1`, deliberately:

- Acceptance adds no work for JSON clients and nothing on the trace2 ingestion
  path; the method is only reached when a client asks for it.
- The decoder is bounded and admission is charged against the existing
  checkpoint slot and byte budget before the body buffer is allocated, so an
  accepted binary request costs no more than a JSON request of the same size.
- `GIT_AI_CHECKPOINT_CAPNP=true` is read from the client's environment, while the
  daemon reads its own configuration. Gating acceptance on the daemon's flag
  would either fail every opted-in delivery against a daemon whose config
  differs, or require the daemon to imitate an older daemon's unknown-method
  refusal so the client falls back, which makes the override a silent no-op.

## Binary negotiation

Each binary delivery uses one connection and three stages:

1. The client encodes a typed delivery and sends a JSON header with method
   `checkpoint.deliver.capnp.v1` and `params.encoded_bytes`.
2. The server checks the declared length and reserves the existing shared
   checkpoint slot and byte budget. It replies with
   `{"ok":true,"data":{"checkpoint_capnp_v1":"ready"}}` before the client
   sends any binary body.
3. The client sends exactly the declared number of unpacked Cap'n Proto bytes.
   The server decodes and validates them, calls the existing checkpoint delivery
   coordinator, waits for application, and sends the ordinary final JSON
   `ControlResponse`.

The readiness reply is admission to transfer a body. Only the final response
acknowledges checkpoint application. Generic single-frame control senders reject
the binary header so callers cannot confuse those responses.

An older daemon rejects the unknown method before receiving any binary data.
Only that explicit refusal permits JSON fallback, on the same connection and
with the same delivery ID. A body-transfer failure, lost final response,
application error, or admission rejection never triggers a live JSON resend.
The existing delivery runtime publishes the same delivery to its durable CBOR
outbox when live delivery fails; replay retains its existing deduplication.

## Limits and ownership

Binary bodies are nonempty and limited to the existing 64 MiB control-frame
budget. JSON and binary checkpoints share the existing connection, request-slot,
and aggregate-byte admission limits. The permit spans transfer, decode,
application, and the final response.

A body has a two-second monotonic receive deadline. Read timeouts poll shutdown
at most 100 ms apart, and trickled bytes cannot reset that deadline. A deadline
with zero time left counts as expired rather than being passed to the socket as
a zero timeout, which Unix rejects as invalid input. A body whose final bytes
arrive is accepted even if the deadline elapsed during that last read.
Disconnect, timeout, decode failure, or application completion releases
admission. The ordinary JSON read timeout is restored after transfer.

Every control response, on either transport, is serialized in memory and
written to the socket with a single `write_all`, so a reply costs one write
rather than one write per serializer token on the unbuffered stream.

The decoder bounds traversal against the actual encoded frame size, bounds
nesting, rejects trailing bytes and unknown enums, and checks collection/text
limits before creating owned values. This prevents pointer aliases from
expanding a small frame into unbounded allocations. Optional empty values remain
distinct from absent values.

The current coordinator consumes owned Rust DTOs. Decoding therefore copies
strings and paths into those types; this implementation does not claim zero-copy
processing across actors. The live JSON sender also borrows its delivery while
encoding, avoiding the previous full delivery clone.

## Schema and verification

The complete delivery schema and reproducible generation instructions are in
[`schemas/README.md`](../../schemas/README.md). Ordinary builds and cross-compiles
use checked-in generated Rust plus the pure Rust runtime; they do not require
the C++ schema compiler. The `Checkpoint Schema` workflow builds the pinned
compiler, caches it by archive digest, and runs `make schema-check` so the
generated file cannot drift from the schema. The generator's `capnpc` pin and
the crate's `capnp` runtime pin move in lockstep: the generator lock resolves
the same `capnp` version as the root `Cargo.toml`, so bump both, regenerate,
and commit the artifact in one change. The workflow's compiler archive digest
moves with the compiler version.

Codec unit tests cover round trips, optional values, every enum, malformed
messages, limits, and pointer alias amplification. Dedicated-daemon regressions
cover negotiation, legacy fallback, mixed controls, exact captured content,
attribution, replay identity, admission, deadlines, shutdown, and final ACKs
behind trace fences. See
[`scripts/benchmarks/checkpoint/IPC.md`](../../scripts/benchmarks/checkpoint/IPC.md)
for comparative measurement commands and their scope.

The [2026-10-26 measurements](../decisions/2026-10-26-checkpoint-ipc-measurements.md)
found a substantial codec gain but no demonstrated small-edit release CLI gain.
JSON remains the default; the binary transport is not adopted as a maintained path.
