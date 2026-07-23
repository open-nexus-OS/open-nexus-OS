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

- Kernel static-VMO zero-copy variant (still deferred; see RFC-0080 Follow-ups).
- Shrinking the 224 MB arena / 8 MiB heap bridges — INTENTIONALLY not pursued
  (desktop system, conservative memory headroom preferred).

## Follow-up landed — RO-only VMO right (kernel-enforced write protection)

- [x] `CapabilityKind::VmoRo` — a contained READ-ONLY alias of a `Vmo`'s
      physical pages (existing `Vmo` caps untouched → zero audit surface).
- [x] `vmo_share_readonly` (SYSCALL 51): derives a `VmoRo` from a `Vmo` held
      with `MAP` rights; nexus-abi wrapper.
- [x] `sys_map` runs a `VmoRo` cap's page flags through the pure host-tested
      `vmo_ro::force_readonly` (WRITE|EXECUTE ALWAYS stripped); `vmo_write`
      rejects `VmoRo`. A compromised app-host runtime cannot write the atlas.
- [x] execd derives the RO alias after filling the atlas and DROPS the writable
      cap — it grants only the RO alias; nobody (not even execd) can corrupt it.
- [x] Proof: `neuron` host tests (`vmo_ro`, 2/2), ci-os-smp1 atlas-RO chain
      (`execd: atlas vmo ready/granted` → `APPHOST: atlas mapped`, text renders,
      no `VMO-POOL exhausted`).
