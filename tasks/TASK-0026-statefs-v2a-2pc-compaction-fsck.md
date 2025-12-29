---
title: TASK-0026 StateFS v2a: 2PC crash-atomicity + bounded compaction + fsck tool (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on (statefs v1): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Related (write hardening): tasks/TASK-0025-statefs-write-path-hardening-integrity-atomic-budgets-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

StateFS v1 (TASK-0009) provides a minimal journaled KV store for `/state`.
We want the next step to be **strict crash atomicity** and **operational tooling**, without changing the kernel:

- two-phase commit (prepare → commit) with idempotent recovery,
- bounded compaction (snapshot/rotate) to keep replay time bounded,
- an offline `fsck-statefs` tool for replay/repair/compact in host workflows.

Repo reality: `statefs/statefsd` are still tasks, not shipped code, so this is **host-first** and **OS-gated**.

## Goal

Prove deterministically (host tests) that:

- only committed transactions become visible after replay,
- prepared-but-not-committed transactions are discarded,
- compaction produces a minimal snapshot and a clean journal,
- `fsck-statefs` can detect and (optionally) repair orphaned transactions.

Once statefs exists in OS (TASK-0009), add QEMU proof markers.

## Non-Goals

- Encryption-at-rest (separate task, see TASK-0027).
- Full filesystem semantics.
- Kernel changes.
 - Named snapshots and read-only snapshot mounts (follow-up `TASK-0134`).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- **Bounded replay**: journal replay must be bounded and deterministic; reject malformed records.
- **Bounded memory**: cap txn-in-flight buffers; cap max payload size; cap number of open txns.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake markers.

## Red flags / decision points

- **RED (gating)**:
  - Do not emit OS markers until TASK-0009 exists and replay actually happens in QEMU.
- **YELLOW (delete semantics)**:
  - Decide whether `DELETE` is immediate at replay time (simple) or transactional (more complex).
  - v2a chooses: `DELETE` is its own committed record (immediate during replay).

## Contract sources (single source of truth)

- StateFS v1 task: TASK-0009 (journal concepts and proof expectations)
- QEMU marker contract: `scripts/qemu-test.sh` (only once OS statefs exists)

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic tests (`tests/statefs_v2a_host/` or crate-local tests):

- happy path: `PREPARE + PAYLOAD + COMMIT` → visible after replay
- crash simulation: `PREPARE + partial PAYLOAD` (no COMMIT) → not visible after replay
- idempotence: replay same journal twice → same state
- compaction: reach threshold → snapshot+rotate; state intact
- fsck:
  - detect orphaned txns
  - `--repair` converts orphans to ABORT (or reports “discarded”)
  - exit codes stable (0 ok, 1 repaired, 2 unrecoverable)

### Proof (OS / QEMU) — after TASK-0009

Extend `scripts/qemu-test.sh` (order tolerant) with:

- `statefsd: journal v2 mounted (2PC)`
- `SELFTEST: statefs v2 crash-atomic ok`
- `SELFTEST: statefs v2 compact ok`

## Touched paths (allowlist)

- `source/services/statefsd/` (journal v2 + compaction; once exists)
- `userspace/statefs/` (client v2 helpers; once exists)
- `tools/fsck-statefs/` (new host tool)
- `tests/` (host tests)
- `source/apps/selftest-client/` (OS markers; gated)
- `docs/storage/statefs.md`
- `docs/testing/index.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. **Journal v2 record format + replay (2PC)**
   - Add records:
     - `PREPARE { txn_id, path, envelope_hash, payload_len, crc32 }`
     - `PAYLOAD { txn_id, chunk }` (bounded chunk size)
     - `COMMIT { txn_id }`
     - `ABORT { txn_id }`
     - `DELETE { path }`
     - `SYNC {}`
   - Replay rule: apply only txns with `COMMIT`; discard incomplete txns.
   - Backward compatibility: read v1 journal if present; write v2 after first compaction.

2. **Compaction (snapshot + rotate)**
   - Trigger threshold: ratio or bytes (configurable).
   - Snapshot contains the minimal current map.
   - Rotate to a new clean journal; bounded work per cycle.
   - Marker when done: `statefsd: compaction done (gen=<n>, entries=<m>)`.

3. **fsck-statefs tool (host)**
   - Replay and validate journals offline.
   - `--repair`: emit ABORT records for orphans and/or rewrite a compacted snapshot.
   - Deterministic output + exit codes.

4. **OS selftest (gated)**
   - Prove crash-atomicity via restart/reopen cycle and last-writer-wins.
   - Prove compaction marker appears after threshold.

## Follow-ups

- `TASK-0134`: named snapshots + read-only mounts + GC/compaction triggers (StateFS “v3” slice)

## Docs (English)

- `docs/storage/statefs.md`: journal v2 layout, 2PC semantics, compaction thresholds, fsck usage.
- `docs/testing/index.md`: how to run host tests; expected OS markers once enabled.
