# Next Task Preparation (Drift-Free)

## Candidate next execution

- **task**: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` — `Draft`
- **immediate follow-up**: `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md` — `Draft`
- **production dependencies**: `TASK-0286`, `TASK-0287`, `TASK-0290`
- **tier**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate C relevant closure obligations)

## Drift check vs real repo state

- [x] `source/services/packagefsd/src/std_server.rs` is still host in-memory registry (no pkgimg contract yet).
- [x] `source/services/packagefsd/src/os_lite.rs` currently decodes `bundleimg` from `bundlemgrd.fetch_image`.
- [x] `TASK-0032` now carries explicit security section + named reject proofs.
- [x] `TASK-0032` header follow-up list explicitly includes `TASK-0033` + production dependencies (`TASK-0286/0287/0290`).
- [x] Gate-C production-grade mapping is explicit in task body; no hidden scope absorption.

## Acceptance criteria (must be testable per cut)

### Host (mandatory)

- Deterministic pkgimg builder/verifier tests prove stable image layout + reproducible index hash.
- Reject-path tests exist and pass (`test_reject_*` malformed/corrupt/path/OOB/cap exceeded).
- `stat/open/read` behavior is proven against contract fixtures (Soll), not implementation trivia.

### OS / QEMU (mandatory for closure claims)

- Deterministic marker ladder proves package image mount/read closure when OS path is claimed:
  - `packagefsd: v2 mounted (pkgimg)`
  - `SELFTEST: pkgimg mount ok`
  - `SELFTEST: pkgimg stat/read ok`
- No fake success markers for degraded/stub paths.
- Marker contract is registered and enforced by canonical harness (`scripts/qemu-test.sh` + `verify-uart` path).

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

- Prep is complete when `TASK-0032` contract text is synchronized with current repo reality, security/reject proofs are explicit, and Gate-C production dependency boundaries are explicit.
