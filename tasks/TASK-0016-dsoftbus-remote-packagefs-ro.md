---
title: TASK-0016 DSoftBus Remote-FS v1: Remote PackageFS proxy (read-only) over authenticated streams
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Depends-on (DSoftBus OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want remote, authenticated access to **packagefs** (read-only) on a peer device over DSoftBus
streams. This enables “fetch from peer” flows and remote inspection/debugging without kernel
changes.

Today:

- `packagefsd` already serves RO bundle entries in OS-lite (`manifest.json`, `payload.elf`, etc.).
- DSoftBus OS backend must be functional first (streams, auth, discovery).

## Goal

Prove in QEMU (single VM dual-node or 2-VM harness once available):

- a node can serve remote packagefs requests over an authenticated DSoftBus stream,
- a peer can stat/open/read/close a file under `/packages/...` / `pkg:/...`,
- all operations are bounded and read-only.

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

## Red flags / decision points

- **RED**:
  - `userspace/dsoftbus` OS backend is currently a placeholder (`userspace/dsoftbus/src/os.rs`).
    Note: OS bring-up streams exist via os-lite services (`netstackd` + `dsoftbusd`) as of TASK-0005,
    but this task still requires factoring the remote-fs protocol into a reusable, bounded handler surface.
- **YELLOW**:
  - On-wire encoding: prefer compact versioned byte frames for OS bring-up; Cap'n Proto schemas may exist as documentation but must not be the only contract.
  - **RPC Format Migration**: This task uses OS-lite byte frames (`PK` magic) as a **bring-up shortcut**. When TASK-0020 (Mux v2) or TASK-0021 (QUIC) lands, consider migrating to schema-based RPC (Cap'n Proto or equivalent). See TASK-0005 "Technical Debt" section for details.

## Contract sources (single source of truth)

- Packagefs behavior: `source/services/packagefsd/src/os_lite.rs`
- DSoftBus stream contract: `userspace/dsoftbus` (`Stream::send/recv` with channel + bytes)
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Add deterministic host tests that spin two host DSoftBus nodes and an in-mem packagefs backend:
  - roundtrip stat/open/read/close
  - negative cases: ENOENT, EBADF, oversize read rejected, path traversal rejected

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with:
    - `dsoftbusd: remote packagefs served`
    - `SELFTEST: remote pkgfs read ok`

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
