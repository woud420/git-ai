# Shared Git/jj workspace path discovery (ENG-414, ENG-415)

Status: accepted — maintained paths-only discovery API; locators require capture revalidation.

`operations::workspace_context::discover(&Path)` exposes the existing Git/jj
path discovery for explicit callers. The diagnostic command retains argument
handling and JSON rendering. The four DTOs expose the same fields, order and
types and add `Debug`; all resolver bodies and the 16 KiB metadata limit remain
unchanged. Shared Git discovery and Trace2 ingestion do not call this API.

These constructible `discovery_only` paths are hints. Discovery performs its
existing path-based ancestor walk and canonicalization outside a future native
capture budget. A native reader must independently bind directories and pointer
relations and validate payloads under bounded reads and a deadline. Discovery
alone does not initialize attribution storage, certify native operation state,
register a source, or enable collection.

Five new TestRepo tests exercise the public API with ordinary and linked Git
workspaces, jj locators without native metadata, linked jj pointers, and malformed
nested boundaries. They compare success/error JSON byte-for-byte with the CLI
and check that fixture contents remain unchanged. Their initial RED run failed
on the absent public module before the extraction. Existing diagnostic tests
supply the broader malformed-layout, no-spawn and real-jj regression cases.

Validation: fresh build, all five new tests (plus one existing checkout test
matching the filter), all 15 diagnostic cases including three pinned real-jj
0.45.1 cases, twelve source/storage policy tests and 33 fork workflow policy
tests passed. Rust 1.93 all-target lint and final format checks passed. An
independent comparison confirmed that all eleven helper bodies, the complete
CLI handler, DTO shape/order and metadata cap were unchanged. Logs are retained as `git-ai-jj-workspace-context-*`.
