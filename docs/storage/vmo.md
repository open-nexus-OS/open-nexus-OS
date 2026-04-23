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
