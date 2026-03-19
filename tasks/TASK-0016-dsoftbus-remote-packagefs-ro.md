---
title: TASK-0016 DSoftBus Remote-FS v1: Remote PackageFS proxy (read-only) over authenticated streams
status: Done
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC (remote packagefs contract): docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md
  - RFC (modular daemon boundary): docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - Depends-on (modularization base): tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md
  - Depends-on (DSoftBus OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Follow-on (remote mutable state): tasks/TASK-0017-dsoftbus-remote-statefs-rw.md
  - Follow-on (mux/backpressure): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Follow-on (transport evolution): tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Follow-on (core/backend split): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Testing methodology: docs/testing/index.md
  - Testing contract: scripts/qemu-test.sh
  - Testing contract (2-VM): tools/os2vm.sh
---

## Context

We want remote, authenticated access to **packagefs** (read-only) on a peer device over DSoftBus
streams. This enables “fetch from peer” flows and remote inspection/debugging without kernel
changes.

Today:

- `packagefsd` already serves RO bundle entries in OS-lite (`manifest.json`, `payload.elf`, etc.).
- DSoftBus OS path for discovery/session/auth is already proven in OS bring-up (`TASK-0005`).
- Daemon modular seams required for this task are now in place (`TASK-0015` Done, `RFC-0027` Completed).
- `userspace/dsoftbus` OS backend remains a placeholder (`userspace/dsoftbus/src/os.rs`), so this task
  should stay daemon-seam first and bounded instead of blocking on a shared backend rewrite.

## Goal

Prove in QEMU (single VM smoke + 2-VM harness):

- a node can serve remote packagefs requests over an authenticated DSoftBus stream,
- a peer can stat/open/read/close a file under `/packages/...` / `pkg:/...`,
- all operations are bounded and read-only.

## Current state snapshot (2026-03-12)

- Structural prerequisite closed: `TASK-0015` is Done (`dsoftbusd` main is thin, orchestration modularized).
- Contract prerequisite closed: `RFC-0027` is Completed.
- Proof baseline is green for single-VM and 2-VM harness paths.
- Main remaining risk for this task is protocol-surface design (RO bounds, path safety, deterministic markers),
  not daemon structure.

## Target-state alignment (post TASK-0015 / RFC-0027)

- Remote-packagefs logic must land on explicit daemon seams, not as new monolithic `main.rs` branches:
  - gateway surface (request routing),
  - session/authenticated stream surface,
  - observability helpers.
- Transport retry/buffering behavior must reuse bounded stream/session handling rather than introducing
  parallel loop logic.
- Marker emission must stay deterministic and routed through existing observability conventions.

## Non-Goals

- Generic remote VFS (directories, rename, write).
- Remote execution or capability transfer.
- Using remote packagefs as a full installer pipeline (separate task).

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Read-only enforcement**: only stat/open/read/close.
- **Bounded IO**:
  - max path length,
  - max read length per request (chunking required),
  - max concurrent handles per connection.
- **Determinism**: stable markers; bounded retries; no busy loops.
- **Security**:
  - only serve requests on an authenticated stream,
  - reject path traversal and non-packagefs schemes deterministically.

## Security considerations

### Threat model

- Unauthenticated or stale session attempts to read package contents.
- Path traversal (`..`, mixed schemes) escaping packagefs namespace.
- Resource exhaustion via oversized path/read requests or too many open handles.

### Security invariants (MUST hold)

- Read service is available only after authenticated session establishment.
- Resolved path remains under packagefs namespace after canonicalization.
- All request sizes and handle counts are capped and enforced fail-closed.
- Logs/markers must not leak secret/session material.

### DON'T DO

- DON'T permit write-like opcodes in this task.
- DON'T serve non-`pkg:/` and non-`/packages/` namespaces.
- DON'T treat failed auth/path validation as recoverable warning; reject deterministically.

### Required negative tests

- `test_reject_unauthenticated_stream_request`
- `test_reject_path_traversal`
- `test_reject_non_packagefs_scheme`
- `test_reject_oversize_read_or_path`

## Red flags / decision points

- **YELLOW**:
  - `userspace/dsoftbus` OS backend is currently a placeholder (`userspace/dsoftbus/src/os.rs`).
    This task should use the now-stable `dsoftbusd` seams first and avoid coupling completion to
    shared backend extraction (tracked by `TASK-0022`).
  - On-wire encoding: prefer compact versioned byte frames for OS bring-up; Cap'n Proto schemas may exist as documentation but must not be the only contract.
  - **RPC Format Migration**: This task uses OS-lite byte frames (`PK` magic) as a **bring-up shortcut**. When TASK-0020 (Mux v2) or TASK-0021 (QUIC) lands, consider migrating to schema-based RPC (Cap'n Proto or equivalent). See TASK-0005 "Technical Debt" section for details.

## Contract sources (single source of truth)

- Packagefs behavior: `source/services/packagefsd/src/os_lite.rs`
- DSoftBus OS seam baseline: `source/services/dsoftbusd/src/os/` (gateway/session/observability boundaries)
- DSoftBus stream contract (host/shared abstraction): `userspace/dsoftbus` (`Stream::send/recv` with channel + bytes)
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Add deterministic host tests that spin two host DSoftBus nodes and an in-mem packagefs backend:
  - roundtrip stat/open/read/close
  - negative cases: ENOENT, EBADF, oversize read rejected, path traversal rejected
  - security rejects: unauthenticated stream and non-packagefs scheme are rejected

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - Extend expected markers with:
    - `dsoftbusd: remote packagefs served`
    - `SELFTEST: remote pkgfs stat ok`
    - `SELFTEST: remote pkgfs open ok`
    - `SELFTEST: remote pkgfs read step ok`
    - `SELFTEST: remote pkgfs close ok`
    - `SELFTEST: remote pkgfs read ok`
  - keep QEMU proofs sequential (single-VM then 2-VM)

## Verified closure evidence (2026-03-16)

- Host proof:
  - `cargo test -p dsoftbusd --tests -- --nocapture` (green)
  - `cargo test -p remote_e2e -- --nocapture` (green, includes remote packagefs roundtrip + negative cases)
- Single-VM proof:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s REQUIRE_DSOFTBUS=1 REQUIRE_DSOFTBUS_REMOTE_PKGFS=1 ./scripts/qemu-test.sh` (green; remote gate auto-skips when cross-vm session markers are absent in single-VM profile)
- 2-VM proof:
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh` (green)
  - Evidence run: `artifacts/os2vm/runs/os2vm_1773847433/summary.json`
- Hygiene proof:
  - repeated `os2vm` launch-phase runs keep tagged QEMU process count at zero after exit and write only run-scoped artifacts under `artifacts/os2vm/runs/`.

## Touched paths (allowlist)

- `source/services/dsoftbusd/` (server handler registration + marker)
- `userspace/dsoftbus/` (OS backend support if needed; prefer separate networking tasks)
- `source/services/packagefsd/` (no behavior change; only a narrow RPC entrypoint if needed)
- `userspace/remote-fs/remote-packagefs/` (client lib)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/distributed/`

## Plan (small PRs)

1. Define a minimal v1 byte-frame protocol for remote packagefs:
   - `PK` magic, version, opcodes STAT/OPEN/READ/CLOSE, error codes.
2. Implement server handler (inside `dsoftbusd`):
   - bridge to local packagefs resolution (`pkg:/...` via packagefsd/vfsd as appropriate),
   - enforce RO + bounds,
   - emit `dsoftbusd: remote packagefs served` on first successful request.
3. Implement client lib and host tests.
4. Add OS selftest marker: `SELFTEST: remote pkgfs read ok`.

## Alignment note (2026-02, low-drift)

- Remote PackageFS should assume the underlying cross-VM session lifecycle is FSM/epoch-managed in `dsoftbusd`
  with reconnect-safe handle cleanup.
- For request/response exchange over the authenticated stream, keep bounded `WouldBlock` handling and deterministic
  retry budgets; do not add unbounded wait loops.
- Discovery receive timing is treated as advisory after peer mapping is learned; remote-fs service loops should not
  depend on discovery polling cadence for forward progress.
