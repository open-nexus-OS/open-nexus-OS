---
title: TASK-0009 Persistence v1 (OS): userspace block device + statefs journal for /state (device keys + bootctl)
status: In review
owner: @runtime
created: 2025-12-22
updated: 2026-02-06
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (device keys): tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Depends-on (MMIO mapping primitive): tasks/TASK-0010-device-mmio-access-model.md
  - Depends-on (audit sink): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Storage docs: docs/storage/vfs.md
  - Testing contract: scripts/qemu-test.sh
enables:
  - TASK-0034: Delta updates v1.1 (persistent bootctl + resume checkpoints)
  - TASK-0007 v1.1: Persistent A/B updates (moved to TASK-0034)
follow-up-tasks:
  - TASK-0034: Delta updates v1.1 (persistent bootctl + resume checkpoints)
  - TASK-0025: StateFS write-path hardening (integrity envelopes + atomic commit + budgets + audit)
  - TASK-0026: StateFS v2a (2PC crash-atomicity + bounded compaction + fsck tool)
  - TASK-0130: Packages v1b (install into `/state/apps/...` with atomic commit)
  - TASK-0018: Crashdumps v1 (store crash artifacts under `/state/crash/...`)
  - TASK-0051: Recovery mode v1b (safe tools: fsck/slot/ota + recovery CLI + proofs)
  - TASK-0241: L10n v1.0b OS (persist `ui.locale` / catalogs state)
  - TASK-0243: Soak v1.0b OS (persist run summaries/exports under `/state/...`)
  - TASK-0031: Zero-copy VMOs v1 plumbing (mentions persistence/statefs as prerequisite)
  - TASK-0027: StateFS encryption-at-rest v2b (builds on statefs v1 substrate)
  - TASK-0132: Storage errors vfs semantic contract (tighten error semantics)
  - TASK-0133: StateFS quotas v1 (accounting/enforcement)
  - TASK-0134: StateFS v3 (snapshots/compaction/mounts)
---

## Current Status (2026-02-06)

**Completed**: Host + QEMU persistence proofs are deterministic and green under modern virtio-mmio.

### Completed
- [x] `statefsd` service with memory + virtio-blk backends
- [x] `statefs` userspace crate with JournalEngine
- [x] `storage-virtio-blk` crate with BlockDevice trait
- [x] Host tests passing (35 tests)
- [x] IPC slot routing infrastructure (slot_map, slot_probe)
- [x] MMIO grants from init-lite to statefsd

### Known integration gate (QEMU/CI)
- QEMU persistence proof requires **modern virtio-mmio** settings for virtio-blk; see
  `docs/dev/platform/qemu-virtio-mmio-modern.md`.
- Canonical harness phases must not mix unrelated late markers (e.g. DHCP) into the persistence proof phase.
- DHCP/virtio-net debugging is out of scope for this task; handle it in a separate networking/debug slice.

**Handoff**: `.cursor/handoff/current.md`

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

## Current repo state (to prevent drift)

- `userspace/statefs/` contains the host-first journal engine and tests.
- `source/services/statefsd/` contains the `/state` authority service for OS-lite.
- There is a low-level virtio-blk scaffold under `source/drivers/storage/virtio-blk/`, but OS/QEMU integration
  depends on the MMIO access model from `TASK-0010` being complete enough for virtio-blk.
- Today’s OS-lite `vfsd` is a read-only proxy for `pkg:/` (no writable mounts); `/state` must be a dedicated
  service authority in v1 (mounting into VFS is a follow-up).

## Goal

Prove (host + QEMU) that:

- state can be written, synced, and recovered after a “restart cycle” (soft reboot simulation),
- `bootctl` and keystore device key material are stored under `/state` and reloaded on the next cycle.

To satisfy follow-up tasks, v1 must also provide:

- **Path-as-key semantics**: keys are UTF-8, normalized, and may contain `/` for hierarchical prefixes
  (e.g. `/state/apps/<appId>/...`, `/state/crash/...`, `/state/boot/...`).
- **Bounded values**: enforce a maximum value size (v1: choose a fixed cap and prove rejection).
- **Atomic replace**: `put(key, bytes)` replaces the full value atomically; no partial writes.
- **List by prefix**: `list(prefix)` returns deterministic ordering, bounded count, and supports prefix queries.
- **Explicit durability**: `sync()` is the only durability boundary; proofs must show “write → sync → restart → read”.

## Expected `/state` layout (v1)

This task defines `/state` as a **logical namespace** served by `statefsd` (not a POSIX filesystem mount in v1).
Keys are treated as normalized UTF-8 paths.

Required prefixes (v1):

- **Updates / boot control**:
  - `/state/boot/bootctl.*` (owned by updates/boot control service; exact keys defined by follow-up tasks)
- **Device identity / keystore persistence**:
  - `/state/keystore/*` (restricted to `keystored` via policy; no other service may read/write these keys)
- **Crash artifacts**:
  - `/state/crash/*` (written by crash pipeline per `TASK-0018`; readable by authorized export/tools only)
- **Apps/registry (future but v1-compatible)**:
  - `/state/apps/<appId>/...` (used by `TASK-0130`; v1 must support atomic replace + sync for these keys)

Notes:

- v1 does not require directory semantics; prefixes exist to support bounded `list(prefix)` and policy rules.
- Follow-up tasks own the exact key naming under their prefixes, but must remain within the v1 bounds (size/count/replay).

## Non-Goals

- Full POSIX filesystem semantics (directories, partial writes, mmap, permissions).
- Full partition discovery.
- True VM reset / bootloader integration for reboot persistence proof (follow-up).
- Snapshots, quotas, and strict cross-service error semantics (follow-ups `TASK-0132`/`TASK-0133`/`TASK-0134`).

## Constraints / invariants (hard requirements)

- **Kernel changes (required prerequisite)**: userspace virtio-blk on QEMU `virt` requires safe MMIO access.
  This is kernel work in `TASK-0010`. This task must not add new kernel surface; it only consumes the MMIO
  mapping primitive once it exists and is policy/capability gated.
- **Bounded parsing + bounded replay**: journal replay must be bounded and reject malformed records deterministically.
- **Integrity**: journal records include checksums (CRC32-C for v1 integrity; authenticity handled elsewhere).
- **Determinism**: markers stable; tests bounded; no unbounded scanning of a “disk”.
- **No fake success**: “persist ok” markers only after re-open/replay proves the data is present.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.
- **Policy + audit**:
  - all `statefsd` operations are deny-by-default via `policyd`, binding to `sender_service_id`;
  - access decisions are audit-logged via `logd` (no UART scraping as “truth”).

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - **Userspace MMIO access for virtio-blk**: on QEMU `virt`, virtio-blk is a MMIO device. As of today,
    userspace cannot arbitrarily map MMIO; `SYSCALL_AS_MAP` maps VMOs, not physical ranges. Unless we
    already have an MMIO-cap/VMO/broker path, a true userspace virtio-blk frontend cannot ship with
    “kernel untouched”. In that case we must:
    - create **TASK-0010** (device MMIO access model / safe mapping capability), or
    - explicitly relax the constraint for this task (kernel exposes a safe device mapping capability).

  Decision (to avoid drift):

  - **We will pull `TASK-0010` before `TASK-0009` (required)**:
    - **What it means**: `TASK-0010` delivers the MMIO mapping primitive + capability distribution model first.
      Then `TASK-0009` implements virtio-blk userspace frontend + `statefsd` on top.
    - **Why it’s drift-resistant**: v9’s OS/QEMU proofs (`blk: virtio-blk up`, persistence markers) are only
      possible when the kernel contract exists. Keeping the kernel work explicitly in v10 prevents “hidden”
      kernel edits in v9 and keeps responsibility boundaries clear.
    - **Proof posture**: v10 can prove *security invariants* (cap-gated, W^X, bounds, deny-by-default distribution)
      independent of storage semantics; v9 proves *durability semantics* on top (sync/restart/replay).
    - **Risk**: schedule coupling — v9 is blocked until v10 is done enough for virtio-blk.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **VFS integration drift**: current `vfsd` os-lite is a read-only proxy for `pkg:/` and does not support
    mount or generic write paths. Trying to “mount /state into vfsd” in v1 will balloon scope.
    Best-for-OS v1 is: `statefsd` is the `/state` authority with a dedicated client API; VFS mounting is a follow-up.
  - **Soft reboot definition**: with kernel untouched we likely cannot perform a real reboot. We must define
    an honest proof cycle (restart `statefsd` and re-open the block backend; restart the consuming service; or a new init cycle hook).
    - **Drift-free v1 definition**:
      - “restart cycle” = `statefsd` process restart + re-open block backend + replay, followed by a read/verify by a client
      - “reboot” / VM reset / bootloader persistence remains **out of scope** for v1 (see Non-Goals) and is a dedicated follow-up proof.
    - **Kernel hook note**:
      - If we choose to add a minimal kernel feature to make the restart cycle cleaner (e.g. a deterministic process restart primitive),
        that change belongs to `TASK-0010` (device/MMIO model track) or a dedicated kernel task, not silently inside v9.
  - **Device key generation entropy**: persistence does not solve entropy. This was previously a bring-up risk,
    but real entropy for OS builds is now provided by `TASK-0008B` (virtio-rng → `rngd` authority → `keystored` keygen).
    v9 should treat entropy as solved and focus on **storage confidentiality boundaries** (policy-gated `/state/keystore/*`, no secret logging).
- **GREEN (confirmed assumptions)**:
  - We already have a stub virtio-blk crate (`source/drivers/storage/virtio-blk`) that can be reused as low-level scaffolding once access exists.

## Security considerations

### Threat model

- **Credential theft from /state**: Attacker reads device keys, bootctl secrets from storage
- **Data tampering**: Attacker modifies stored credentials or boot configuration
- **Journal corruption**: Attacker corrupts journal to cause data loss or boot failure
- **Replay attack on journal**: Attacker replays old journal entries to restore revoked keys
- **Unauthorized access to /state**: Service without proper capability accesses stored secrets
- **Physical attack on storage**: Attacker with physical access reads unencrypted storage

### Security invariants (MUST hold)

- Device keys and sensitive credentials MUST only be accessible to authorized services
- Journal records MUST include integrity checksums (CRC32 minimum, HMAC for authenticity future)
- Journal replay MUST reject corrupted or tampered records deterministically
- `statefsd` access MUST be capability-gated (no ambient /state access)
- Persistent data MUST be integrity-protected against bit-flips and corruption
- Key paths (`/state/keystore/*`) MUST be restricted to keystored only

### DON'T DO

- DON'T store secrets in plaintext without integrity protection
- DON'T allow arbitrary services to read `/state/keystore/*` paths
- DON'T accept journal records that fail integrity checks
- DON'T assume storage is reliable (always verify checksums on read)
- DON'T allow rollback of journal to restore revoked/rotated keys (future: monotonic counter)
- DON'T skip capability checks for "trusted" services

### Attack surface impact

- **Significant**: `/state` contains device keys and boot configuration (high-value targets)
- **Persistence risk**: Compromised keys persist across reboots
- **Physical access risk**: Unencrypted storage vulnerable to extraction

### Mitigations

- CRC32 checksums on all journal records (integrity)
- Capability-gated access to statefsd endpoints
- Key paths restricted by sender_service_id (keystored only for `/state/keystore/*`)
- Bounded journal replay: reject malformed records, limit replay depth
- Future: at-rest encryption for sensitive paths, HMAC for authenticity

## Security proof

### Audit tests (negative cases)

- Command(s):
  - `cargo test -p statefs -- reject --nocapture` (crate to be introduced by this task; name may split host/os)
- Required tests:
  - `test_reject_corrupted_journal` — CRC mismatch → replay stops deterministically at last valid record
  - `test_reject_unauthorized_keystore_access` — wrong service → denied
  - `test_reject_malformed_record` — invalid format → rejected
  - `test_bounded_replay` — replay depth limited
  - `test_reject_value_oversized` — enforce v1 value size cap deterministically
  - `test_truncated_tail_stops_replay` — truncated final record stops replay deterministically
  - `test_partial_record_boundary_replay` — record spans >2 blocks; replay must not truncate/stop early

### Hardening markers (QEMU)

- `statefsd: access denied (path=<p> sender=<svc>)` — capability enforcement
- `statefsd: crc mismatch (record=<n>)` — integrity verification works
- `SELFTEST: statefs unauthorized access rejected` — access control verified

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **Existing VFS model (today)**: `source/services/vfsd/src/os_lite.rs` (read-only, pkg:/ proxy)

## Stop conditions (Definition of Done)

### Proof (Host)

- Add deterministic tests for the statefs journal engine and replay:
  - `cargo test -p statefs -- --nocapture` (or `-p statefs-host` if split is needed)
  - Coverage:
    - Put/Get/Delete/List
    - crash/replay: write records, drop instance, reopen and replay → data intact
    - CRC mismatch rejection (deterministic stop at last valid record)
    - truncated tail stops replay deterministically
    - partial record at block boundary does not cause early EOF
    - size limits / path normalization / ENOENT

### Proof (OS / QEMU)

- **Unblocked**: `TASK-0010` (MMIO access model) is complete enough for virtio-blk MMIO mapping.
  This task’s OS/QEMU proof is now valid and enforced by the canonical harness.

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
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
   - Only proceed once `TASK-0010` has delivered the virtio-blk MMIO capability + mapping primitive and init can
     distribute it safely to the block authority service.
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
- Kernel changes required for MMIO mapping live in `TASK-0010`; `TASK-0009` adds no kernel changes.

## Follow-ups (explicitly out of v1 scope; require separate tasks)

These items are valuable for the OS vision (security + maintainability + extensibility) but are
out of scope for v1 and MUST NOT be smuggled into this task:

- **Authenticity & anti-rollback**: HMAC/AEAD for journal authenticity + monotonic counters to prevent rollback.
- **Encryption-at-rest**: key custody + sealing policy (builds on `TASK-0027`).
- **VFS mount integration**: mount `/state` into VFS with writable semantics (see `TASK-0134`).
- **Compaction/snapshots/quotas**: multi-generation journal management and accounting (`TASK-0133`/`TASK-0134`).
- **Offline tooling**: fsck-like verification/repair, export/import, and telemetry summaries.
- **IPC framing hardening**: request IDs / conversation IDs in statefs frames to avoid shared-inbox ambiguities.

## RFC seeds (for later, once green)

- Decisions made:
  - device access model for userspace block devices (MMIO caps/broker)
  - statefs journal format + compaction policy
  - “soft reboot” definition and transition to real reboot proof
