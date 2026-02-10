---
title: TASK-0017 DSoftBus Remote-FS v1: Remote StateFS proxy (RW, ACL) over authenticated streams
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Depends-on (DSoftBus OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Depends-on (statefs): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy/audit semantics): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Testing contract: scripts/qemu-test.sh
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

## Red flags / decision points

- **RED**:
  - Blocked until `statefsd` exists (TASK-0009) and DSoftBus OS streams exist (TASK-0005).
- **YELLOW**:
  - Audit sink dependency: if TASK-0006 is not landed yet, we must explicitly fall back to UART audit markers and later migrate.
  - **RPC Format Migration**: This task uses OS-lite byte frames as a **bring-up shortcut**. When TASK-0020 (Mux v2) or TASK-0021 (QUIC) lands, consider migrating to schema-based RPC (Cap'n Proto or equivalent). See TASK-0005 "Technical Debt" section for details.

## Contract sources (single source of truth)

- Statefs contract: TASK-0009 (Put/Get/Delete/List/Sync semantics and bounds)
- DSoftBus stream contract: `userspace/dsoftbus`
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic host tests with two in-proc nodes and in-mem state backend:
  - RW roundtrip within allowed prefix
  - EPERM for disallowed keys
  - oversize write rejected

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `dsoftbusd: remote statefs served`
    - `SELFTEST: remote statefs rw ok`

## Touched paths (allowlist)

- `source/services/dsoftbusd/` (server handler + marker)
- `userspace/statefs/` (client used by server bridge)
- `userspace/remote-fs/remote-statefs/` (client lib)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/distributed/remote-fs.md`

## Plan (small PRs)

1. Define minimal v1 byte-frame protocol for remote statefs (GET/PUT/DEL/LIST/SYNC).
2. Implement server handler in `dsoftbusd`:
   - bridge to local `statefs` client,
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
