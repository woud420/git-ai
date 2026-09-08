# Native jj registration record fixtures

These synthetic records qualify structural inspection only. They do not model
published source seals, live filesystem identity or native checkout evidence.
The [decision](../../../docs/decisions/2026-09-08-jj-native-registration-records.md)
defines the read boundary; the independent generator fixes field order and keys.

`tests/integration/generate_registration_records.py` uses only Python's standard
library. Run it with `--output-dir` pointing to an empty temporary directory.
Compare its `fixtures/` files with this directory and its Rust metadata file with
`tests/integration/jj_registration_record_vectors.rs`. The generated manifest
records every checksum, scalar key and malformed-vector reason. Generation is
not part of production or normal test execution.

There are 15 canonical and 40 malformed CBOR records, plus two UTF-8 name files.
Large values appear only where they qualify byte-string or field-size limits.
The largest valid source/workspace records are 67,492/99,328 bytes; the 128-KiB
outer boundary fixtures are intentionally malformed, with a separate +1 case.

Malformed fields carry fresh record checksums. Guard values follow the fixed
key encoding so unrelated scalar mismatches do not hide the intended rejection.
Workspace-name limits also have private codec tests that avoid SQL-name joins.
The native baseline reference uses the existing independently frozen v2 fixture;
opaque checkout and seal bytes are explicitly unauthenticated synthetic data.

The directory attributes disable Git line-ending conversion for CBOR records and
exact workspace-name fixtures. Their bytes must remain identical when checked
out with `core.autocrlf=true`, including records whose first bytes look textual.
