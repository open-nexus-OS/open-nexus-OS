# Next Task Preparation (Drift-Free)

## Candidate next execution

- **recently closed**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Done`
- **task**: `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md` — `Draft`
- **production dependencies**: `TASK-0286`, `TASK-0287`, `TASK-0290`
- **tier**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate C relevant closure obligations)

## Drift check vs real repo state

- [x] `source/services/packagefsd/src/std_server.rs` includes validated pkgimg v2 mount path.
- [x] `source/services/packagefsd/src/os_lite.rs` validates pkgimg v2 on `bundlemgrd.fetch_image` authority path.
- [x] `userspace/storage/src/pkgimg.rs` provides bounded parser/reject contract with required `test_reject_*`.
- [x] `tools/pkgimg-build` exists with build + verify binaries.
- [x] TASK/RFC/README status lines are synchronized for closure (`TASK-0032` Done, `RFC-0041` Complete).

## Acceptance criteria (for next cut: TASK-0033)

### Host (mandatory)

- VMO splice/read-range semantics must be proven on top of already-validated pkgimg v2 index contract.
- Reject-path tests must exist for OOB splice and hash mismatch.

### OS / QEMU (mandatory for closure claims)

- Required marker ladder (when claimed):
  - `packagefsd: splice→vmo ok (len=<n>)`
  - `SELFTEST: pkgimg vmo ok`
- Existing TASK-0032 pkgimg markers remain green (no regressions).

## Security checklist (mandatory)

- [x] Threat model is explicit (corrupt image, path traversal, OOB index ranges, authority drift).
- [x] Invariants are explicit (fail-closed mount, bounded parser, read-only semantics, channel-authoritative identity).
- [x] Negative-path proof requirement is explicit with named `test_reject_*` suite.
- [x] No payload-derived identity/policy trust is allowed in packagefs paths.
- [x] Production-grade dependency split is explicit (`TASK-0033`, `TASK-0286`, `TASK-0287`, `TASK-0290`).

## Out-of-scope handoff (normative)

This prep stays honest about scope:

1. `TASK-0032` owns deterministic pkgimg format + bounded mount/read fastpath closure.
2. `TASK-0033` owns VMO splice/zero-copy data path from package image.
3. `TASK-0286/0287/0290` remain explicit production-grade dependencies for full Gate-C release claims.

## Linked contracts

- `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md`
- `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md`
- `tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md`
- `tasks/TASK-0287-kernel-memory-pressure-v1-hard-limits-oom-handoff.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/architecture/12-storage-vfs-packagefs.md`
- `docs/standards/SECURITY_STANDARDS.md`
- `docs/standards/RUST_STANDARDS.md`

## Done condition (current prep)

- Prep is complete when `TASK-0033` scope starts from frozen `TASK-0032` closure baseline without reopening package image contract work.
