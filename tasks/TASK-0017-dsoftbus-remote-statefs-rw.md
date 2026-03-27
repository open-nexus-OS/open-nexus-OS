---
title: TASK-0017 DSoftBus Remote-FS v1: Remote StateFS proxy (RW, ACL) over authenticated streams
status: Done
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC (remote statefs RW contract): docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md
  - RFC (modular daemon boundary): docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - Depends-on (modularization base): tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md
  - Depends-on (DSoftBus OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Depends-on (statefs): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy/audit semantics): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Related (netstackd modular baseline): tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md
  - Follow-on (mux/flow-control): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Follow-on (QUIC transport): tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Follow-on (core abstraction): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Testing methodology: docs/testing/index.md
  - Testing distributed debug guide: docs/testing/network-distributed-debugging.md
  - Testing contract: scripts/qemu-test.sh
  - Testing contract (2-VM): tools/os2vm.sh
---

## Context

Once `/state` exists, we want controlled remote RW access for a narrow subset of keys to enable
distributed workflows (e.g., shared state sync, remote install staging) while keeping the system
secure and auditable.

This task defines a **proxy**, not a generic remote filesystem.

## Goal

Prove in QEMU:

- remote statefs operations work over authenticated DSoftBus streams,
- writes are restricted by a default ACL (`/state/shared/*` only),
- every remote write is audited (exported via logd once available),
- selftest can roundtrip a RW key to a peer.

## Target-state alignment (post TASK-0015 / RFC-0027)

- Remote-statefs proxying must attach to explicit daemon seams (gateway/session/observability),
  not re-expand `dsoftbusd/src/main.rs` into cross-cutting control flow.
- ACL and audit decisions stay at the gateway/policy boundary and remain independent from transport
  retry mechanics.
- Reconnect behavior must remain idempotent and bounded; no unbounded retry/write loops.

## Current state snapshot (2026-03-25)

- Prerequisite seams are available and proven:
  - `TASK-0015` (`dsoftbusd` modular daemon boundaries) is `Done`.
  - `TASK-0016` (remote packagefs RO over authenticated streams) is `Done`.
  - `TASK-0016B` (`netstackd` modularization + deterministic network proof hardening) is `Done`.
- Prerequisite platform capabilities are available:
  - `TASK-0009` (`statefsd`) is `Done`.
  - `TASK-0008` (policy/audit model) and `TASK-0006` (logd/audit sink) are `Done`.
- Harness/proof baseline is stable:
  - single-VM marker contract in `scripts/qemu-test.sh` is green.
  - two-VM contract in `tools/os2vm.sh` is green and emits typed summaries.

## Implementation status (2026-03-25 closeout)

- Remote statefs v1 request handling is implemented in `dsoftbusd` gateway with:
  - authenticated-session gating,
  - deny-by-default ACL (`/state/shared/*`),
  - fail-closed rejects for unauthenticated / outside-ACL / prefix-escape / oversized requests,
  - deterministic audit labels for remote `PUT`/`DELETE`.
- Required negative tests are present and green:
  - `test_reject_statefs_write_outside_acl`
  - `test_reject_statefs_prefix_escape`
  - `test_reject_oversize_statefs_write`
  - `test_reject_unauthenticated_statefs_request`
- QEMU evidence is green and includes:
  - `dsoftbusd: remote statefs served`
  - `SELFTEST: remote statefs rw ok`
- Persistence-parity closeout:
  - remote statefs proxy is wired to `statefsd` (no authoritative in-daemon shadow backend path),
  - `dsoftbusd` uses bounded fail-closed statefs proxying with internal v2 nonce-correlation for
    request/reply matching (RFC-0019-aligned shared-inbox behavior),
  - `init-lite` routing and policy capability grants now include the `dsoftbusd` -> `statefsd`
    path for `/state/shared/*` remote RW behavior.
- Host contract tests now include protocol-level and gateway-behavior integration checks for
  desired behavior (not fallback internals), including strict v2 nonce-correlation validation.

## Non-Goals

- Full remote `/state` access.
- Remote capability transfer.
- High-throughput bulk data plane (future can use filebuffer/VMO-style chunking).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **ACL enforced by default**:
  - allow only `/state/shared/*` (or equivalent) for remote RW.
  - everything else returns EPERM deterministically.
- **Bounded data**:
  - max key length,
  - max value size per request,
  - chunking for larger payloads if needed.
- **Audit**:
  - every remote PUT/DELETE must produce an audit record (or deterministic UART marker until logd exists).

## Security considerations

### Threat model

- Unauthorized peer writes outside the shared state namespace.
- ACL bypass by crafted key paths/prefix confusion.
- Missing audit trail for remote mutations.
- Replay/duplicate side effects under reconnect/retry edges.

### Security invariants (MUST hold)

- Only authenticated peers may perform remote RW operations.
- ACL remains deny-by-default; only `/state/shared/*` (or declared equivalent) is writable remotely.
- Authorization and audit behavior is deterministic even under transport retries.
- Remote writes/deletes must always produce an audit event (logd or deterministic fallback marker).

### DON'T DO

- DON'T expose full `/state` RW remotely.
- DON'T continue writes silently when audit emission path is unavailable; emit deterministic fallback evidence.
- DON'T encode authorization in client-provided identity fields; rely on authenticated session identity.

### Required negative tests

- `test_reject_statefs_write_outside_acl`
- `test_reject_statefs_prefix_escape`
- `test_reject_oversize_statefs_write`
- `test_reject_unauthenticated_statefs_request`

## Red flags / decision points

- **RED (open blockers at kickoff)**:
  - none; prerequisite blockers are closed (`TASK-0005`, `TASK-0006`, `TASK-0008`, `TASK-0009`, `TASK-0015`, `TASK-0016`, `TASK-0016B` are complete).
- **RED (must-pass implementation gates)**:
  - If remote RW requests can mutate outside the ACL namespace, stop and fix ACL normalization/authorization first.
  - If audit emission cannot be proven for remote `PUT`/`DELETE`, stop and resolve evidence path before claiming progress.
  - If unauthenticated requests are not rejected deterministically, stop and fix identity/authorization flow before feature expansion.
- **YELLOW**:
  - Keep ACL/audit checks transport-independent; retries/reconnects must not duplicate side effects.
  - Maintain deterministic error labels for ACL/authz/bounds rejects to avoid fake-green ambiguity in QEMU logs.
  - **RPC Format Migration**: This task uses OS-lite byte frames as a **bring-up shortcut**. When TASK-0020 (Mux v2) or TASK-0021 (QUIC) lands, consider migrating to schema-based RPC (Cap'n Proto or equivalent). See TASK-0005 "Technical Debt" section for details.

## Contract sources (single source of truth)

- Statefs contract: TASK-0009 (Put/Get/Delete/List/Sync semantics and bounds)
- DSoftBus stream contract: `userspace/dsoftbus`
- QEMU marker contract: `scripts/qemu-test.sh`
- 2-VM contract and typed summaries: `tools/os2vm.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic host tests with two in-proc nodes and in-mem state backend:
  - RW roundtrip within allowed prefix
  - EPERM for disallowed keys
  - oversize write rejected
  - audit marker/record emission validated for PUT/DEL paths
  - unauthenticated request rejected fail-closed
  - prefix-escape/path-normalization bypass rejected fail-closed

### Proof (Build hygiene)

- `just dep-gate`
- `just diag-os`
- `cargo test -p dsoftbusd --tests -- --nocapture`

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - Extend expected markers with:
    - `dsoftbusd: remote statefs served`
    - `SELFTEST: remote statefs rw ok`
  - Reject markers for unauthorized/oversize writes must remain deterministic and fail-closed
  - keep QEMU proofs sequential (single-VM then 2-VM)

## Touched paths (allowlist)

- `source/services/dsoftbusd/` (server handler + marker)
- `source/apps/selftest-client/`
- `source/init/nexus-init/src/os_payload.rs` (route wiring for `dsoftbusd` -> `statefsd`)
- `recipes/policy/base.toml` (capability grant for remote statefs proxy path)
- `scripts/qemu-test.sh`
- `tools/os2vm.sh`
- `docs/distributed/remote-fs.md`

## Plan (small PRs)

1. Define minimal v1 byte-frame protocol for remote statefs (GET/PUT/DEL/LIST/SYNC).
2. Implement server handler in `dsoftbusd`:
   - enforce statefs contract handling at gateway level (current v1 path uses deterministic shadow backend),
   - enforce ACL and bounds,
   - emit `dsoftbusd: remote statefs served` on first successful request,
   - emit audit record for PUT/DEL.
3. Implement client lib + host tests.
4. Add OS selftest: `SELFTEST: remote statefs rw ok`.

## Alignment note (2026-02, low-drift)

- Remote StateFS should build on the current FSM/epoch-managed session lifecycle in `dsoftbusd` and keep reconnect
  behavior idempotent at the RPC layer.
- Keep ACL/audit enforcement independent from transport retries; transport may return bounded `WouldBlock`, but
  authorization decisions must remain deterministic and side-effect-safe.
- Avoid coupling remote-statefs progress to discovery polling frequency; session-facing loops should remain bounded
  and transport-driven.
