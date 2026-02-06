# RFC-0018: StateFS Journal Format v1

- Status: Complete (v1 contract implemented; host + QEMU proofs green)
- Owners: @runtime
- Created: 2026-02-02
- Last Updated: 2026-02-06
- Links:
  - Tasks: `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` (execution + proof)
  - ADRs: `docs/adr/0023-statefs-persistence-architecture.md`
  - Related RFCs: `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (MMIO access for virtio-blk)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (capability-gated access)
  - Related RFCs: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy enforcement)
  - Engineering: `docs/dev/platform/qemu-virtio-mmio-modern.md` (force modern virtio-mmio in QEMU)

## Status at a Glance

- **Phase 0 (Journal Core)**: ✅ Host-only journal engine + BlockDevice trait (host tests green)
- **Phase 1 (statefsd Service)**: ✅ IPC endpoints + policy-gated access
- **Phase 2 (OS Integration)**: ✅ QEMU proof is deterministic under modern virtio-mmio (`blk: virtio-blk up`, persistence markers)

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - Journal record format (on-disk binary layout)
  - CRC32 integrity checksums
  - Replay semantics and bounded replay
  - statefsd IPC API (Put/Get/Delete/List/Sync)
  - BlockDevice trait abstraction
  - Value size limits and path normalization
- **This RFC does NOT own**:
  - VFS mount integration (follow-up: `TASK-0134`)
  - Snapshots, compaction, quotas (follow-ups)
  - Encryption-at-rest (follow-up: `TASK-0027`)
  - Real VM reboot/bootloader integration (follow-up)
  - Journal authenticity / anti-rollback (follow-ups; see Security considerations)
  - Offline repair / fsck tooling (follow-up)
  - RPC framing improvements (request IDs / conversation IDs) beyond v1 byte frames.
    - Note (2026-02-05): request/reply correlation is now specified in
      `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` because deterministic QEMU proofs (and future OS
      correctness) require nonce-based request/reply matching rather than ad-hoc drains/yields on shared inboxes.
  - DMA or IRQ delivery (separate RFCs)

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC must link to the task(s) that implement and prove each phase/milestone.

## Context

Open Nexus OS currently has no durable userspace persistence. Several upcoming subsystems require it:

- Updates A/B (`bootctl`) must survive reboot/boot cycles
- Keystore device identity keys must be stored under `/state`
- Future logging wants a persistent journal

TASK-0010 (Device MMIO Access Model) is now complete, enabling userspace virtio-blk access.
This RFC defines the journal format and statefs service API for the `/state` namespace.

## Goals

- Define a simple, robust journaled key-value store format for `/state`
- Provide bounded replay with CRC32 integrity checks
- Support path-as-key semantics for hierarchical prefixes
- Enable capability-gated, policy-enforced access via statefsd
- Prove persistence via soft reboot (statefsd restart + replay)

## Non-Goals

- Full POSIX filesystem semantics (directories, partial writes, mmap, permissions)
- Full partition discovery
- True VM reset / bootloader integration (v1 = soft reboot only)
- Snapshots, quotas, compaction (follow-ups)
- Encryption-at-rest (follow-up)

## Constraints / invariants (hard requirements)

- **Determinism**: markers/proofs are deterministic; no timing-fluke "usually ok".
- **No fake success**: never emit "ok/ready" markers unless the real behavior occurred.
- **Bounded resources**: explicit limits for value size (v1: 64 KiB), key count, replay depth.
- **Security floor**: CRC32 integrity on all records; capability-gated access; policy deny-by-default.
- **Stubs policy**: any stub must be explicitly labeled, non-authoritative, and must not claim success.
- **No kernel changes**: v1 consumes MMIO mapping primitive from TASK-0010; kernel remains unchanged.

## Proposed design

### Contract / interface (normative)

#### BlockDevice Trait

```rust
/// Abstract block device for storage backend.
pub trait BlockDevice {
    /// Block size in bytes (typically 512).
    fn block_size(&self) -> usize;

    /// Total number of blocks.
    fn block_count(&self) -> u64;

    /// Read a single block into buffer.
    /// Returns Err on I/O failure.
    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// Write a single block from buffer.
    /// Returns Err on I/O failure.
    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError>;

    /// Flush all pending writes to durable storage.
    fn sync(&mut self) -> Result<(), BlockError>;
}
```

#### Journal Record Format (on-disk)

Each journal record is a variable-length entry:

``` text
+--------+--------+--------+----------+-------+-------+--------+
| Magic  | OpCode | KeyLen | ValueLen | Key   | Value | CRC32  |
| 4 bytes| 1 byte | 2 bytes| 4 bytes  | N     | M     | 4 bytes|
+--------+--------+--------+----------+-------+-------+--------+
```

- **Magic**: `0x4E585346` ("NXSF" = Nexus StateFS)
- **OpCode**: `0x01` = Put, `0x02` = Delete, `0x03` = Checkpoint
- **KeyLen**: Little-endian u16 (max 255 bytes)
- **ValueLen**: Little-endian u32 (max 64 KiB for v1)
- **Key**: UTF-8 normalized path (e.g., `/state/keystore/device.key`)
- **Value**: Raw bytes (opaque to journal)
- **CRC32**: CRC32-C (Castagnoli) over Magic..Value (everything except the CRC itself), little-endian u32

Fixed overhead: 15 bytes total (11 bytes header + 4 bytes CRC32).

#### Replay Semantics

1. Scan journal from start, reading records sequentially
2. For each record:
   - Verify CRC32 matches; on mismatch treat as corruption (stop replay at last valid record)
   - Apply operation to in-memory key-value map
   - Put: insert/replace key with value
   - Delete: remove key
   - Checkpoint: mark replay progress (for future compaction)
3. Replay depth bounded: stop after `MAX_REPLAY_RECORDS` (v1: 100,000)
4. On corruption/truncation: stop replay at last valid record

#### statefsd IPC API

Operations are versioned byte frames over kernel IPC:

| OpCode | Name   | Request                     | Response                    |
|--------|--------|-----------------------------|-----------------------------|
| 0x01   | Put    | key_len + key + val_len + val | status (0=ok, nonzero=err) |
| 0x02   | Get    | key_len + key               | status + val_len + val      |
| 0x03   | Delete | key_len + key               | status                      |
| 0x04   | List   | prefix_len + prefix + limit | status + count + keys       |
| 0x05   | Sync   | (empty)                     | status                      |
| 0x06   | Reopen | (empty)                     | status                      |

- All operations are capability-gated via kernel `sender_service_id`
- Policy checks via policyd: `/state/keystore/*` restricted to keystored only
- Error codes: 0 = OK, 1 = NOT_FOUND, 2 = ACCESS_DENIED, 3 = VALUE_TOO_LARGE, 4 = KEY_TOO_LONG, 5 = IO_ERROR
- `Reopen` is a test-only op: it triggers a journal replay from the current device to simulate a restart cycle

#### Value Size Limits (v1)

- Maximum key length: 255 bytes
- Maximum value size: 64 KiB (65,536 bytes)
- Reject operations exceeding limits with appropriate error code

#### Path Normalization

Keys are UTF-8 strings treated as paths:
- Must start with `/state/`
- No `..` or `.` path components
- No trailing slashes (normalized away)
- Case-sensitive

### Phases / milestones (contract-level)

- **Phase 0 (Journal Core)**: Host-only journal engine + BlockDevice trait + MemBlockDevice for tests
  - Proof: `cargo test -p statefs` with Put/Get/Delete/List/Sync + reject tests
- **Phase 1 (statefsd Service)**: IPC endpoints + policy-gated access + `statefsd: ready` marker
  - Proof: QEMU marker `statefsd: ready`
- **Phase 2 (OS Integration)**: virtio-blk backend + persistence proof (soft reboot)
  - Proof: QEMU markers `blk: virtio-blk up`, `SELFTEST: statefs persist ok`

## Security considerations

### Threat model

- **Credential theft from /state**: Attacker reads device keys from storage
- **Data tampering**: Attacker modifies stored credentials or boot configuration
- **Journal corruption**: Attacker corrupts journal to cause data loss or boot failure
- **Replay attack on journal**: Attacker replays old journal entries to restore revoked keys
- **Unauthorized access to /state**: Service without proper capability accesses stored secrets
- **Physical extraction**: attacker reads unencrypted storage media

### Security invariants (MUST hold)

- Device keys and sensitive credentials MUST only be accessible to authorized services
- Journal records MUST include CRC32-C integrity checksums
- Journal replay MUST reject corrupted or tampered records deterministically (no heuristic recovery in v1)
- statefsd access MUST be capability-gated via kernel `sender_service_id`
- Key paths (`/state/keystore/*`) MUST be restricted to keystored only via policy
- No secrets in logs or error messages

### DON'T DO

- DON'T allow arbitrary services to read `/state/keystore/*` paths
- DON'T accept journal records that fail CRC32 checks
- DON'T assume storage is reliable (always verify checksums on read)
- DON'T skip capability checks for "trusted" services
- DON'T log key values or sensitive data
- DON'T treat CRC32 as authenticity (it is integrity only)

### Mitigations

- CRC32-C checksums on all journal records (integrity)
- Capability-gated access to statefsd endpoints
- Key paths restricted by `sender_service_id` (keystored only for `/state/keystore/*`)
- Bounded journal replay: reject malformed records, limit replay depth
- Future: at-rest encryption for sensitive paths; authentication/anti-rollback (HMAC/AEAD + monotonic counter)

## Failure model (normative)

| Condition | Behavior |
|-----------|----------|
| CRC32 mismatch | Stop replay at last valid record (explicit corruption) |
| Truncated record | Stop replay at last valid record |
| Key too long (>255) | Return KEY_TOO_LONG error |
| Value too large (>64K) | Return VALUE_TOO_LARGE error |
| Key not found (Get) | Return NOT_FOUND error |
| Unauthorized access | Return ACCESS_DENIED, audit log |
| Block I/O failure | Return IO_ERROR, do not corrupt state |

No silent fallback: all errors are explicit and deterministic.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p statefs -- --nocapture
```

Required test coverage:
- `test_put_get_delete_list` — basic operations
- `test_replay_after_reopen` — crash/replay integrity
- `test_reject_corrupted_journal` — CRC mismatch → replay stops deterministically
- `test_reject_value_oversized` — size limit enforcement
- `test_reject_key_too_long` — key length limit
- `test_bounded_replay` — replay depth limit
- `test_truncated_tail_stops_replay` — truncated final record stops replay at last valid record
- `test_partial_record_boundary_replay` — record spans >2 blocks; replay must not truncate/stop early

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Note: Current QEMU needs modern virtio-mmio for virtio-blk to progress the used ring; see
`docs/dev/platform/qemu-virtio-mmio-modern.md`.

Note (determinism): OS/QEMU proofs that depend on cross-service IPC (audit sink / crash reports / statefs control plane)
must avoid shared-inbox ambiguity. We standardize request/reply correlation via nonces in
`docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` so statefs persistence proofs remain deterministic as the OS grows.

### Deterministic markers

- `blk: virtio-blk up (ss=<sector_size> nsec=<num_sectors>)` — block device ready
- `statefsd: ready` — statefs service endpoints live
- `SELFTEST: statefs put ok` — put operation succeeded
- `SELFTEST: statefs persist ok` — data survived restart cycle
- `SELFTEST: bootctl persist ok` — bootctl persisted via statefs
- `SELFTEST: device key persist ok` — device key persisted via statefs
- `statefsd: access denied (path=<p> sender=<svc>)` — policy enforcement audit

## Alternatives considered

- **SQLite/embedded DB**: Too heavy for no_std, adds complexity
- **Raw file I/O**: No journal = no crash recovery
- **Log-structured merge tree**: Overkill for v1 key count

## Open questions

- (None for v1; compaction/quotas/authenticity deferred to follow-ups)

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Journal core (host-first) — proof: `cargo test -p statefs`
  - [x] BlockDevice trait + MemBlockDevice
  - [x] Journal record format + CRC32 (implementation must align to CRC32-C contract)
  - [x] Put/Get/Delete/List/Sync operations
  - [x] Replay engine with bounded depth
  - [x] Negative tests (`test_reject_*`)
- [x] **Phase 1**: statefsd service — proof: QEMU `statefsd: ready`
  - [x] IPC endpoints over kernel IPC
  - [x] Policy-gated access via policyd
  - [x] Audit logging for access decisions
- [x] **Phase 2**: OS integration — proof: QEMU persistence markers
  - [x] virtio-blk backend for statefsd
  - [x] keystored migration to `/state/keystore/*`
  - [x] updated migration to `/state/boot/*`
  - [x] QEMU virtio-mmio modern mode wired into canonical harness (force-legacy=off)
  - [x] Soft reboot proof (restart + replay + verify) wired into CI/QEMU phases
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
