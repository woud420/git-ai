# Shared native history fixtures

Copy this directory to `tests/integration/fixtures/jj-history/`. The standalone Rust helpers use only `JjOperationEvidence` and their generated constants. No filesystem access, TestRepo, subprocess, production decoder, or hash bypass appears in the helpers.

Integration tests may declare `#[path = "fixtures/jj-history/helpers.rs"] mod native;`. Library-private tests may use a test-only wrapper containing `use crate as git_ai;` followed by `include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/integration/fixtures/jj-history/helpers.rs"));`. The wrapper owns any ordinary test-only dead-code allowance needed for shared unused helpers.

## API and calibrated cases

- `first`, `left`, `right`, `merge` reproduce the existing paired operation/view oracle. MERGE is the current-state baseline used by every new family.
- `rich_parent` reproduces registration successor A→MERGE exactly; `rich_child` supplies B→A. Both preserve the RICH view with the default workspace. `late_branch` is D→LEFT; `mixed_merge` is [MERGE,D]; `converged_merge` is [A,B]. To close D to the virtual root, supply LEFT and FIRST as well.
- `chain(n)` supplies up to 257 MINIMAL-view records rooted at MERGE. With only the last operation sampled, 256/257 is the node boundary. With sampled heads [MERGE,last], 255/256 nonterminals gives a 256/257 read-pair union.
- `raw_chain(false/true)` supplies 256 records with exactly 8MiB/8MiB+1 aggregate operation+view bytes. Every record has a valid independently calculated ID.
- `raw_with_baseline(false/true)` supplies 255 nonterminals for sampled heads [MERGE,last]. Including the freshly read MERGE pair gives exactly 8MiB/8MiB+1 with exactly 256 union slots in both cases. The largest description is 64,214 bytes, below the native metadata limit.
- `predecessor_records(false/true)` supplies two records with aggregate 4096/4097 predecessor keys plus edges, each individually below the native limit.
- `view_records(false/true)` supplies two/three records with aggregate 4096/4097 view references. The first two share one 2048-reference view; each occurrence still counts.
- `raw_size` sums encoded operation and view lengths. The helper also exports the basic graph IDs, generated chain IDs and `MERGE_PAIR_BYTES` (1139).

MINIMAL-view head records do not contain a workspace. Use the fixture's independently retained MERGE/RICH checkout for count/byte/semantic tests; own-checkout metadata must not seed the history graph. To test a reached outside-head checkout, use A with sampled B and preserve the saved registration relation.

## Provenance and validation

`generate.py` calls the existing independent operation/view/evidence oracle scripts. Expected IDs never come from the Rust implementation or the research decoder. It writes compact IDs and small fixed vectors; large bodies are constructed by Rust helpers at test time.

`calibrate.py` verifies all 531 distinct operation hashes and all three view hashes with the retained independent research reader, checks envelope joins, small-record bounds and eight exact/+1 DAG shapes, and compares dynamic bytes to a separately written wire encoder corresponding to the Rust helper. It also confirms generated `vectors.rs` is byte-for-byte reproducible. `calibration.json` records source hashes and exact sizes. This is Python fixture calibration, not a Rust compile or runtime test result. Root owns Rust RED/GREEN qualification.

From the repository root:

```sh
python3 tests/integration/fixtures/jj-history/generate.py --output /tmp/jj-history-vectors.rs
cmp /tmp/jj-history-vectors.rs tests/integration/fixtures/jj-history/vectors.rs
python3 tests/integration/fixtures/jj-history/calibrate.py \
  --oracle-dir tests/integration \
  --reader scripts/benchmarks/jj/reader/decode_store.py \
  --output /tmp/jj-history-calibration.json
```

All metadata is synthetic. No private hostname or repository fixture is committed.
