---
title: TASK-0033 packagefs v2b: zero-copy `pkg:/` reads — VMO pass-through from bundlemgrd + ONE payload-VMO header codec
status: In Progress (reviewed 2026-09-18 — end-state rewrite; residual of the TASK-0295 supersession)
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
  - Contract (this task): docs/rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md
  - Contract (VFS surface): docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md
  - VFS splice landed here: tasks/TASK-0295-zero-copy-read-write-vmo-splice.md
  - Digest-checked file VMOs land here: tasks/TASK-0321-ota-phase-b-verified-system-volume-bundle-set.md
  - Transport cap + E2BIG: tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md
---

## End-state rewrite 2026-09-18 (binding; supersedes the 2026-09-09 rewrite and the 2026-07-15 supersession note below)

**The idea (2025-12-22), restated:** a package entry should be readable as a VMO view —
integrity verified once, at the authority that owns the bytes, and bounded. That idea is
right and is the end state. What the 2026-09-09 review could not yet see is that it is not a
*performance* idea. It is the only thing that makes `pkg:/` a filesystem at all.

### Ground truth 2026-09-18 (measured against today's tree, after TASK-0324 and TASK-0054C)

**The pieces exist and still do not meet.** The VMO-splice read path is live at the VFS layer
(TASK-0295: `source/services/vfsd/src/splice_os.rs`, `OP_READ_VMO`, payload-first /
header-last release, `SELFTEST: vfs splice roundtrip ok`), and the package layer already hands
out digest-checked VMOs (`bundlemgrd` `OP_GET_FILE_VMO = 11`, TASK-0321 P5, `payload_ops.rs`)
— packagefsd is already a correct client of it, with the reply-cap + header-last discipline
from TASK-0324 P7-d (`packagefsd/src/os_lite.rs:193`). But vfsd's `pkg:/` branch
(`splice_os.rs:139`) still does `namespace.open(&path).map(|h| h.bytes)` → `vmo_write`.

**And below it, one op carries everything.** `vfsd/src/os_lite.rs:97` `packagefs_resolve` is
the ONLY packagefsd read op: `stat` (`:124`), `open` (`:132`), inline `read` and `read_vmo`
all go through it, and it ships the WHOLE entry inline —
packagefsd answers `[found u8][size u64][kind u16][bytes…]` with a blocking `server.send`
(`packagefsd/src/os_lite.rs:286`), after materializing the entry with `vec![0u8; len]` on a
384 KiB bump heap that never frees (`VolumeReader::fetch`, `:193`).

**What that costs, measured on today's volume** (`packagefsd: mounted (system volume slot=a
bundles=23 files=115)`): of the 115 entries, **22 are larger than the 8181-byte reply ceiling**
(`IPC_PAYLOAD_MAX` 8192 minus the 11-byte header), and 2 of those are larger than the whole
packagefsd heap. They are exactly the files an app or a launcher reads:

| entry | bytes | |
|---|---|---|
| `pkg:/windowd/payload.elf` | 7 247 776 | larger than packagefsd's whole 384 KiB heap |
| `pkg:/gpud/payload.elf` | 493 608 | larger than packagefsd's whole 384 KiB heap |
| `pkg:/settings/payload.nxir` | 259 424 | over the reply ceiling |
| `pkg:/desktop-shell/payload.nxir` | 223 896 | over the reply ceiling |
| … 18 more, down to `pkg:/greeter/payload.nxir` | 16 600 | over the reply ceiling |

There are two endings and both are fatal. The two largest entries exceed the whole heap, so
`vec![0u8; len]` reaches `alloc_error` first; the rest reach `server.send` →
`IpcError::TooBig` → `LiteError::Transport` → `?` → `run_loop` returns →
`service_main_loop` exits. (Where exactly the line falls between the two depends on how much
of the 384 KiB the registry already holds — which is itself the argument: the size at which
`pkg:/` stops working is not a declared bound, it is whatever is left of a bump heap.)
**`pkg:/` today serves 93 of its 115 files; a request for any of the other 22 kills
packagefsd, and `pkg:/` is then gone fleet-wide until reboot.** A `stat` is enough — the
resolve op does not distinguish metadata from bytes.

**Why every lane is green.** The only `pkg:/` file any proof reads is
`pkg:/system/build.prop` — **19 bytes**. `SELFTEST: pkgimg mount ok`, `SELFTEST: pkgimg
stat/read ok`, `SELFTEST: vfs splice roundtrip ok` and `vfsd: vmo splice read ok` are all
true, and all four are proven against those 19 bytes. (The 2026-09-09 note "No `pkg:/` marker
exists" was wrong in letter and right in substance: the markers exist; not one of them moves a
byte the transport cannot carry.) The large number in the boot log —
`vfsd: vmo splice stream ok (bytes=266240)` — is the nxfs `/data` branch, which streams in
64 KiB windows straight into the caller's VMO; the `pkg:/` branch is precisely the one that
does not.

**TASK-0054C changed the errno, not the ceiling.** The 8 KiB transport cap predates it
(`EINVAL`); it is now `nexus_abi::IPC_PAYLOAD_MAX` / `E2BIG` / `IpcError::TooBig`. D4's error
model is therefore already expressible in the ABI, and nothing in D1–D4 below needs to change.

**Two payload-header codecs still coexist**, unchanged since 2026-09-09: `NXPL`
(`nexus-wire/src/bundlemgrd.rs:158`, `status u8` + `len u32`) with 52 matching lines across 7 files
(bundlemgrd `payload_ops.rs` 17, `nexus-wire` 16, packagefsd 9, app-host `probe/boot.rs` 4 +
`probe/mod.rs` 1, init `bootstrap/volume_spawn.rs` 3, execd `os_lite.rs` 2) versus `NXVR`
(`vfs-types/src/splice.rs:35`, `status u16` + `len u32`).

### Architecture-review verdict (three lenses, 2026-09-18)

- **Scope** — in: the `pkg:/` read path (`vfsd/splice_os.rs`, `vfsd/os_lite.rs`,
  `packagefsd/os_lite.rs`, `bundlemgrd/payload_ops.rs`) and the one payload-VMO codec
  (`nexus-wire`, `vfs-types`, its 7 decoder files). Out: writable packagefs, the nxfs branch,
  kernel VMO sealing (TASK-0290), cross-device VMO transport, and any change to
  `OP_GET_FILE_VMO`'s own protocol — it is already right and stays.
- **Invariant** — bytes cross a hop only as a VMO; only bundlemgrd writes a success header,
  and only after the digest verifies; every other hop may write an ERROR header only. Size is
  refused, never truncated. Negative tests: `test_reject_short_header`,
  `test_reject_bad_magic`, `test_reject_integrity`, `test_reject_oversize_for_vmo`.
- **Contract** — RFC-0097 "Payload-VMO header v2 + `pkg:/` pass-through", extending RFC-0072
  Phase 3; no new ADR (no boundary moves — the same three services, the same VMO model, one
  codec instead of two).

### Goal (end system)

`pkg:/` reads are a VMO pass-through: the caller's VMO is cap-moved vfsd → packagefsd →
bundlemgrd, bundlemgrd is the only byte writer (digest-checked against the verified volume
index), and vfsd/packagefsd never copy, never buffer and never poll. One payload-VMO header
codec for the whole system. `pkg:/` serves **every** entry on the volume, and an entry that
does not fit the caller's VMO is refused with a named code — never a dead service.

### Non-goals

Writable packagefs; the nxfs `/data` branch (already streams); kernel VMO sealing (TASK-0290,
soft dependency); cross-device VMO transport; changing `OP_GET_FILE_VMO`'s protocol.

### Invariants

- Header-last release ordering; payload ≤ VMO capacity or `E2BIG` (never truncated).
- Every hop may write only an ERROR header; the success header is written only by
  bundlemgrd, only after the entry digest verified.
- **No `pkg:/` operation is bounded by `IPC_PAYLOAD_MAX`.** `stat` returns metadata only;
  bytes move through the VMO. The 22-entry class above must become readable, and the failure
  for a too-small VMO must be a code in the header, not a dead server.
- Bounded: one in-flight forwarded VMO per packagefsd client slot; unknown path / digest
  mismatch fail deterministically with named codes.
- No per-request allocation in packagefsd's loop (`os-service-bump-allocator-no-free`): after
  the pass-through, packagefsd holds no entry bytes at all.

### Decisions

- **D1 Model audit.** Both sides already use caller-provided VMO / server fills / header-last
  (vfs `OP_READ_VMO`; bundlemgrd `GET_FILE_VMO` where packagefsd is the caller). The conflict is
  only the header codec and who copies — no model change needed.
- **D2 One codec.** `nexus_wire::payload_vmo` (`MAGIC = "NXVR"`, 16 B, `status u16` = the
  RFC-0072 code space, `len u32`) is the SSOT; `vfs-types::splice` re-exports it; bundlemgrd's
  `PAYLOAD_MAGIC`/`encode|decode_payload_header`/`PAYLOAD_STATUS_*` are DELETED and every
  decoder (execd `GET_PAYLOAD`, init bundle-ELF / `GET_INDEX`, packagefsd, app-host) decodes
  the one header. Both headers were already 16 B with the magic at `[0..4]` and `len` at
  `[8..12]`, so the bytes change only in the magic.

  **Revised by measurement (P1).** The status TABLE moves to `nexus_wire::status` as well
  (`nexus_vfs_types` re-exports it, so the ~160 `VfsError` sites and 10 dependent crates are
  untouched). It has to: the header is written and read by bundlemgrd, execd, init and
  app-host, none of which are VFS clients. Both candidate crates have zero dependencies and
  `nexus-abi` already re-exports `nexus-wire`, so this direction adds no dependency edge
  anywhere, while the other would have put a userspace VFS crate under a core ABI crate.

  The private space had FIVE values, not three — and bundlemgrd's generic `STATUS_*` family was
  written into the same header byte: `PAYLOAD_STATUS_OK 1` → `CODE_OK 0`, `UNKNOWN 2` →
  `NotFound 1`, `TOO_LARGE 3` → `TooBig 8`, `DIGEST 4` → `Integrity 9`, `NOT_ARMED 5` →
  `Invalid 11`, plus `STATUS_MALFORMED 1` → `Invalid 11`, `STATUS_UNAVAILABLE 5` → `Io 13` and
  a denied payload op `STATUS_UNSUPPORTED 2` → `Access 2`. **`STATUS_MALFORMED` and
  `PAYLOAD_STATUS_OK` were both `1`**, so a malformed request wrote a header that read as
  SUCCESS and execd's `status != PAYLOAD_STATUS_OK` could never fire — the system was saved
  only by a downstream `len == 0` check in app-host. One table with `0 = OK` makes that
  unrepresentable. The VMO op's done-reply carries the same table and widens with it
  (`PAYLOAD_DONE_RSP_LEN` 9 → 10, `status:u16le`); `QUERY_BUNDLE`/`VOLUME_STATUS` are not
  payload ops and keep the generic space, which can no longer leak into a header because the
  types differ.

  Two rules become explicit because `CODE_OK` is `0` and a fresh VMO is all-zero: the header is
  written in ONE 16-byte `vmo_write` (`encode_header` returns the whole array for that reason),
  and a REUSED VMO is zeroed with `ZEROED_HEADER` before it is armed (packagefsd already did
  the second — it survived the migration). Gate: `scripts/check-payload-vmo.sh`
  (`just payload-vmo`, in `just check`) counts declarations — one magic, one status table, no
  retired magic, no retired private status space — with a self-test proving it catches a second
  declaration; plus roundtrip + negative tests, and the boot ladder (`bundlemgrd: bundle
  served`, `init: spawn from volume`, `packagefsd: mounted`, `APPHOST: payload source=bundle`)
  proving the volume path still boots.
- **D3 Pass-through.** New packagefsd op `OP_READ_VMO` (path + CAP_MOVE VMO) forwards to
  bundlemgrd `GET_FILE_VMO`; vfsd's copying `pkg:/` branch (`splice_os.rs:139`) is DELETED and
  replaced by the forward; `packagefs_resolve` is split — `stat`/`open` carry metadata
  (`size`, `kind`) only, bytes never ride the reply — and `VolumeReader.vmo` shrinks to
  `INLINE_IO_MAX + 16` (inline `OP_READ` only). packagefsd's loop moves to the one server
  shape (`PendingReply` + `serve_next`, TASK-0054C P5c) while its dispatch is being rewritten.
  Gate: `splice_os.rs` has no `pkg:/` byte path and `packagefs_resolve` has no `bytes` field
  (grep-gate in `tests/vfs_e2e`).
- **D4 Errors.** Unknown path → packagefsd writes `NotFound` header and closes; digest failure →
  bundlemgrd writes `CODE_INTEGRITY`; entry larger than the caller's VMO → `TooBig` in the
  header, written before any payload byte.

### Packages

- **P0** RFC-0097 seed + this ledger + IMPLEMENTATION-ORDER line (approval zone `docs/rfcs`).
  Blast: paper.
- **P1** ✅ **2026-09-18** Codec unification (`nexus-wire/{payload_vmo,status}.rs`, `vfs-types`
  re-exports, all 7 decoder files, `NXPL` + its private status space deleted, done-reply
  widened, `scripts/check-payload-vmo.sh` in `just check`). Blast: volume boot, execd app
  launch, OTA `bundle reused`/flip/fallback lanes, packagefsd mount — `test-all`.
  Incidental, found by the lane and fixed with it: `SELFTEST: ui v2 input ok` was emitted by
  windowd but declared in no marker manifest, and evidence assembly is deny-by-default on an
  unknown `SELFTEST:` line — so every run that actually drove input failed to assemble its
  bundle, and every run that did not passed. Registered in `markers/ui.toml` without an
  `emit_when` (it is a real line, not a claim every profile must produce); proven by replaying
  the exact UART that failed.
- **P2** ✅ **2026-09-18** Pass-through + the oversize selftest. `pkg:/settings/payload.nxir`
  (259 424 B) is read end to end through the caller's VMO: `packagefsd: read vmo forwarded
  (bytes=259424 n=2)` → `vfsd: vmo splice forwarded ok (bytes=259424)` →
  `SELFTEST: pkgimg vmo ok (bytes=0x3f560)`, plus `SELFTEST: pkgimg vmo oversize deny ok`.
  Beyond D3 as written, three things the work turned up:
  (a) **the size bound had no owner** — it was discovered while writing, so the code depended on
  which layer noticed first (`Io` from the block read vs `TooBig` from the hash read-back);
  packagefsd now refuses before forwarding, being the hop that knows the entry's size and the
  VMO's length. The boot proof caught this, not review.
  (b) **packagefsd's response endpoint had three declared readers** (vfsd, dsoftbusd, harness)
  on one queue — the 0049B/P2-g hazard, and unusable for an answer that names a capability. All
  three legs are `RouteKind::ReplyInbox` now (a declaration change; init already implemented the
  kind) and packagefsd answers only senders that moved a reply cap.
  (c) **`ArmedVmos` was bundlemgrd-private** and packagefsd needs the same table, so it moved to
  `nexus_ipc::armed_vmo` rather than being copied.
  Four files crossed the LOC ratchet and were split by responsibility, not baseline-bumped:
  `packagefsd/volume_reader.rs`, `vfsd/namespace.rs`, `nexus-vfs/os.rs`, and the dsoftbus
  packagefs leg into `packagefs_ro.rs`. Blast: vfs lanes, `vfsd: vmo splice` markers, visible
  lane (app assets via `pkg:/`).
- **P3** Docs + markers (`docs/storage/packagefs.md`, RFC-0072 cross-link, CHANGELOG,
  ledger Done).

### Definition of Done

Host: header roundtrip + negatives (`test_reject_short_header`, `test_reject_bad_magic`,
`test_reject_integrity`, `test_reject_oversize_for_vmo`); packagefsd forward unit test against
a fake bundlemgrd; a grep gate proving exactly one payload-VMO magic in the tree.
QEMU (registered in `proof-manifest/markers/vfs.toml`): `packagefsd: read vmo forwarded
(bytes=<n>)`, `vfsd: vmo splice forwarded ok (bytes=<n>)`, and `SELFTEST: pkgimg vmo ok
(bytes=<n>)` where **`<n>` is an entry from the 22-entry class above the old ceiling**
(`pkg:/settings/payload.nxir`, 259 424 B) read through `OP_READ_VMO` — the selftest refuses to
emit the marker at all for an entry ≤ 8181 bytes, so it cannot quietly fall back to proving
what `build.prop` proved. Integrity stays bundlemgrd's: an OK header appears only after the
entry hashes to its index digest. `SELFTEST: pkgimg vmo oversize deny ok` proves a VMO half the
entry's size is refused with `TooBig` and that packagefsd is still serving afterwards. The
copying marker `vfsd: vmo splice read ok` leaves the contract.

### Touched paths

`source/libs/nexus-wire/src/{payload_vmo.rs (new),bundlemgrd.rs,lib.rs}`, `userspace/vfs-types/
src/splice.rs`, `source/services/vfsd/src/{splice_os.rs,os_lite.rs}`, `source/services/
packagefsd/src/os_lite.rs`, `source/services/bundlemgrd/src/payload_ops.rs`, `source/services/
execd/src/os_lite.rs`, `source/services/app-host/src/probe/{boot.rs,mod.rs}`, `source/init/
nexus-init/src/bootstrap/volume_spawn.rs`, selftest `src/os_lite/vfs.rs`, markers triple,
`docs/rfcs/RFC-0097-*.md`, `docs/storage/packagefs.md`, `CHANGELOG.md`.

### Dependencies (active work only)

None open. TASK-0324 P0–P9 and TASK-0054C P0–P6 are Done; the Vfsd→Packagefsd route exists on
the declarative topology arm and `check-bundle-provenance.sh` is the regression signal.

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
