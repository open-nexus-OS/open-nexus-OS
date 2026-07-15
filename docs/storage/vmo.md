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

OS-gated:

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Required marker ladder:

- `vmo: producer sent handle`
- `vmo: consumer mapped ok`
- `vmo: sha256 ok`
- `SELFTEST: vmo share ok`

Current limitation:

- Two-process VMO proof is now real (producer task -> transferred slot in spawned consumer task -> consumer RO map + bounded payload verification).
- Producer-side `sha256` marker currently pairs consumer success with deterministic fixture digest check; kernel-enforced seal/right hardening still remains in `TASK-0290`.

## Consumers

The VMO transfer floor (RFC-0040) is used in production by:

- **execd → bundlemgrd → app** payload load: execd creates a payload VMO, CAP_MOVEs
  a clone to bundlemgrd, which fills it payload-first + header-last; execd then moves
  the VMO into the child's fixed payload slot (`nexus_abi::bundlemgrd` header codec).
- **vfsd `OP_READ_VMO`** zero-copy file reads (RFC-0072 Phase 3, `TASK-0295`): the client
  creates a VMO, CAP_MOVEs it to vfsd with the read request, and the provider (nxfs `/data`
  or read-only `pkg:/`) fills it **payload-first, header-last** using the shared
  `nexus-vfs-types::splice` header (magic `NXVR`, 16-byte header, data at offset 16). The
  client polls the header (bounded) and reads the bytes back. Reads/writes at or below
  `INLINE_IO_MAX = 4096` stay inline; above it, inline is `E2BIG` (never a silent slow path).
  Fallbacks are counted, not silent: `vfsd: vmo splice read ok (bytes=<n>, fallbacks=<m>)`.
  Proven: `SELFTEST: vfs splice roundtrip ok` (cross-process byte-equality) +
  `SELFTEST: vfs inline oversize deny ok`.
