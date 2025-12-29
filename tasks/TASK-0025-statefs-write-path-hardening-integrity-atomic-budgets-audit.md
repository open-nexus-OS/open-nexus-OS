---
title: TASK-0025 StateFS write-path hardening: integrity envelopes + atomic commit + budgets + audit (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (statefs): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Depends-on (policy guardrails): tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md
  - Depends-on (policy authority): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want to harden the write-path for persistent state (`/state`) with:

- integrity envelopes (SHA-256, bounded metadata),
- atomic writes (temp → commit),
- explicit size/latency budgets,
- policy + audit enforcement.

Repo reality today:

- `statefs` / `statefsd` are not implemented yet (TASK-0009).
- VFS does not expose a generic writable `/state` mount; `statefsd` is expected to be the authority for v1.

So this task must be **host-first** and **OS-gated** behind TASK-0009 (and audit/policy tasks).

## Goal

Prove, deterministically:

- Host: atomic + integrity + budgets behave correctly, including negative cases and audit emission to a test sink.
- OS/QEMU (once statefs exists): selftest demonstrates allow/deny markers without fake success.

## Non-Goals

- Full POSIX file semantics (partial writes, rename across directories, permissions).
- Authenticity/signatures of envelopes (integrity only in v1; authenticity is handled by policy/identity).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Bounded memory and bounded parsing:
  - envelope metadata size capped
  - payload size capped
  - journal replay bounded and deterministic.
- No `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.
- No fake markers: only emit success after commit verified.

## Red flags / decision points

- **RED (gating)**:
  - Until TASK-0009 lands, there is no server/client to harden. Do not add QEMU markers before that.
- **YELLOW (audit sink availability)**:
  - Audit via logd depends on TASK-0006. Until then, use a bounded local audit sink in tests and keep OS audit as “best effort”.
- **YELLOW (policy source of truth)**:
  - Policy should not be duplicated across `statefsd`, `policyd`, and ABI filters. Prefer a single authority (policyd/nexus-sel) with ABI filters as guardrails.

## Contract sources (single source of truth)

- Persistence substrate: TASK-0009 `statefs` journal engine + `statefsd` service contract.
- Audit sink: TASK-0006 (`logd` v1) structured records.
- Policy guardrails: TASK-0019 (ABI filter chain) and TASK-0008 (policy model).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- New deterministic tests:
  - happy path: wrap + put_atomic + restart/replay → same bytes
  - tamper: hash mismatch → `EINTEGRITY`
  - oversize: payload > cap → `E2BIG`
  - deadline exceeded: simulated slow sink → `EDEADLINE`
  - audit: allow/deny events emitted with stable fields.

### Proof (OS / QEMU) — only after TASK-0009

Update `scripts/qemu-test.sh` to accept:

- `statefsd: write hardening on (atomic+integrity)`
- `SELFTEST: statefs atomic ok`
- `SELFTEST: statefs integrity deny ok`
- `SELFTEST: statefs oversize deny ok`

and, once writers exist in-tree:

- `updated: bootctl persisted (atomic)`
- `keystored: device key persisted (atomic)`

Notes:

- Postflight scripts must delegate to canonical tests/harness; no independent “log greps = success”.

## Touched paths (allowlist)

- `userspace/storage/` (new integrity envelope library)
- `source/services/statefsd/` (atomic put + verify + budgets; once it exists)
- `userspace/statefs/` (client put_atomic API; once it exists)
- `source/apps/selftest-client/` (markers; once statefs exists)
- `source/services/updated/` and `source/services/keystored/` (migrate critical writes; gated)
- `docs/storage/statefs.md`
- `docs/security/abi-filters.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. **Integrity envelope library (host-first)**
   - Add `userspace/storage/nexus-integrity`:
     - envelope v1: version, alg, size, sha256 hash, bounded metadata (subject/purpose/ts)
     - `wrap(payload, ...)` and `verify(envelope, payload)`
   - Decide encoding: CBOR preferred for boundedness; JSON acceptable if strictly capped and canonicalised.

2. **Statefs atomic put (gated on TASK-0009)**
   - Add `PutAtomic` to statefs protocol:
     - temp record includes envelope + payload
     - verify SHA-256, enforce budgets
     - append commit marker record; only then return success.
   - Budgets:
     - size cap default 1 MiB (configurable)
     - latency budget default 250ms/op (warn if exceeded).
   - Errors:
     - `EINTEGRITY`, `E2BIG`, `EDEADLINE`, `EPERM` mapped deterministically.

3. **Client API (gated on TASK-0009)**
   - `put_atomic(path, payload, meta)` which wraps payload and calls `PutAtomic`.
   - Keep `put()` but deprecate; route to `put_atomic` unless explicitly opted out.

4. **Policy + audit (gated)**
   - Policy:
     - policyd/nexus-sel gate for `state.put_atomic` by subject + path prefixes.
     - ABI filters: examples and optionally enforce client-side deny-by-default for state put.
   - Audit:
     - emit allow/deny records to logd once TASK-0006 exists; otherwise stub to bounded local sink.

5. **Selftest (gated on TASK-0009)**
   - Atomic success marker.
   - Intentional integrity failure marker (wrong hash).
   - Oversize marker.

## Docs

- `docs/storage/statefs.md`: envelope format, atomic semantics, budgets, error codes.
- `docs/security/abi-filters.md`: best practices for allowlisting state prefixes (StatePutAtomic).
- `docs/testing/index.md`: run host tests and interpret OS markers once enabled.
