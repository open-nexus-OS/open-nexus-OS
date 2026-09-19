# VMO Plumbing v1 (TASK-0031)

`TASK-0031` establishes the host-first VMO plumbing floor for zero-copy claims.
It does **not** claim production-grade kernel closure; that remains routed to `TASK-0290`.

## Contract surface

- Userspace crate: `userspace/memory` (`package = nexus-vmo`)
- Typed API:
  - `Vmo::create(len)`
  - `Vmo::from_bytes(bytes)`
  - `Vmo::from_file_range(path, offset, len)` (host-only fixture helper)
  - `Vmo::write(offset, bytes)` (bounded)
  - `Vmo::map_ro(offset, len)` (host)
  - `Vmo::slice(offset, len)` (`VmoSlice` bounded read-only view)
  - `Vmo::transfer_to(peer, rights)` (deny-by-default authorization)
  - `Vmo::transfer_to_slot(peer, rights, dst_slot)` (OS slot-directed transfer)
- Deterministic counters:
  - `copy_fallback_count`
  - `control_plane_bytes`
  - `bulk_bytes`
  - `map_reuse_hits`
  - `map_reuse_misses`

## Security and honesty stance

- Transfer is deny-by-default unless the peer is explicitly authorized.
- Oversized mappings and out-of-range offsets fail closed.
- Sealed RO buffers reject writes via crate policy.
- Markers are emitted only after real behavior; no marker-only success path.
- v1 still depends on kernel closure from `TASK-0290` for production-grade sealing rights.

## Proof commands

Host-first:

```bash
cargo test -p nexus-vmo -- --nocapture
cargo test -p nexus-vmo -- reject --nocapture
```

OS-gated: **retired.** The two-process VMO-share proof (`vmo: producer sent handle` …
`SELFTEST: vmo share ok`) was retired with the RFC-0068 exec migration — the producer blocked on a
consumer child that no longer ran — and has not been in the ladder since. Its unscheduled probe and
hand-built consumer ELF were deleted in TASK-0324 P4f-6. A restored proof belongs on the app-child
spawn path with a declared child slot; kernel-enforced seal/right hardening remains in `TASK-0290`.

## Consumers

The VMO transfer floor (RFC-0040) is used in production by:

All of them speak ONE header (`nexus_wire::payload_vmo`, RFC-0097): magic `NXVR`, 16 bytes,
`status u16` from the RFC-0072 table, `len u32`, data at offset 16. There were two of these —
`NXVR` for the vfs splice and `NXPL` for bundlemgrd's payload ops — until TASK-0033 P1; the
private one carried a status space whose values collided with bundlemgrd's generic reply
statuses, so a malformed request wrote a header that read as SUCCESS.

- **execd → bundlemgrd → app** payload load: execd creates a payload VMO, CAP_MOVEs
  a clone to bundlemgrd, which fills it payload-first + header-last; execd then moves
  the VMO into the child's fixed payload slot.
- **vfsd `OP_READ_VMO`** zero-copy file reads (RFC-0072 Phase 3, `TASK-0295`): the client
  creates a VMO, CAP_MOVEs it to vfsd with the read request, and the provider fills it
  **payload-first, header-last**. The client waits on the header (bounded) and reads the bytes
  back — or maps the VMO and copies nothing. Reads at or below `INLINE_IO_MAX = 4096` stay
  inline; above it, inline is `E2BIG` (never a silent slow path).
  - `/data` (nxfs): vfsd is the provider and streams in 64 KiB windows straight into the
    caller's VMO. Marker `vfsd: vmo splice stream ok (bytes=<n>, fallbacks=<m>)`.
  - `pkg:/` (read-only volume): vfsd is NOT the provider — it forwards the caller's VMO to
    packagefsd, which forwards it to bundlemgrd, the only hop that may write bytes or a
    success header, and only after the entry hashes to its index digest (TASK-0033 P2,
    RFC-0097 §2). Markers `packagefsd: read vmo forwarded (bytes=<n> n=<count>)` and
    `vfsd: vmo splice forwarded ok (bytes=<n>, fallbacks=<m>)`.
  Proven: `SELFTEST: vfs splice roundtrip ok` (cross-process byte-equality),
  `SELFTEST: vfs inline oversize deny ok`, and — for the pass-through —
  `SELFTEST: pkgimg vmo ok (bytes=0x3f560)`: 259 424 bytes, an entry that no `pkg:/` read
  could carry before, plus `SELFTEST: pkgimg vmo oversize deny ok` for a VMO too small.
  The size bound is checked by packagefsd BEFORE the VMO is forwarded: it is the hop that
  knows both the entry's size and the VMO's length, and leaving it to the writer made the
  error code depend on which layer noticed the overrun first.
