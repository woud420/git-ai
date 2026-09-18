# Socket health checks

The daemon sends a control ping and a trace liveness sentinel during its
periodic socket health check. If either remains unacknowledged for 120 seconds,
it logs one warning until acknowledgments resume. A subsequent stall can
produce a new warning.

`GIT_AI_DAEMON_PING_STALL_SECS` changes that warning window; `0` disables these
pings. Invalid values use the 120-second default. Each ping has a 100 ms
connection/write/read budget, and control responses are limited to 1 KiB.

A ping warning does not restart the daemon. Existing recovery for failed
socket connections remains active, including its minimum-uptime policy.
Pings verify the control handler and trace reader, not completion of queued
Git work. Use `git-ai bg status` to inspect the attribution pipeline.

The trace sentinel creates no repository root, sequence entry, or completion
record. It performs no Git, object, ref, or repository-filesystem lookup.
