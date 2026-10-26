# Checkpoint IPC

The CLI sends captured checkpoint deliveries to the daemon's existing control
socket. JSON remains the default. Set the `checkpoint_capnp` feature flag in
configuration, or `GIT_AI_CHECKPOINT_CAPNP=true` in the client environment, to
select the Cap'n Proto transport. Both debug and release defaults are false.
Other control methods and Git's trace2 JSON stream retain their existing format.

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
at most 100 ms apart, and trickled bytes cannot reset that deadline. Disconnect,
timeout, decode failure, or application completion releases admission. The
ordinary JSON read timeout is restored after transfer.

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
the C++ schema compiler.

Codec unit tests cover round trips, optional values, every enum, malformed
messages, limits, and pointer alias amplification. Dedicated-daemon regressions
cover negotiation, legacy fallback, mixed controls, exact captured content,
attribution, replay identity, admission, deadlines, shutdown, and final ACKs
behind trace fences. See
[`scripts/benchmarks/checkpoint/IPC.md`](../../scripts/benchmarks/checkpoint/IPC.md)
for comparative measurement commands and their scope.
