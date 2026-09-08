# Pinned jj operation-reader experiment

This is the ENG-413 research harness. It is not imported by git-ai and does not
enable native jj attribution. The decision and limits are in
[`jj-reader-proof.md`](../../../../docs/decisions/2026-09-07-jj-reader-proof.md).

Use Python 3.10+ and explicit local binaries. The tests require jj 0.45.1; an
unsupported or missing binary fails rather than silently skipping qualification.
All fixture repositories, config and home directories are temporary. No installed
Git/jj/git-ai configuration is changed and no git-ai daemon is started.

```sh
GIT_AI_TEST_JJ_BINARY=/absolute/path/to/jj \
GIT_AI_TEST_REAL_GIT=/absolute/path/to/real/git \
python3 -B -m unittest discover -s scripts/benchmarks/jj/reader -p 'test_*.py' -v

GIT_AI_TEST_JJ_BINARY=/absolute/path/to/jj \
GIT_AI_TEST_REAL_GIT=/absolute/path/to/real/git \
python3 -B scripts/benchmarks/jj/reader/probe_cli.py \
  --output /absolute/path/to/new-cli-evidence.json
```

`probe_cli.py` refuses an existing output path. Its report contains synthetic
operation IDs, counts and observations. The CLI probe intentionally removes only
the disposable index in its own temporary fixture to expose index-rebuild writes.
The production repository and the user's jj repositories are never inputs to
that mutation experiment.

`decode_store.py` decodes and verifies the current-profile operation/view domain
hashes. `read_batch.py` adds bounded metadata reads, integrated-head DAG traversal,
checkout context and sample consistency checks. Resumed reads use membership in
the durable set of already observed operations; the latest heads alone are not a
complete traversal boundary. The test harness may scan its
small generated repositories to establish an independent completeness/nonmutation
oracle; those scans are not part of the bounded reader.

The reader's subprocess count is zero by construction. The fixture harness uses
Git/jj subprocesses to construct and inspect synthetic history. It tests stored
rewrite evidence, not line attribution. Store type markers do not establish the
writer's version, and the cooperative elapsed budget does not interrupt a blocked
filesystem call. See the decision for fail-closed cases and downstream gates.
