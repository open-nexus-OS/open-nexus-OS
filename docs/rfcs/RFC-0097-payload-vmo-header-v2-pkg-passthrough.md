<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0097: Payload-VMO header v2 — ONE header codec, and `pkg:/` reads as a VMO pass-through

- Status: **Implemented 2026-09-18** (TASK-0033 P0–P3). `pkg:/` served 93 of the system volume's 115 entries when this was written, and asking for one of the other 22 ended packagefsd. The reply-frame ceiling is gone: an entry is bounded by the caller's VMO now, and nothing else. Proven on `pkg:/settings/payload.nxir` — 259 424 bytes, from the previously fatal class.
- Owners: @runtime
- Created: 2026-09-18
- Last Updated: 2026-09-18
- Links:
  - Tasks: `tasks/TASK-0033-packagefs-v2b-vmo-splice-from-image.md` (execution + proof, P0–P3)
  - Extends: `docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md` (Phase 3 named the VMO data plane and the `E2BIG` rule; this RFC makes `pkg:/` obey them and gives the header ONE definition)
  - Builds on: `docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md` / `tasks/TASK-0321-ota-phase-b-verified-system-volume-bundle-set.md` P5 (`OP_GET_FILE_VMO`: digest-checked file VMOs out of the verified system volume — unchanged by this RFC)
  - Cites: `docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md` (the control plane is small, bounded and capped at `IPC_PAYLOAD_MAX` → `E2BIG`; bulk belongs in a VMO — this RFC is the other half of that sentence)
  - Cites: `docs/rfcs/RFC-0079-ipc-last-sender-eof.md` (a forwarded request is awaited, or the peer's death ends it — never polled)

## Status at a Glance

- **P1 (one codec)**: ✅ 2026-09-18 — `nexus_wire::payload_vmo` is the SSOT, `NXPL` deleted, every decoder moved, gated by `just payload-vmo`
- **P2 (`pkg:/` pass-through)**: ✅ 2026-09-18 — vfsd → packagefsd → bundlemgrd, no hop copies; proven on a 259 424-byte entry
- **P3 (docs + markers)**: ✅ 2026-09-18

"Complete" means the contract is defined and the proof gates are green.

What this RFC did NOT need in the end: a new status code (`VfsError::Integrity` already
existed), an ADR (no boundary moved), and any change to `OP_GET_FILE_VMO` — the protocol the
pass-through rides was already right, which is why the work was mostly deletion.

## Scope boundaries (anti-drift)

- **This RFC owns**: the byte layout and status space of the payload-VMO header; who may write which header on a forwarded VMO; the `pkg:/` read path's shape and its bounds.
- **This RFC does NOT own**: `OP_GET_FILE_VMO`'s own request/reply protocol (TASK-0321 P5, unchanged); the nxfs `/data` branch (already streams); kernel VMO sealing (TASK-0290); writable packagefs; cross-device VMO transport.

## Context

Two facts stand next to each other in today's tree and have never been put together.

**The data plane exists twice.** `OP_READ_VMO` (RFC-0072 Phase 3, TASK-0295) moves bulk file
bytes as a caller-provided VMO and releases them with a header written last. `OP_GET_FILE_VMO`
(TASK-0321 P5) does the same thing one layer down, out of the verified system volume, with the
entry digest checked by bundlemgrd before the header goes down. packagefsd is already a
correct client of the second. vfsd still is not a client of the first for `pkg:/`.

**So `pkg:/` reads copy, and the copy has a ceiling.** `vfsd`'s `pkg:/` branch resolves an
entry by asking packagefsd for it, and packagefsd answers with the entry's bytes **inline in
the IPC reply**. That reply is bounded by `IPC_PAYLOAD_MAX` (8192 B, RFC-0096) minus an
11-byte header. Measured on the system volume as built today — 115 entries — **22 entries are
above that ceiling**, among them every service and app payload: `pkg:/windowd/payload.elf`
(7 247 776 B), `pkg:/gpud/payload.elf` (493 608 B), `pkg:/settings/payload.nxir` (259 424 B),
down to `pkg:/greeter/payload.nxir` (16 600 B).

Asking for one of them does not return an error to the caller. It **ends packagefsd**: the
oversized reply fails the send with `E2BIG`, the service loop maps that to a transport error
and returns, and `pkg:/` is gone fleet-wide until reboot. The largest entries do not even get
that far — they exceed packagefsd's whole 384 KiB never-freeing heap, so the service dies one
step earlier, in `alloc_error`, while materializing bytes it was never supposed to hold. The
size at which `pkg:/` stops working is therefore not a declared bound at all; it is whatever
is left of a bump allocator.

This has always been true and has never been seen, because the only `pkg:/` file any proof
reads is `pkg:/system/build.prop` — **19 bytes**. Four green markers stand on those 19 bytes.

**And the header is defined twice.** `NXVR` (`vfs-types::splice`, `status u16` = RFC-0072
codes, `len u32`) and `NXPL` (`nexus-wire::bundlemgrd`, `status u8` from a private
three-value space, `len u32`) are both 16 bytes, both put the magic at `[0..4]` and the length
at `[8..12]`, and differ only in the magic and in what a status number means. Two names for
one idea is how a system grows a second one.

## Goals

1. `pkg:/` serves **every** entry on the volume, with no hop copying and no hop buffering.
2. Exactly **one** payload-VMO header codec in the tree, enforced by a gate.
3. Integrity stays where it belongs: bundlemgrd remains the only writer of a success header,
   and only after the entry digest verifies against the verified volume index.
4. A read that does not fit is **refused with a code in the header** — never truncated, and
   never fatal to a service.

## Non-goals

Writable packagefs; changing `OP_GET_FILE_VMO`; the nxfs branch; kernel VMO sealing;
cross-device VMO transport; a general-purpose shared-memory protocol.

## The contract

### §1 One header: `nexus_wire::payload_vmo`

The SSOT moves to `source/libs/nexus-wire/src/payload_vmo.rs`. `vfs-types::splice` re-exports
it so `OP_READ_VMO` clients are unaffected; `nexus-wire::bundlemgrd`'s `PAYLOAD_MAGIC`,
`PAYLOAD_STATUS_*`, `encode_payload_header` and `decode_payload_header` are **deleted**.

The status TABLE moves with it, to `nexus_wire::status` (`nexus_vfs_types` re-exports it, so
the VFS surface is unchanged). It has to: the header is written and read by bundlemgrd, execd,
init and app-host, none of which are VFS clients, and a vocabulary that lives in a crate named
after one of its consumers is how a second one gets written. Because `nexus-abi` already
re-exports `nexus-wire`, every consumer reaches both through its existing
`nexus_abi::` paths — the unification adds no dependency edge anywhere.

```
offset  0   4   6   8      12     16
        |NXVR|st |rsv| len  | rsv |
         magic u16       u32
```

- `magic = "NXVR"`. Its presence is the release fence: a client that sees it sees complete
  data. Nothing else may be used as a completion signal.
- `status: u16` is the RFC-0072 code space (`error.rs`): `0 = OK`, `1 = NotFound`,
  `8 = TooBig`, `9 = Integrity`, `13 = Io`, … Unknown non-zero codes fail closed as `Io`.
  No private status space exists any more.
- `len: u32` is the payload byte count, starting at offset 16.

Two rules that were implicit in `NXPL` and become explicit here, because `CODE_OK` is `0` and
a fresh VMO is all-zero:

- **The header is written in ONE `vmo_write` of all 16 bytes.** Magic and status must never
  become visible separately.
- **A reused VMO has its header zeroed before it is armed.** Otherwise a stale `OK` from the
  previous operation is a valid completion signal for this one.

#### The migration, and the defect it removes

The private space had five values, not three, and bundlemgrd's GENERIC reply statuses were
written into the same header byte alongside them — two const families, one `u8`, same module:

| retired | value | replacement | value |
|---|---|---|---|
| `PAYLOAD_STATUS_OK` | 1 | `CODE_OK` | 0 |
| `PAYLOAD_STATUS_UNKNOWN` | 2 | `NotFound` | 1 |
| `PAYLOAD_STATUS_TOO_LARGE` | 3 | `TooBig` | 8 |
| `PAYLOAD_STATUS_DIGEST` | 4 | `Integrity` | 9 |
| `PAYLOAD_STATUS_NOT_ARMED` | 5 | `Invalid` | 11 |
| `STATUS_MALFORMED` (in the payload path) | 1 | `Invalid` | 11 |
| `STATUS_UNAVAILABLE` (in the payload path) | 5 | `Io` | 13 |
| a denied payload op | `STATUS_UNSUPPORTED` 2 | `Access` | 2 |

Read the first and sixth rows together: **`STATUS_MALFORMED` and `PAYLOAD_STATUS_OK` were both
`1`.** A malformed `GET_PAYLOAD` request wrote a header that decodes as SUCCESS, and execd —
whose only check is `status != PAYLOAD_STATUS_OK` — returned `true` and never emitted its
`execd: FAIL app payload (status)`. The system survived on a downstream `len == 0` check in
app-host, which is luck, not a contract. `STATUS_UNAVAILABLE` and `PAYLOAD_STATUS_NOT_ARMED`
collided the same way at `5`, reporting a dead volume as a client protocol error.

One table with `0 = OK` makes this unrepresentable: every error is non-zero, so no error can
read as success. That is what `test_reject_status_collision_is_unrepresentable` and
`test_reject_error_status_reads_as_ok` assert.

The VMO op's DONE-REPLY carries the same table, so it widens with it:
`[B, N, ver, op|0x80, status:u16le, len:u32le]`, `PAYLOAD_DONE_RSP_LEN` 9 → 10. A reply and
the header it announces can no longer disagree about what a number means. bundlemgrd's other
ops (`QUERY_BUNDLE`, `VOLUME_STATUS`) are not payload-VMO ops and keep their own generic
reply space — now unable to leak into a header, because the types differ.

### §2 `pkg:/` is a pass-through

A `pkg:/` `OP_READ_VMO` moves the caller's VMO, by CAP_MOVE, along the whole chain:

```
client ──VMO──▶ vfsd ──VMO──▶ packagefsd ──VMO──▶ bundlemgrd
                                                     │ verifies digest
                                                     │ writes payload
                                                     ▼ writes header LAST
```

- vfsd resolves the namespace and forwards; it does not read, write or size the bytes.
- packagefsd maps `pkg:/<bundle>/<path>` to a volume entry and forwards to `OP_GET_FILE_VMO`;
  it does not hold the bytes. Its own VMO shrinks to the inline-read tier
  (`INLINE_IO_MAX + 16`).
- bundlemgrd is the only byte writer and the only writer of a success header.
- Every forward is a request whose answer is **awaited** (reply capability, RFC-0079 EOF ends
  the wait) — never polled, never clock-bounded.

### §3 Metadata is not bytes

`stat` and `open` on `pkg:/` return `size` and `kind` only. The resolve reply carries no
entry bytes on any path. This is what removes the 8 KiB ceiling from operations that never
wanted bytes in the first place.

Inline `read` keeps its existing contract: at most `INLINE_IO_MAX` (4096 B), and above it
`E2BIG` — a protocol error, never a silent slow path (RFC-0072).

### §4 Errors

| condition | who writes it | code |
|---|---|---|
| path not on the volume | packagefsd | `NotFound (1)` |
| entry larger than the caller's VMO | the first hop that can tell, before any payload byte | `TooBig (8)` |
| entry digest ≠ index digest | bundlemgrd | `Integrity (9)` |
| transport / volume read failure | the hop that saw it | `Io (13)` |

A non-OK header is terminal for that request and fatal to nobody: every hop stays alive and
serving. No hop may write a success header it did not earn.

### §5 Bounds

- One in-flight forwarded VMO per packagefsd client slot, keyed by `sender_service_id` as the
  kernel stamped it (`nexus_ipc::armed_vmo`) — a sender is served only the VMO IT armed.
- No per-request heap allocation in packagefsd's serve loop — after the pass-through it holds
  no entry bytes at all (`os-service-bump-allocator-no-free`). Its reusable VMO shrinks from
  "the largest entry on the volume" (7.25 MiB) to the inline tier.
- The payload must fit the caller's VMO after reserving the 16-byte header (`fits`), and
  **packagefsd checks that before forwarding**. It is the hop that knows both numbers — the
  entry's size from the verified index and the VMO's length from the capability it holds — and
  leaving the check to the writer made the refusal depend on which layer noticed the overrun
  first (`Io` from the block read, `TooBig` from the hash read-back). That is not a contract;
  the boot proof caught it (P2).

### §6 Who answers whom

packagefsd answers exactly the senders that moved a reply capability. Its pre-minted RESPONSE
endpoint had three declared readers — vfsd, dsoftbusd and the harness — on one queue, where any
of them could take any of the others' answers; forwarding a VMO through it would have been
worse, because the answer names a capability. All three legs are `RouteKind::ReplyInbox` now
and that endpoint has no readers left.

## Proof

Host (P1, green): header roundtrip + golden bytes and the negatives
`test_reject_short_header`, `test_reject_bad_magic` (including the retired `NXPL` magic),
`test_reject_unwritten_header`, `test_reject_oversize_for_vmo`,
`test_reject_status_collision_is_unrepresentable`, `test_reject_error_status_reads_as_ok`.
Gate `just payload-vmo` (in `just check`) counts the declarations: one magic, one status
table, no retired magic and no retired private status space — with a self-test that proves the
scanner catches a second declaration. P2 adds packagefsd's forward against a fake bundlemgrd.

QEMU (P2, green — `smp1`, 2026-09-18):

- `packagefsd: read vmo forwarded (bytes=259424 n=2)` and
  `vfsd: vmo splice forwarded ok (bytes=259424)` — the hop lines, counted.
- `SELFTEST: pkgimg vmo ok (bytes=0x3f560)` — **259 424 bytes**: `pkg:/settings/payload.nxir`,
  an entry from the class that could not be read at all before, read end to end through the
  caller's VMO. The selftest refuses to emit it for an entry at or below 8181 bytes, so the
  marker cannot quietly fall back to proving what `build.prop` proved. Integrity is
  bundlemgrd's: it writes an OK header only after the entry hashes to its index digest, and
  `Integrity` otherwise.
- `SELFTEST: pkgimg vmo oversize deny ok` — a VMO half the entry's size is refused with
  `TooBig` and nothing is written; packagefsd is still serving, which the successful read
  after it proves. (Half, not one byte short: `vmo_create` rounds up to a page.)
- The copying marker `vfsd: vmo splice read ok` is gone from the tree. It had covered both
  providers, which hid that only one of them spliced; `/data` says
  `vfsd: vmo splice stream ok` and `pkg:/` says `vfsd: vmo splice forwarded ok` now.

## Relationship to RFC-0096

RFC-0096 fixed the control plane: small frames, one trap per side, a hard cap of
`IPC_PAYLOAD_MAX` with `E2BIG` so that an oversized frame is a named protocol error instead
of an accident. That cap is only honest if the bulk plane it implies actually exists on every
surface. `pkg:/` is the surface where it did not. This RFC is the other half.
