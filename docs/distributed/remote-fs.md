# Remote FS contracts (DSoftBus)

This document tracks the constrained remote filesystem-style contracts exposed over authenticated
DSoftBus streams.

## v1 slices

- Remote packagefs read-only (`TASK-0016`, `RFC-0028`)
  - Marker evidence: `dsoftbusd: remote packagefs served`
- Remote statefs read-write (`TASK-0017`, `RFC-0030`)
  - Scope: bounded proxy operations over authenticated sessions
  - ACL: deny-by-default, writable namespace is `/state/shared/*` only
  - Reject floor: fail-closed for unauthenticated, ACL, prefix-escape, and oversize requests
  - Audit floor: each remote `PUT`/`DELETE` emits deterministic audit evidence
  - Backend: proxied to `statefsd` with bounded nonce-correlated request/reply matching
  - Marker evidence:
    - `dsoftbusd: remote statefs served`
    - `SELFTEST: remote statefs rw ok`

## out of scope in v1

- Mux/flow-control transport redesign (`TASK-0020`)
- QUIC transport contract (`TASK-0021`)
- Shared no_std DSoftBus core extraction (`TASK-0022`)
