# Architecture Inventory

Boundary inventory checked against implementation on 2026-09-15. This classifies
responsibility groups and the enforced dependency subset; it is not an exhaustive
module list or a claim that the whole dependency graph is acyclic. The
[layer boundary audit](layer-boundary-audit.md) records evidence and measured
coordinator boundaries. The [P9 plan](../decisions/2026-07-20-layered-architecture-plan.md)
is a historical roadmap.

## Classification

All module paths below are under `src/`.

| Module | Responsibility and actual boundary |
|---|---|
| `model/{domain,attribution,diff_json,stream_watermark,telemetry,git_oid}` | Domain DTOs/value logic. Domain includes concrete Git concepts. `git_oid` owns the existing OID syntax predicates; `operations::git::oid` re-exports them. |
| `model/{authorship_log,working_log,authorship_log_serialization,checkpoint_delivery,metrics}` | Domain data/serialization plus timestamp or random-ID convenience constructors in working-log, delivery, metric and authorship-serialization code. Do not equate all constructors with pure computation. |
| `model/{attribution_tracker,imara_diff_utils,move_detection,hunk_shift,transcript}` | Attribution computation and transcript values. `attribution_tracker/diff_pipeline.rs` samples `Instant` and emits tracing diagnostics around calculation. |
| `model/clock.rs` | Ambient wall-clock reads. Reducer/analyzer kernels do not use it. |
| `model/stream_types.rs`, `model/stat_snapshot.rs` | Stream shapes/readers consume supplied `BufRead`; stat DTO conversion consumes supplied `fs::Metadata`. Neither fact proves the whole model IO-free or that these types open files. |
| `model/jj*` | Native jj observation/operation/view/registration values and validation. Path byte encoding is platform-specific value conversion; journal persistence is separate. |
| `model/repository/` | SQLite stores, checkpoint outbox and file-lock/storage primitives. Connection ownership varies; see `state-ownership.md` and the persistence contract. |
| `error/` | Shared `GitAiError` representation bridges IO, Git CLI, SQLite, persistence and API errors. Naming this type does not execute an adapter, but it is not an isolated domain-error package. |
| `repo_url.rs`, `uuid.rs`, `checkpoint_content_budget.rs` | Mixed facilities: repository-backed URL resolution, random UUID creation, and config-backed budget construction respectively; not a pure-domain group. |
| `feature_flags.rs`, `config/` | Bootstrap/configuration and ambient environment/file reads. Config still uses Git repository context for prompt-storage decisions. |
| `clients/{api,auth,http}`, `clients/git_cli/` | Network/auth and Git process adapters. No operations imports; the import policy does not prove all adapter internals pure. |
| `operations/git/{cli_parser,command_classification,command_policy}` | Pure parsing/classification of supplied strings. An explicit guarded kernel inside the Git directory; `repo_state` is not part of it. |
| `operations/git/` remainder | Git/ref/object/notes/repository adapters and some mixed workflows. `notes_store` wakes daemon uploads; `trace2_validation` runs daemon self-checks. These existing couplings prevent a directory-wide DAG claim. |
| `operations/daemon/{reducer,analyzers}` | Value-only reduction and built-in classification, with explicit permitted dependencies on model values/error representation and the pure Git kernels. Custom registered analyzers are outside the built-in source guard. |
| `operations/daemon/` remainder | Trace normalization/enrichment, actors, sequencers, sockets, checkpoint/rewrite execution, workers and evidence capture. `actor_coordinator_*` files extend one coordinator; they are not separate state owners. |
| `operations/authorship/` | Mixed computation + Git/notes IO by design: virtual attribution, range authorship and rewrite modules compose the model algebra with storage. |
| `operations/{commands,mdm,streams,ci,jj,workspace_context}` | Commands, integrations, stream readers and VCS-specific observation workflows. Actual adapter/orchestration dependencies remain visible; no blanket purity claim. |
| `metrics/`, `observability/` | Event emission, aggregation, pricing and diagnostics; compatibility re-exports do not make these pure model owners. |
| `cli/`, `main.rs` | Interface and dispatch; `argv[0]` behavior is load-bearing. |
| `tokio_runtime.rs`, `process_timeout.rs`, `notes/reference_server` | Runtime/process glue and reference HTTP-contract infrastructure. |

## Enforcement

`tests/integration/layer_import_policy.rs` is a bounded source check in the
existing integration-test style. It rejects forbidden imports/qualified accesses
and tests the syntax forms it recognizes. Its permitted directions are the
contract; it does not establish semantic purity for arbitrary external code,
macros, callbacks or re-exports.

- Model code outside `model/repository/` must not depend on operations, CLI,
  clients, config, persistence or the orchestration `metrics` facade, nor access
  Tokio or rusqlite. Model metric DTO consumers import `model::metrics`.
- Persistence imports must not name operations or CLI; client imports must not
  name operations. Inline accesses in these adapter layers retain the older
  import-only scope. Relative paths inside inline test modules need manual review.
- The reducer may depend on model values, shared errors and built-in analyzers.
  Built-in analyzers may additionally use the explicitly named pure Git parsing
  and policy modules. Arbitrary `operations::git` dependencies are forbidden.
- Those Git kernels are guarded as part of the closure. Reducer/analyzer guards
  also reject ambient state, process, filesystem/network IO and timing/random
  facilities. The shared error type is a representation dependency, not an
  adapter operation.

There is no exception list. A passing test establishes this bounded dependency
boundary, not whole-model determinism or whole-crate acyclicity. The historical
P9.2 leaks are resolved; future movements must update this inventory and policy
fixtures together instead of expanding an exception list.

## Retained distinctions

- `model/hunk_shift.rs::DiffHunk` is attribution algebra;
  `operations/commands/diff.rs::DiffHunk` is a command DTO. Both remain in use.
- `model/attribution.rs` owns `Attribution`/`LineAttribution`; the tracker retains
  its curated re-export. `model/imara_diff_utils.rs` owns `ByteDiff`.
- Path formatting belongs to `operations/git/path_format.rs`, executable
  discovery/spawn to `cli/git_ai_exe.rs`, CLI environment checks to
  `cli/environment.rs`, and locks to `model/repository/lock_file.rs`.
- Daemon/self-check workflows live in `operations/daemon/{self_check,
  attribution_self_check}.rs`, Git trace2 checks in
  `operations/git/trace2_validation.rs`, and self-check blame classification in
  `operations/commands/blame/self_check_validation.rs`.

Existing mixed boundaries are documented, not permission to introduce new
`common`, `shared`, `utils` or `helpers` modules. Prefer existing concrete owners.
