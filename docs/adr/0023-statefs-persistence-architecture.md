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

## Invariants
- No secrets are logged or emitted in error messages.
- Access decisions are based on kernel `sender_service_id`.
- Journal replay is bounded and deterministic.
- CRC32 integrity checks are enforced on every record.
- `statefsd` never emits `ok/ready` markers unless behavior is real.

## Implementation Plan
1. Implement host-first journal engine and BlockDevice abstraction.
2. Implement `statefsd` service with IPC endpoints and policy checks.
3. Integrate virtio-blk backend for OS mode.
4. Migrate keystored/updated to `/state` keys and add persistence proofs.

## References
- `userspace/statefs/src/lib.rs`
- `source/services/statefsd/` (planned)
- `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
