# Independent native admission fixtures

Canonical packet/state maps use the frozen v1 order/domains; raw operation/view fields are CBOR byte strings. The source, original receipt and FIRST cutoff are selected from the independently frozen schema3 SQL. This fixture has synthetic physical metadata, not source continuity.

Copy this directory's generator, .cbor files, vectors.rs and boundary.rs to tests/integration/fixtures/jj-admission/. metadata.json and boundary.json are useful independent calibration evidence; no test or Rust writer is used to generate expected IDs. Regeneration from the repository root:

```
python3 tests/integration/fixtures/jj-admission/generate_admission_fixtures.py --oracle-dir tests/integration --schema-v3 tests/fixtures/jj-observation-v3.sql --output-dir /tmp/jj-admission-fixtures
rustfmt --edition 2024 --config skip_children=true /tmp/jj-admission-fixtures/vectors.rs /tmp/jj-admission-fixtures/boundary.rs
```

There are36 small records totaling57,823binary bytes:18 structurally valid and18 malformed. Structurally valid wrong-native/order/parent fixtures intentionally exercise the operations boundary, not a hidden model decoder. Each malformed fixture retains a valid original query source/generation selector, while its checksum and admission ID are recomputed from its actual raw bytes. State checksum and packet identity are separate scalar contracts.

`boundary.rs` records256 exact independently hashed native operation IDs and compact length/digest expectations. Tests reconstruct raw operation bytes and canonical packet maps without a production encoder. The exact8MiB packet contains8,271,146raw bytes, all256 pairs below1MiB, with native-valid metadata and one view reference per pair. The over-case changes only the final description by one byte, then recomputes its native operation ID, captured head and canonical packet digest. The files deliberately omit the two8MiB binary packets.

Generation used independent Python CBOR and pinned jj domain oracles. Rust compile/runtime and separate decoder calibration remain independently owned review steps; this README makes no GREEN claim.
