# TASK-0302: Shared glyph-atlas RO VMO

- Status: Done
- Owners: @ui / @kernel-mm-team / @runtime
- RFC: `docs/rfcs/RFC-0080-shared-atlas-ro-vmo.md`
- Related: RFC-0075 8d (the baked CJK atlas), 8e/8f (arena reclaim)

## Goal / stop condition

The ~4.25 MB glyph atlas is ONE shared read-only mapping across windowd + every
app-host. Opening N app windows adds ~0 atlas bytes (arena flat across an
open/close storm). Pixel output unchanged.

## Phase 0 — library foundation (host-provable)

- [x] `nexus-text-baked`: `Face.cov` resolves via a process-global atlas base
      (`AtomicPtr<u8>` + len) + compile-time `(cov_offset, cov_len)` per face;
      `set_atlas_base(ptr, len)`.
- [x] build.rs: bake ONE concatenated blob (`font13 ++ font16`) to a file +
      emit `*_COV_OFFSET/LEN`; `EMBEDDED_ATLAS` include_bytes behind the
      `embedded-atlas` feature (default ON → base auto-inits, no behavior
      change).
- [x] Host goldens: base pointed at a heap copy resolves byte-identically to
      the embedded path; `measure`/`draw_text_row` unchanged; determinism.

## Phase 1 — provisioning (QEMU)

- [x] execd owns the atlas VMO (`vmo_create` + `vmo_write` from
      `EMBEDDED_ATLAS`); RO-clone-grants it to each app-host on spawn.
- [x] app-host maps it RO at a fixed VA (`vmo_map_page` loop) + `set_atlas_base`
      before first render; builds `nexus-text-baked` with `embedded-atlas` OFF
      (and dsl-runtime too — feature unification).
- [x] windowd keeps its embedded copy (`embedded-atlas` ON).
- [x] Proof: `VMO-POOL` used/peak flat across an ≥4-window open/close storm;
      CJK text still renders; app-host ELF ~4.25 MB smaller.

## Non-goals (this task)

- RO-only VMO right (hardening follow-up); kernel static-VMO zero-copy variant.
- Shrinking the 224 MB arena / 8 MiB heap bridges (separate, once shared).
