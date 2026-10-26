# Checkpoint transport schema

`checkpoint.capnp` describes the complete live checkpoint delivery envelope.
The durable outbox continues to use its existing CBOR format. Field and enum
ordinals are wire compatibility contracts: add new ordinals rather than
renumbering or reusing old ones. The delivery's existing `schema_version`
remains independently validated.

`generated/checkpoint_capnp.rs` is the unmodified output of Cap'n Proto 1.5.0
and `capnpc` 0.27.0. It is checked in so ordinary builds and cross-compiles need
only the pure Rust `capnp` runtime, without a C++ compiler or a system Cap'n
Proto installation. Generated code lives alongside its schema; handwritten
Rust stays subject to the repository's source length policy.

To regenerate from the repository root, provide Cap'n Proto 1.5.0 on `PATH` or
set `CAPNP` to its executable:

```sh
cargo run --locked --manifest-path tools/checkpoint-schema/Cargo.toml
cargo run --locked --manifest-path tools/checkpoint-schema/Cargo.toml -- --check
```

The second command verifies the checked-in artifact without modifying it. The
generator checks the compiler version and has its own pinned dependency lock.
Do not edit or format the generated Rust directly.

The compiler used for the initial artifact was built temporarily from the
[official 1.5.0 source archive](https://capnproto.org/capnproto-c++-1.5.0.tar.gz),
whose SHA-256 is
`77dbc13ca82d9c87ddb4581dd49559d45b63096433d3dadea08b7f31b360a5ba`.
