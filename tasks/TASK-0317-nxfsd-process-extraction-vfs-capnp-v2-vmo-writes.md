---
title: TASK-0317 nxfsd process extraction + vfs.capnp v2 write surface + VMO write path (one authority per store, RFC-0072 Phase 2 closed)
status: Draft
owner: @runtime
created: 2026-08-14
depends-on:
  - TASK-0315
  - TASK-0316
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Store split (decision 4, "one authority per store"): docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Surface contract (Phase 2 + Phase 3 write half): docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md
  - Data plane: docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md · predecessor tasks/TASK-0295 (read splice; writes explicitly deferred there)
  - Kernel seal closure (soft dependency): tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md
---

## Context

Three transitional shortcuts remain at the service seam after TASK-0315/0316:

- **nxfsd is not a process** — the `DataStore` runs in-process inside vfsd (ADR-0043 amendment,
  `source/services/nxfsd/src/lib.rs:12-19`); no service_topology entry, no endpoint, no route.
  Because vfsd is single-threaded, one slow `/data` op stalls every `pkg:/` resolution for all
  apps; the app-host client deadline is 250 ms, so slow FS ops surface as UI **errors**.
- `/data` traffic speaks the private `nexus-vfs-types::fileops` frame codec, not the contracted
  RFC-0072 `vfs.capnp` v2 write ops — Phase 2 is formally unimplemented.
- **The write plane is inline-only**: `OP_WRITE_TEXT`, max 4096 B, offset 0, whole-file replace.
  Writing anything larger than 4 KiB through the surface is impossible, not merely slow;
  TASK-0295 delivered the read splice and explicitly deferred writes with no successor task.

## Goal

- **nxfsd extracted** ("one authority per store", the ADR-0043 follow-up described as
  mechanical): `KernelServer` loop around the process-boundary-agnostic store, service_topology
  entry + spawn + endpoint + vfsd→nxfsd route + policyd allow; the `data` partition access
  (RemoteBlockDevice route from TASK-0315) moves to nxfsd. vfsd becomes a pure router for
  `/data` — a slow store op no longer blocks unrelated vfsd traffic.
- **RFC-0072 Phase 2 closed**: `/data` served over the `vfs.capnp` v2 write ops
  (Create/Write/Truncate/Mkdir/Rename/Remove) with the stable error SSOT; the private `fileops`
  codec is retired from the wire (kept only as internal encoding if still useful, never as the
  contract).
- **VMO write path** (RFC-0072 Phase 3 write half): `OP_WRITE_VMO` — bulk writes as CAP_MOVE'd
  VMO handles with offset/append semantics (engine API from TASK-0316); `INLINE_IO_MAX = 4096`
  enforced with `E2BIG` on the write side exactly as on reads; copy-fallback counted.
- **Hot-path hygiene**: the per-request `format!` + UART `debug_write` markers on splice/readdir
  paths (`vfsd/os_lite.rs:337-342,415`, `store.rs:47`) move behind a debug gate.

## Non-Goals

- Engine-internal changes (TASK-0316 delivered them).
- Kernel-enforced VMO sealing (TASK-0290; fallback counting + honest trust-boundary docs until
  then, same stance as TASK-0295).
- Multi-threaded vfsd — the stall fix is process isolation per store, not threads in vfsd.
- App-visible API changes beyond RFC-0073's existing `svc.files.*` surface.

## Constraints / invariants (hard requirements)

- Identity/authorization unchanged: vfsd remains the single client-facing surface; nxfsd accepts
  only vfsd (sender_service_id check) — a `test_reject_*` proves a direct app→nxfsd call is
  denied.
- Path canonicalization stays at the vfsd boundary + defensive re-validation in nxfsd (bounded
  lengths, no `..` traversal) — negative tests carried over, not re-invented.
- Bounded VMO sizes per write request; header-last release ordering as in the read splice.
- Boot ordering: nxfsd readiness gates `/data` mount markers; no fake `ready`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Codec/route tests for the capnp v2 write ops incl. error-SSOT mapping; VMO write roundtrip
  byte-equality + `E2BIG` write-side enforcement + fallback counter; reject tests (direct-call
  deny, oversize, traversal).

### Proof (OS / QEMU) — required

- `nxfsd: ready` (own process) + `nxfsd: mounted /data (rw, clean)` via the route
- `SELFTEST: vfs write vmo ok` + `SELFTEST: vfs write oversize deny ok`
- `SELFTEST: nxfsd direct-call deny ok`
- Existing stash write/copy + splice + cold-boot ladder green; a deliberately slow `/data` op no
  longer delays a concurrent `pkg:/` read (selftest asserts responsiveness).

## Touched paths (allowlist)

- `source/services/nxfsd/` (main/entry/server), `source/services/vfsd/`
- `tools/nexus-idl/schemas/vfs.capnp` (additive), `userspace/nexus-vfs/`, `userspace/vfs-types/`
- `source/init/nexus-init/` (topology/spawn/routes), `policies/base.toml`
- `source/apps/selftest-client/`, `scripts/qemu-test.sh`
- `docs/rfcs/RFC-0072-…` (Phase 2/3 status; approval-gated), `docs/storage/nxfs.md`
