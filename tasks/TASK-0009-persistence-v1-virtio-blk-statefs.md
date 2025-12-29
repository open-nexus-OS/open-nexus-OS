---
title: TASK-0009 Persistence v1 (OS): userspace block device + statefs journal for /state (device keys + bootctl)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (device keys): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Storage docs: docs/storage/vfs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We currently have no durable userspace persistence. Several upcoming subsystems require it:

- Updates A/B (`bootctl`) must survive reboot/boot cycles.
- Keystore device identity keys must be stored under `/state`.
- Future logging “deluxe” wants a persistent journal.

This task introduces a minimal persistence layer:

- a userspace block device frontend (virtio-blk on QEMU),
- and a small, robust journaled key/value store (“statefs”) as the persistence substrate for `/state`.

Kernel remains unchanged.

## Goal

Prove (host + QEMU) that:

- state can be written, synced, and recovered after a “restart cycle” (soft reboot simulation),
- `bootctl` and keystore device key material are stored under `/state` and reloaded on the next cycle.

## Non-Goals

- Full POSIX filesystem semantics (directories, partial writes, mmap, permissions).
- Full partition discovery.
- True VM reset / bootloader integration for reboot persistence proof (follow-up).
 - Snapshots, quotas, and strict cross-service error semantics (follow-ups `TASK-0132`/`TASK-0133`/`TASK-0134`).

## Constraints / invariants (hard requirements)

- **Kernel changes (required prerequisite)**: userspace virtio-blk on QEMU `virt` requires safe MMIO access.
  This is implemented as kernel work in `TASK-0010`. After `TASK-0010` is complete, `statefs/statefsd`
  remain userspace-only.
- **Bounded parsing + bounded replay**: journal replay must be bounded and reject malformed records deterministically.
- **Integrity**: journal records include checksums (CRC32 is fine for v1 integrity; authenticity handled elsewhere).
- **Determinism**: markers stable; tests bounded; no unbounded scanning of a “disk”.
- **No fake success**: “persist ok” markers only after re-open/replay proves the data is present.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - **Userspace MMIO access for virtio-blk**: on QEMU `virt`, virtio-blk is a MMIO device. As of today,
    userspace cannot arbitrarily map MMIO; `SYSCALL_AS_MAP` maps VMOs, not physical ranges. Unless we
    already have an MMIO-cap/VMO/broker path, a true userspace virtio-blk frontend cannot ship with
    “kernel untouched”. In that case we must:
    - create **TASK-0010** (device MMIO access model / safe mapping capability), or
    - explicitly relax the constraint for this task (kernel exposes a safe device mapping capability).
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **VFS integration drift**: current `vfsd` os-lite is a read-only proxy for `pkg:/` and does not support
    mount or generic write paths. Trying to “mount /state into vfsd” in v1 will balloon scope.
    Best-for-OS v1 is: `statefsd` is the `/state` authority with a dedicated client API; VFS mounting is a follow-up.
  - **Soft reboot definition**: with kernel untouched we likely cannot perform a real reboot. We must define
    an honest proof cycle (restart `statefsd` and re-open the block backend; restart the consuming service; or a new init cycle hook).
  - **Device key generation entropy**: persistence does not solve entropy. If keystore device keys are still
    “bring-up insecure”, that must remain explicitly labeled (TASK-0008 RED).
- **GREEN (confirmed assumptions)**:
  - We already have a stub virtio-blk crate (`source/drivers/storage/virtio-blk`) that can be reused as low-level scaffolding once access exists.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **Existing VFS model (today)**: `source/services/vfsd/src/os_lite.rs` (read-only, pkg:/ proxy)

## Stop conditions (Definition of Done)

### Proof (Host)

- Add deterministic tests for the statefs journal engine and replay/compaction:
  - `cargo test -p statefs -- --nocapture` (or `-p statefs-host` if split is needed)
  - Coverage:
    - Put/Get/Delete/List
    - crash/replay: write records, drop instance, reopen and replay → data intact
    - CRC mismatch rejection
    - size limits / path normalization / ENOENT

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Extend expected markers with (order tolerant):
    - `blk: virtio-blk up`
    - `statefsd: ready`
    - `SELFTEST: statefs put ok`
    - `SELFTEST: statefs persist ok`
    - `SELFTEST: bootctl persist ok`
    - `SELFTEST: device key persist ok`

Notes:

- Postflight scripts (if added) must **only** delegate to canonical harness/tests; no `uart.log` greps as “truth”.

## Touched paths (allowlist)

- `source/drivers/storage/virtio-blk/` (reuse/extend as low-level driver)
- `userspace/storage/` (new userspace block abstraction + virtio-blk frontend wrapper)
- `source/services/statefsd/` (new service: journaled KV store over block)
- `userspace/statefs/` (client)
- `source/services/keystored/` (migrate persistence to `/state/keystore/...`)
- `source/services/updated/` (migrate `bootctl` persistence to `/state/boot/...`)
- `source/apps/selftest-client/` (persistence proof markers)
- `scripts/qemu-test.sh`
- `docs/storage/` and `docs/testing/`

## Plan (small PRs)

1. **Statefs core (host-first)**
   - Implement the journal format + replay engine behind a `BlockDevice` trait.
   - Provide a mem-backed BlockDevice for tests.

2. **statefsd service + client**
   - Expose Put/Get/Delete/List/Sync over kernel IPC (compact, versioned byte frames).
   - Emit `statefsd: ready` when endpoints are live.

3. **OS block backend (virtio-blk)**
   - Only proceed if the RED MMIO access requirement is satisfied.
   - Emit `blk: virtio-blk up (ss=... nsec=...)` once probed.

4. **Migrate consumers**
   - `keystored`: read/write device key material under `/state/keystore/...` via `statefs` client.
   - `updated`: read/write bootctl under `/state/boot/bootctl.*` via `statefs` client.

5. **Soft reboot proof**
   - Define a deterministic restart cycle:
     - restart `statefsd` and re-open the block backend (or restart the client + reopen service),
     - then re-read the previously written key.
   - Emit markers only after the second-cycle read succeeds.

6. **Docs**
   - `docs/storage/statefs.md`: journal format, bounds, Sync semantics, durability caveats.
   - Update keystore and updates docs to reference `/state` locations.

## Acceptance criteria (behavioral)

- Host tests prove replay and integrity checks deterministically.
- QEMU run proves put + persist-after-restart and shows bootctl/device key persistence markers.
- Kernel untouched.

## RFC seeds (for later, once green)

- Decisions made:
  - device access model for userspace block devices (MMIO caps/broker)
  - statefs journal format + compaction policy
  - “soft reboot” definition and transition to real reboot proof
