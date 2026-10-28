# Native jj lineage qualification (ENG-413)

Status: accepted — exact native transitions with explicit unknown rewrite provenance.

## Accepted evidence boundary

On 2026-10-27 Jean-Michel approved completing the reader proof with proven support
and explicit unknown cases. Exact semantic rewrite relations are not claimed where
jj omits evidence or synthesizes predecessors. This supersedes the earlier gate
requiring intrinsic rewrite proof for every listed operation; it does not enable
native attribution or weaken later line-mapping requirements.

`operations::jj::lineage::verify_transition` joins a native-verified operation and
exactly its nonroot parents. It verifies every operation/view hash and rejects
missing, unrelated, duplicate or corrupt parents. The virtual root has no file.
Workspace transitions retain each parent's exact working-copy OID in native parent
order; absent workspaces stay distinct from present ones, and multiple parents are
not collapsed to an arbitrary latest value. The result makes no claim that these
operations were integrated or durably admitted.

The reader performs no filesystem or Git access. Inputs are capped at 32 parents,
8 MiB total raw evidence and 4,096 aggregate view commit references, in addition to
the existing native envelope and decoder limits. It reuses the existing operation
and view verifiers. Nothing is connected to Git Trace2 ingestion.

## Operation support contract

| Family | Evidence recovered | Attribution boundary |
| --- | --- | --- |
| Snapshot | Exact old/new workspace OIDs and recorded predecessor | Requires a matching immutable checkpoint delta before credit |
| New / commit | New commit entries, recorded rewrites and exact workspace movement | New entries with empty predecessor vectors make no inherited-credit claim |
| Describe / rebase / abandon | Exact native predecessor candidates and per-parent workspace OIDs | Later content mapping must prove retained lines; no blanket note copying |
| Split / squash / absorb | Ordered one-to-many / many-to-one recorded candidates | Later mapping must partition credit without duplication |
| Undo / operation restore | Exact historical OIDs in before/after workspace views, including empty predecessor maps | Reuse existing evidence for identical immutable OIDs; do not invent rewrite edges |
| Git import | Exact imported view state and whatever predecessor entries were actually recorded | Absent or synthetic provenance stays unknown; matching change IDs is insufficient |

`None` predecessor evidence remains distinct from an explicitly recorded empty map.
The API deliberately exposes `recorded_predecessors`, with no method that promotes
those candidates into authorship. Descriptions, command strings, arrival order and
the current checkout cannot manufacture provenance. A restored existing revision
retains its immutable identity, including any already attested authorship; unknown
regions are not relabeled as human.

## Qualification

Regression-first: the new `jj_lineage` integration tests failed to compile against
unchanged main because the transition API did not exist. The focused portable tests
then passed after implementation. The explicit real-jj lane exercises all twelve
operation families above in both colocated and noncolocated TestRepo repositories,
asserts representative exact relations and the empty-map recovery/import cases,
and compares repository bytes before and after native verification while a file is
left unsnapshotted. No daemon or installed runtime is used by those fixtures.

The existing 26 Python reader qualification tests passed again with the official
jj 0.45.1 build. The downloaded macOS arm64 release archive matched the published
SHA-256 `51ba42e3d0682616f6eb015045bfe45289b396f03511f9897f645ce8e9272743`.

Reproduce with explicit `GIT_AI_TEST_JJ_BINARY` and `GIT_AI_TEST_REAL_GIT` paths:

```sh
gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_lineage
gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_lineage_real EXTRA_TEST_BINARY_ARGS=--ignored
python3 -B -m unittest discover -s scripts/benchmarks/jj/reader -p 'test_*.py' -v
```

Real native runtime qualification here is macOS with jj 0.45.1. Linux and Windows
rollout, version expansion, performance qualification and native line attribution
remain their own delivery gates. ENG-415 can use the reader without treating
observation as attribution.
