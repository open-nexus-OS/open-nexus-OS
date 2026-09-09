---
title: TASK-0033 packagefs v2b: zero-copy `pkg:/` reads — VMO pass-through from bundlemgrd + ONE payload-VMO header codec
status: Draft (reopened 2026-09-09 — end-state rewrite; residual of the TASK-0295 supersession)
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0028
  - TASK-0031
  - TASK-0032
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Depends-on (package image v2): tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md
  - Depends-on (VMO plumbing): tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - Depends-on (ABI filter policies): tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md
  - Testing contract: scripts/qemu-test.sh
  - Superseded by: tasks/TASK-0295-zero-copy-read-write-vmo-splice.md
---

## End-state rewrite 2026-09-09 (binding; supersedes the 2026-07-15 supersession note below)

**Ground truth 2026-09-09:** the VMO-splice read path exists at the VFS layer (TASK-0295 Done:
`source/services/vfsd/src/splice_os.rs`, `OP_READ_VMO`, header-last release, `SELFTEST: vfs
splice roundtrip ok`) and the package layer already hands out digest-checked VMOs
(`bundlemgrd` `GET_FILE_VMO`, TASK-0321 P5, `payload_ops.rs`; packagefsd fetches through it,
`packagefsd/src/os_lite.rs`). But the two never meet: vfsd's `pkg:/` branch
(`splice_os.rs:139-166`) does `namespace.open → Vec<u8> → vmo_write` — packagefsd bytes are
copied into packagefsd, over IPC into vfsd, and into the caller's VMO (three copies), and two
payload-header codecs coexist (`NXVR {status u16, len u32}` in `userspace/vfs-types/src/
splice.rs` vs `NXPL {status u8, len u32}` in `nexus-wire/src/bundlemgrd.rs`). No `pkg:/` marker
exists. The original goal — a VMO view of a package entry, integrity verified, bounded — is
therefore delivered only for direct bundlemgrd clients, not through the VFS surface apps use.

### Goal (end system)

`pkg:/` `OP_READ_VMO` = the caller's VMO is cap-moved vfsd → packagefsd → bundlemgrd, and
bundlemgrd is the only byte writer (digest-checked against the verified volume index);
vfsd/packagefsd never copy or poll. One payload-VMO header codec for the whole system.

### Non-goals

Writable packagefs; the nxfs `/data` branch (already streams); kernel VMO sealing (TASK-0290,
soft dependency); cross-device VMO transport.

### Invariants

- Header-last release ordering; payload ≤ VMO capacity or `E2BIG` (never truncated).
- Every hop may write only an ERROR header; the success header is written only by
  bundlemgrd, only after the entry digest verified.
- Bounded: one in-flight forwarded VMO per packagefsd client slot; unknown path / digest
  mismatch fail deterministically with named codes.

### Decisions

- **D1 Model audit.** Both sides already use caller-provided VMO / server fills / header-last
  (vfs `OP_READ_VMO`; bundlemgrd `GET_FILE_VMO` where packagefsd is the caller). The conflict is
  only the header codec and who polls — no model change needed.
- **D2 One codec.** `nexus_wire::payload_vmo` (`MAGIC = "NXVR"`, 16 B, `status u16` = RFC-0072
  codes + `CODE_INTEGRITY` for digest failure, `len u32`) becomes the SSOT; `vfs-types::splice`
  re-exports it; bundlemgrd's `PAYLOAD_MAGIC`/`encode|decode_payload_header`/
  `PAYLOAD_STATUS_*` are DELETED and every decoder (execd `GET_PAYLOAD`, init bundle-ELF /
  `GET_INDEX`, packagefsd, app-host) decodes the one header. Gate: the codec exists in exactly
  one module (structure test in nexus-wire), roundtrip + negative tests, and the boot ladder
  (`bundlemgrd: bundle served`, `init: spawn from volume`) proves the volume path still boots.
  Contract: RFC-0097 "Payload-VMO header v2 + `pkg:/` pass-through" (beyond RFC-0072 Phase 3).
- **D3 Pass-through.** New packagefsd op `OP_READ_VMO` (path + CAP_MOVE VMO) forwards to
  bundlemgrd `GET_FILE_VMO`; vfsd's copying `pkg:/` branch is DELETED and replaced by the
  forward; packagefsd's `VolumeReader.vmo` shrinks to `INLINE_IO_MAX + 16` (inline `OP_READ`
  only). Gate: `splice_os.rs` has no `pkg:/` byte path (grep-gate in `tests/vfs_e2e`).
- **D4 Errors.** Unknown path → packagefsd writes `NotFound` header and closes; digest failure →
  bundlemgrd writes `CODE_INTEGRITY`; oversize → `E2BIG` before any write.

### Packages

- **P0** RFC-0097 (approval zone `docs/rfcs`). Blast: paper.
- **P1** Codec unification (nexus-wire + vfs-types + all decoders). Blast: volume boot, execd
  app launch, OTA `bundle reused`/flip/fallback lanes, packagefsd mount — `test-all`.
- **P2** Pass-through (vfsd/packagefsd/bundlemgrd) + selftest. Blast: vfs lanes,
  `vfsd: vmo splice` markers, visible lane (app assets via `pkg:/`).
- **P3** Docs + markers (`docs/storage/packagefs.md`, RFC-0072 cross-link).

### Definition of Done

Host: header roundtrip + negatives (`test_reject_short_header`, `test_reject_bad_magic`,
`test_reject_integrity`); packagefsd forward unit test against a fake bundlemgrd.
QEMU (registered in `proof-manifest/markers/vfs.toml`, `scripts/qemu-test.sh`, `markers.txt`):
`bundlemgrd: file vmo ok (bytes=<n>)`, `packagefsd: read vmo forwarded`,
`SELFTEST: pkgimg vmo ok` (selftest reads a > 4 KiB `pkg:/` entry via `OP_READ_VMO`, sha256 ==
index digest); the copying marker `vfsd: vmo splice read ok` leaves the contract.

### Touched paths

`source/libs/nexus-wire/src/{payload_vmo.rs (new),bundlemgrd.rs}`, `userspace/vfs-types/src/
splice.rs`, `source/services/vfsd/src/splice_os.rs`, `source/services/packagefsd/src/os_lite.rs`,
`source/services/bundlemgrd/src/payload_ops.rs`, `source/services/execd/src/*` (payload decode),
`source/init/nexus-init/src/bootstrap/*` (bundle-ELF decode), selftest `vfs.rs`, markers triple.

### Dependencies (active work only)

TASK-0324 P4 (vfsd/packagefsd/bundlemgrd on the declarative topology arm; the
Vfsd→Packagefsd route exists) and P0's `check-bundle-provenance.sh` lane as the regression
signal.

> **Supersession note (2026-07-15, historical — reopened 2026-09-09, see the end-state rewrite above).** The VMO-splice read path is absorbed into `TASK-0295`
> (zero-copy read/write for the whole VFS surface: packagefs + nxfs), under the RFC-0072 Phase 3
> contract. The goals and bounds below remain valid input; execution and proof live in TASK-0295.
> Do not implement against this file.

## Context — historical, superseded by the end-state rewrite above

Package image v2 provides fast lookup and read paths, but large payload reads still copy bytes through IPC.
The architecture vision expects VMO/filebuffer for bulk data-plane transfers.

This task adds a **splice-to-VMO** path while keeping a safe fallback to the existing copy-based `read`.

## Goal — historical, superseded by the end-state rewrite above

Provide a capability-gated, bounded “splice to VMO” API for packagefs:

- for large reads, clients can request a VMO-backed view of an image range,
- integrity is preserved (hash verified against index),
- fallback to copy-based read remains available and tested.

## Non-Goals

- Writable packagefs.
- Cross-device transport of VMOs (DSoftBus VMO frames are separate).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success markers (only after VMO mapped and digest verified).
- Bounded budgets:
  - max splice length per request,
  - max total live VMOs served by packagefsd,
  - LRU eviction.
- Policy-gated (ABI filters + policyd), deny-by-default.

## Red flags / decision points

- **RED (VMO transfer feasibility)**:
  - If VMO handles cannot be safely transferred across processes with existing syscalls/caps, this task cannot deliver a real cross-process splice.
  - In that case, keep splice as an in-process optimization only and document it; do not claim “zero-copy”.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- For a large file entry:
  - `splice_to_vmo` returns a VMO descriptor/handle,
  - consumer maps RO and computes sha256,
  - sha256 equals index hash.
- Negative:
  - request beyond bounds fails deterministically,
  - hash mismatch fails deterministically.

### Proof (OS / QEMU) — gated

Once VMO sharing is proven:

- `packagefsd: splice→vmo ok (len=<n>)`
- `SELFTEST: pkgimg vmo ok`

## Touched paths (allowlist)

- `source/services/packagefsd/` (splice handler + budgets; gated)
- `userspace/memory/nexus-vmo/` (consumer mapping helpers)
- `source/apps/selftest-client/` (gated markers)
- `docs/storage/packagefs.md` (splice semantics + budgets)
- `scripts/qemu-test.sh`
