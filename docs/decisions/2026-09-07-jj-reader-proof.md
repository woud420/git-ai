# Native jj reader selection (ENG-413)

Status: accepted — selected reader profile and qualification contract for native implementation.

The next implementation uses a bounded reader of immutable jj operation and view
files. The stock CLI remains a test oracle. No observer, checkpoint integration,
or native attribution is enabled by this research increment.

The experiment targets jj `0.45.1-7c41cdeb16b6b321c64e789a966b6adf723816a5`,
with the binary identity recorded in the [initial evidence](2026-09-07-jj-support-evidence.md).
The explicit format profile is `jj-simple-op-store/0.45.1`.

## Why the CLI is insufficient

Queries used `--ignore-working-copy --at-operation=<full-128-hex-id>` and explicit
JSON templates. Ordinary quiescent reads preserved repository manifests, but
removing a synthetic repository's disposable commit index produced a counterexample:
`evolog -G -r @ -n 1` recreated index files. `log -r all() -n 256` independently
did the same. Pinning an operation prevents snapshot/reconciliation; it does not
prevent the CLI's index maintenance.

The [index fallback](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/default_index/store.rs#L519)
explains the writes. The [evolog implementation](https://github.com/jj-vcs/jj/blob/v0.45.1/cli/src/commands/evolog.rs#L118)
collects its starting revisions before applying its output limit. The
[evolution walker](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/evolution.rs#L91)
can walk arbitrary operation ancestry before emitting a result and materializes
individual commit/predecessor objects. A fixed number of CLI processes therefore
does not satisfy the bounded work or zero Git object lookup requirements.

Comparing `evolog -r all()` to exact stored predecessor maps also found incomplete
starting coverage after history visibility changes:

| Scenario | Stored mappings | CLI mappings | Missing |
| --- | ---: | ---: | ---: |
| Abandon | 8 | 5 | 3 |
| Undo | 7 | 5 | 2 |
| Operation restore | 7 | 1 | 6 |
| Git import | 8 | 7 | 1 |

Snapshot, new, commit, describe, rebase, split, squash, absorb and both divergent
heads had matching mappings in these fixtures. A single union query over every
captured historical view recovered the four omissions, but retained the cost and
mutation problems. Operation templates also omit the view ID and the distinction
between absent and empty predecessor evidence.

## Selected evidence contract

The prototype reads only `store/type`, `op_store/type`, `op_heads/type`, head
markers, addressed operations/views, and optionally the workspace's checkout
record. It does not load a jj repository, its commit index, or Git objects.
There are zero reader subprocesses. The qualification harness separately invokes
jj/Git to construct fixtures and compare results.

Operations and views are raw protobuf files named by a BLAKE2b-512 hash of jj's
normalized domain representation, **not** their protobuf bytes. The decoder
reconstructs that representation and verifies both identities. It understands
current bookmarks, tags, remote refs, per-workspace Git HEADs, working copies,
and conflicted reference terms. It rejects unknown fields, wrong wire types,
duplicate map keys, malformed IDs, inconsistent compatibility mirrors, unsupported
legacy forms and content-address mismatches.

Sources: [protobuf schema](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/protos/simple_op_store.proto),
[domain types](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/op_store.rs),
and [store normalization](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/simple_op_store.rs).
The all-zero root operation/view are synthetic; they are traversal boundaries,
not missing files to repair.

The predecessor map records newly created commits with an empty list and rewrites
with their recorded ordered predecessors, including intermediate rewrites inside a
transaction. Split is one-to-many; squash and absorb can be many-to-one. These
relations are evidence for later line mapping, not permission to copy entire
authorship records. In particular, [Git import can record synthetic predecessors](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/git.rs#L806)
by matching change IDs and choosing among divergent candidates. A stored edge
is therefore not uniformly an intrinsic rewrite proof. Existing imported Git commits can be absent from the map;
undo/restore can change the visible view with an empty map. Such transitions must
retain unknown attribution until later phases establish the appropriate evidence.

The batch reader starts from the sampled integrated head set, walks parents to
previously persisted operation IDs, and returns parent-first new operations.
The caller supplies an indexed membership lookup scoped to this jj store; each
answer is cached once per sample. A complete initial sample uses the synthetic
root as its boundary. The saved head set alone is insufficient: after observing
`B -> A`, a late concurrent `D -> A` must recognize A as already observed even
though A is no longer a head. Repeated transient heads `{A, B}` expose the same
problem. Both cases failed the first frontier-only prototype and now have replay
regressions. The durable store must retain operation membership separately from
the compare-and-swap head set.
Unreachable detached operations never enter the batch merely because their files
exist. Redundant ancestor heads are preserved rather than normalized on disk.
Heads and checkout bytes are sampled again after reading; a changed sample fails
without returning a partial batch. Equal samples are not an atomic snapshot or a
claim that no concurrent operation ran. Immutability supplies the evidence;
durable observed and applied frontiers remain separate responsibilities.

The checkout protobuf supplies the actual workspace name and last completed
checkout operation. That immutable operation's view supplies its working-copy
commit. The latest view describes what should be checked out and can differ for
a stale workspace. Checkout context is reported separately from integrated batch
membership; it cannot admit a detached operation into attribution.

## Bounds and failure behavior

| Budget per sample | Default |
| --- | ---: |
| Integrated heads / parent fan-in | 32 |
| Operations / distinct views | 256 / 256 |
| Predecessor map entries plus edges | 4,096 |
| View commit references | 4,096 |
| Individual metadata record | 2 MiB |
| Total metadata bytes | 16 MiB |
| Cooperative elapsed-time checks | 250 ms |
| Git/jj subprocesses and Git object reads | 0 |

Counts and reads are capped before unbounded traversal or file reads. The elapsed
budget is checked around work; it cannot interrupt a blocked filesystem syscall.
A production worker must remain isolated from Trace2 ingestion and be cancellable
at its scheduling boundary. No hard filesystem latency guarantee is claimed.

Missing addressed history, exhausted budgets, unsupported
metadata, hash mismatch or changing sampled heads/checkout reject the entire
sample. There is no partial-success cursor advancement or current-HEAD fallback.
Initial histories exceeding the budget need an explicit bootstrap policy in the
observer phase; silently scanning all history or inventing earlier attribution
would violate this contract.

The on-disk store type markers contain no writer-version field. This profile is
an explicit compatibility assumption tested with a pinned binary; it is not an
automatic version detector. Rejecting structural extensions cannot detect an
unknown writer that changes semantics without changing the schema. Rollout must
remain opt-in and version-qualified, with unsupported history failing closed.
Conflict terms in refs are preserved for hashing; resolving content/tree conflicts
and attributing them is outside this reader proof.

## Reproduction and downstream gate

The portable experiment lives in `scripts/benchmarks/jj/reader/`. Its README lists
the explicit binary inputs, test commands and isolated outputs. No script changes
the user's installation or starts the git-ai daemon. Qualification checks include
ordinary rewrites, imported/abandoned history, divergence, detached operations,
stale/additional workspaces, malformed data, limits and byte/metadata preservation.

ENG-415 may now implement the reader and durable observation boundary against
this explicit contract. The research Python modules are not a production runtime
dependency. ENG-416 must establish causal ordering before any recorded operation
can drive checkpoint migration or authorship side effects.

## Local verification

The committed package passed all 26 tests on macOS with the pinned jj binary and
Homebrew Git. The portable CLI probe also completed all 14 scenario families and
reproduced the six-path index mutation. Syntax, portability/path hygiene and
`git diff --check` passed. Logs are task-local at
`/private/tmp/git-ai-jj-reader-green.log` and
`/private/tmp/git-ai-jj-cli-proof-final.json`. The existing Rust implementation
is unchanged in this research commit; its native attribution integration and
Linux/Windows qualification remain pending.
