# ADR-0023: StateFS Persistence Architecture

Status: Accepted
Date: 2026-02-02
Owners: @runtime

## Context
Open Nexus OS lacks durable userspace persistence. Several subsystems (updates, keystore, crashdumps)
require data that survives process restarts and soft reboot cycles. The existing `vfsd` is read-only
and not suitable for writable `/state` semantics in v1.

## Decision
Introduce a dedicated `statefsd` service that owns the `/state` namespace and exposes a journaled
key-value API (Put/Get/Delete/List/Sync) over kernel IPC. Persistence is provided via a block-device
backend (virtio-blk on QEMU), and a journal format with CRC32 integrity checks.

Key architectural decisions:

- **Authority**: `statefsd` is the sole authority for `/state` in v1 (no VFS mount integration).
- **Storage format**: append-only journal with CRC32 integrity; bounded replay for determinism.
- **Access control**: capability-gated IPC + policyd deny-by-default rules.
- **Soft reboot**: persistence proof uses `statefsd` restart + replay, not full VM reset.

## Rationale
- Avoids scope creep into VFS mount semantics while enabling persistence quickly.
- Journaled KV store is simpler and more deterministic than a full filesystem.
- Capability-gated access enforces least privilege and auditability.
- Soft reboot proof is achievable without kernel changes.

## Consequences
- Clients must use a `statefs` client API instead of POSIX I/O.
- `statefsd` becomes a high-value security boundary for secrets.
- `/state` semantics are limited to key-value operations in v1.
- Follow-up tasks will be needed for mounts, compaction, quotas, and encryption-at-rest.

### Delivered follow-ups (2026-08-18) — v1b/v2a/v2b shipped

The v1 substrate described above is unchanged; three follow-ups closed the gaps this ADR
predicted (full byte contracts: `docs/storage/statefs.md`):

- **TASK-0025 (v1b, authenticity):** CRC32 detects corruption, not tampering — values under
  enrolled prefixes now carry `NXEV` envelopes (HMAC-SHA256 keyed via label-scoped HKDF over
  a device-key signature) with a replay-fed anti-rollback seq tracker and a write budget.
  New wire statuses 9 (integrity) / 10 (rollback).
- **TASK-0026 (v2a, atomicity + compaction + fsck):** journal v2 adds
  `PREPARE/PAYLOAD/COMMIT/ABORT` with committed-only replay (multi-key both-or-neither) and
  bounded compaction (`CHECKPOINT` snapshot into the inactive half of an A/B region split,
  atomic `NXS2` superblock flip). Replay cost is now proportional to the live journal instead
  of lifetime writes, which **defused a boot time bomb**: `MAX_REPLAY_RECORDS` was a hard
  open-failure of `/state`, not a throttle. Offline tooling: `fsck-statefs` (stable exit
  codes, append-only repair). Cold-boot durability is proven against a preserved image
  (`NEXUS_KEEP_BLK=1`), no longer only soft-reboot replay.
- **TASK-0027 (v2b, encryption at rest):** opt-in XChaCha20-Poly1305 sealing of value
  payloads for enrolled **non-boot-critical** prefixes, default off. Keys/paths stay
  plaintext; boot-critical prefixes (`/state/keystore/`, `/state/boot/`) are structurally
  unenrollable (they must replay before key material exists). Enablement is gated by a new
  `statefs.admin` capability and the `encryption on` marker only follows an in-process AEAD
  self-check.

Invariant additions from these tasks: transactions are all-or-nothing across restart at every
write boundary; compaction is bounded per cycle and its completion marker requires a device
readback verify; sealed values are never served as plaintext on any verification failure.

## Invariants
- No secrets are logged or emitted in error messages.
- Access decisions are based on kernel `sender_service_id`.
- Journal replay is bounded and deterministic.
- CRC32 integrity checks are enforced on every record.
- `statefsd` never emits `ok/ready` markers unless behavior is real.
- Crashdump artifacts (when present) must remain constrained to `/state/crash/...` and pass policy-bound authorization; no broad anonymous-write bypasses.

## Implementation Plan
1. Implement host-first journal engine and BlockDevice abstraction.
2. Implement `statefsd` service with IPC endpoints and policy checks.
3. Integrate virtio-blk backend for OS mode.
4. Migrate keystored/updated to `/state` keys and add persistence proofs.

## References
- `userspace/statefs/src/lib.rs`
- `source/services/statefsd/` (planned)
- `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
