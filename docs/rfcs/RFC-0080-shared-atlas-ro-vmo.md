# RFC-0080: Shared glyph-atlas RO VMO (kill per-instance duplication)

- Status: Draft
- Owners: @ui / @kernel-mm-team / @runtime
- Created: 2026-07-23
- Last Updated: 2026-07-23
- Links:
  - Tasks: `tasks/TASK-0302-shared-atlas-ro-vmo.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md`
    (Phase 8d baked the CJK atlas that made this duplication expensive),
    `docs/adr/0042-*` (per-app surface VMOs), RFC-0075 8f/8e (arena reclaim)

## Status at a Glance

- **Phase 0 (library: runtime atlas base + data split)**: ✅
  (2026-07-23 — `nexus-text-baked` resolves `cov` through a process-global
  atlas base (`AtomicPtr` + per-face offset/len over ONE concatenated blob);
  `set_atlas_base` installs a mapped VMO, else the `embedded-atlas` feature
  (default ON) lazily backs it with the linked blob. Host golden proves the
  runtime base is byte-identical to embedded; `ci-os-smp1` green — every
  current consumer keeps the embedded blob, zero behavior change. The
  build-separates-per-service check (scripts/build.sh `cargo build -p …`)
  confirms Phase 1 can drop the blob from app-host without unifying it back.)
- **Phase 1 (provisioning: shared atlas VMO, mapped RO)**: ⬜

Definition: “Complete” = the atlas exists as ONE shared read-only mapping;
opening N app windows adds ~0 atlas bytes (was ~4.25 MB each); proof = boot
`VMO-POOL` telemetry flat across an open/close storm + host golden parity.

## Scope boundaries (anti-drift)

- **This RFC owns**: how the baked glyph coverage atlas (~4.25 MB, RFC-0075 8d)
  is shared read-only across windowd + every app-host instead of embedded (and
  per-instance COPIED by `exec`) in each.
- **This RFC does NOT own**: the bake itself (RFC-0075 8d owns the charset +
  faces), `ui.font.family` live switching (separate follow-up), a runtime font
  cache (TASK-0202).

## Context

RFC-0075 8d bakes ~4.25 MB of A8 coverage (`font13.a8` 1.68 MB + `font16.a8`
2.57 MB) and `include_bytes!`s it into `nexus-text-baked` — so it lands in the
RO segment of EVERY binary that links the crate (windowd, app-host, and
dsl-runtime → app-host). Worse, `exec` COPIES each RO segment into the VMO
arena per launch (RFC-0075 8e), so N open app windows = N× ~4.25 MB of
identical atlas bytes resident. That drove the arena 160→224 MB and the kernel
heap 2→8 MiB as bridges; the recorded fix is to share ONE copy.

## Goals

- The atlas is ONE physical copy, mapped READ-ONLY into windowd + every
  app-host (a cloned VMO cap → same physical pages, `sys_map` READ-only).
- Opening/closing app windows adds ~0 atlas bytes (arena stays flat).
- Pixel-identical output (host goldens unchanged) — only the storage moves.

## Non-Goals

- A kernel change: cloning a `Vmo{base,len}` cap and RO-`map_page`-ing it into
  many address spaces already shares the physical pages (payload-VMO
  precedent). No new syscall.
- A general shared-RO-`exec`-segment optimization (would also dedup, but adds
  kernel page-refcount/teardown coupled to the RFC-0075 8f reclaim — riskier;
  out of scope, noted as an alternative).

## Constraints / invariants

- **Read-only sharing**: consumers map the atlas READ-only. The atlas VMO must
  never be mapped WRITE by an app-host (a shared-page corruption vector). The
  map is done by the trusted app-host RUNTIME, not app code; a RO-only VMO
  right is a hardening follow-up (today: convention + code review).
- **Pixel parity**: the resolved coverage bytes are byte-identical to the
  embedded blob — host goldens (`draw_text_row`, `measure`) unchanged.
- **No hot-path syscalls**: the atlas is MAPPED (direct reads), never
  `vmo_read` (a syscall per glyph would be fatal to render latency).
- **Fail-loud, never fake**: if the atlas VMO is missing/unmapped, an app-host
  MUST fail visibly (blank/`?`), never silently render garbage.

## Proposed design

### Phase 0 — library: runtime atlas base + data split (this RFC's foundation)

- `nexus-text-baked` `Face.cov: &'static [u8]` → resolved at runtime from a
  process-global **atlas base** (`AtomicPtr<u8>` + len) plus compile-time
  `(cov_offset, cov_len)` per face. `set_atlas_base(ptr, len)` installs it once.
- The big coverage blob moves behind an `embedded-atlas` feature: build.rs
  bakes ONE concatenated blob (`font13 ++ font16`) to a file and, under the
  feature, `include_bytes!`s it as `EMBEDDED_ATLAS` (the base auto-inits to it).
  The small metrics tables (`*_GLYPHS/EXTRAS/WIDE/KERN`, a few KB) stay embedded
  everywhere. Feature default ON keeps every current consumer byte-identical.
- Feature-unification discipline: app-host + dsl-runtime build the crate with
  `embedded-atlas` OFF (no blob in their images); windowd + the atlas OWNER
  build it ON. Because the blob lives ONLY under the feature, a consumer that
  doesn't enable it never links 4.25 MB.

### Phase 1 — provisioning: the shared atlas VMO

- An OWNER holds the atlas VMO once: `vmo_create(atlas_len)` + `vmo_write` from
  `EMBEDDED_ATLAS`, at startup. Candidate: **execd** (already creates the
  per-app payload VMO and grants per-spawn) owns the master; per app-host
  spawn it `cap_clone`s the atlas VMO and transfers it into the child's fixed
  slot (RO). windowd keeps its own embedded copy (single instance — 1 copy).
- app-host maps the granted atlas VMO READ-only, page-by-page, at a fixed VA
  (below the stack window, `vmo_map_page` loop — the smoltcp-queue precedent),
  then `set_atlas_base(va, len)` before the first render. No embedded blob.
- Result: 1 shared copy for ALL app-hosts + 1 for windowd, regardless of open
  window count (was N+1). Arena stays flat across an open/close storm.

## Proof

- Host (Phase 0): `nexus-text-baked` goldens with the base pointed at a heap
  buffer resolve byte-identically to the embedded path; `measure`/`draw`
  unchanged; determinism.
- QEMU (Phase 1): `VMO-POOL` used/peak telemetry stays flat while an open/close
  storm cycles ≥4 app windows; text still renders (CJK included); no
  `VMO-POOL exhausted`. Image size: app-host ELF shrinks ~4.25 MB.

## Follow-ups

- A RO-only VMO right (kernel) so a MAP cap cannot be mapped WRITE — hardens
  the shared atlas against a compromised app-host runtime.
- Zero-copy variant: a kernel “static VMO” over the atlas embedded ONCE in the
  kernel image (identity-mapped) — no owner copy at all. Bigger (kernel +
  linker + security); deferred.
- Once shared, the arena 224→smaller and heap 8→smaller bridges can shrink.
