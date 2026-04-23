# RFC-0041: PackageFS v2 read-only image + precomputed index fastpath (host-first, OS-gated)

- Status: Done
- Owners: @runtime @storage @security
- Created: 2026-04-23
- Last Updated: 2026-04-23
- Links:
  - Tasks: `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
  - Related RFCs: `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`

## Status at a Glance

- **Phase 0 (pkgimg v2 contract + host parser/verifier floor)**: ✅
- **Phase 1 (packagefsd v2 mount/read path, host-first + OS-gated)**: ✅
- **Phase 2 (Gate-C closure handoff boundaries explicit and proven)**: ✅

Definition:

- "Done" means this RFC's contract is implemented for v2 image/index mount-read fastpath and the listed proof gates are green. Kernel/zero-copy production closure remains explicit follow-up scope.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - Canonical v2 package image (`pkgimg`) contract for read-only package content.
  - Deterministic precomputed index contract and mount-time integrity validation behavior.
  - Bounded parser/read invariants and deterministic reject behavior for malformed/corrupt image input.
  - Host-first + OS-gated mount/read proof contract for `packagefsd`.
- **This RFC does NOT own**:
  - VMO splice/data-plane zero-copy path (`TASK-0033`).
  - Kernel production-grade resource/accounting and zero-copy closure obligations (`TASK-0286`, `TASK-0287`, `TASK-0290`).
  - Full blk-authority redesign for OS mount source.

### Relationship to tasks (single execution truth)

- `TASK-0032` is the execution SSOT for this RFC.
- Task stop conditions and proof commands are authoritative for closure.
- Follow-up contracts must stay explicit in task header/body and must not be silently absorbed.

## Context

`packagefsd` currently has a split implementation baseline:

- host path (`std_server.rs`) supports optional `pkgimg` mount from `PACKAGEFSD_PKGIMG_PATH`,
- os-lite path (`os_lite.rs`) fetches from `bundlemgrd` and validates `pkgimg` before mount success.

That baseline works for bring-up but does not yet define a stable, versioned production contract for deterministic package image mount/read behavior at scale.

## Goals

- Define a versioned, deterministic `pkgimg` v2 format with explicit superblock/index/data boundaries.
- Guarantee deterministic, bounded, fail-closed mount validation and lookup behavior.
- Keep `stat/open/read` fastpath stable with O(1)-class lookup via precomputed index.
- Keep host-first proofs primary, with OS/QEMU markers as gated closure evidence.

## Non-Goals

- Writable packagefs behavior.
- Cross-process VMO splice data path (owned by `TASK-0033`).
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Determinism**: image layout, index ordering, hash checks, and markers are deterministic.
- **No fake success**: `packagefsd: v2 mounted (pkgimg)` only after full validation + index load.
- **Bounded resources**: explicit caps for image/index size, entry count, path length, and read bounds.
- **Security floor**:
  - mount fails closed on malformed/corrupt/out-of-range image data,
  - path traversal (`..`, empty segments) is rejected,
  - identity/policy decisions remain channel-authoritative (no payload identity trust).
- **Stubs policy**: any transitional fallback must be explicit and must not emit success markers for unvalidated behavior.

## Proposed design

### Contract / interface (normative)

- `pkgimg` v2 has:
  - deterministic superblock (magic, version, offsets/lengths),
  - deterministic index payload (stable-sorted bundle/path keys),
  - `sha256(index_bytes)` stored in superblock and verified at mount.
- `packagefsd` mount contract:
  - validate header/version/bounds/index hash first,
  - reject mount on first validation failure with deterministic error path,
  - expose read-only resolve/read semantics after successful mount only.
- Versioning:
  - unknown `pkgimg` version fails closed,
  - future versions require explicit new contract update.

### Phases / milestones (contract-level)

- **Phase 0**: `pkgimg` v2 format + deterministic parser/verifier reject contract.
- **Phase 1**: `packagefsd` v2 mount/read path (host-first primary proof, OS-gated marker proof).
- **Phase 2**: explicit Gate-C handoff boundaries to `TASK-0033` and `TASK-0286/0287/0290` synchronized in task/docs.

## Security considerations

- **Threat model**:
  - corrupted image bytes,
  - forged/truncated index data,
  - out-of-range entry offsets/lengths,
  - path traversal attempts,
  - authority drift via payload-derived identity assumptions.
- **Mitigations**:
  - strict version/magic validation,
  - bounded parser with hard caps,
  - index hash verification before mount success,
  - deterministic path sanitization rules,
  - fail-closed mount/read behavior on validation failure.
- **Open risks**:
  - blk-backed authority path remains a follow-up boundary and must not be silently mixed into this scope.
  - zero-copy performance claims stay out-of-scope until `TASK-0033` and kernel closure tasks are complete.

## Failure model (normative)

- Invalid superblock/version/hash/bounds/path returns deterministic reject behavior and mount failure.
- No silent fallback to unvalidated registry/image data.
- Unsupported versions fail closed.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p storage && cargo test -p packagefsd && cargo test -p pkgimg-build
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (if applicable)

- `packagefsd: v2 mounted (pkgimg)`
- `SELFTEST: pkgimg mount ok`
- `SELFTEST: pkgimg stat/read ok`

## Alternatives considered

- Keep in-memory-only host registry as long-term model (rejected: not scalable, no stable image contract).
- Add blk-backed mount authority in this RFC (rejected: scope creep; authority/proof closure belongs to follow-up work).

## Open questions

- None for this RFC scope; per-file hash requirement is deferred explicitly to follow-up contract work if promoted from optional to required semantics.

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: define `pkgimg` v2 format + bounded parser/reject contract — proof: `cd /home/jenning/open-nexus-OS && cargo test -p storage && cargo test -p packagefsd && cargo test -p pkgimg-build`
- [x] **Phase 1**: `packagefsd` v2 mount/read path with deterministic mount markers — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- [x] **Phase 2**: Gate-C follow-up boundaries (`TASK-0033`, `TASK-0286/0287/0290`) explicit in task/docs — proof: `cd /home/jenning/open-nexus-OS && rg "TASK-0033|TASK-0286|TASK-0287|TASK-0290" tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
