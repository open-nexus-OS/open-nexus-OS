# Current State — Open Nexus OS

Last updated: 2026-05-21

## Active focus

**TASK-0059 / RFC-0058: ShadowCache heap exhaustion behoben.**
Phase 7 (Compositor Retained-Mode Upgrades) implementiert, aber ShadowCache per-row `to_vec()`
erschöpft den Bump-Allocator bei echtem Display-Betrieb → `alloc-fail` → windowd exit.
QEMU mit `DISPLAY_BACKEND=none` hat den Rendering-Pfad nie getestet (Testing-Gap).

**Nächster Schritt:** P1 (Shadow per Box) + P2 (Zero-Alloc 2D Blur) als kombinierter Fix:
ShadowArena (pre-allocated) + Per-Box-Caching + blur_separable_zero_alloc.

---

## Implementiert (uncommitted)

### Phase 1: TileMap in Render-Loop
- `has_dirty_in_row_range`, band-skip, clear-AFTER-write, all-tiles-marked

### Phase 2: LayerCache API
- `insert()`/`get()`/`get_mut()`/`invalidate()`, `record_layer_cache_row()` with `rows_filled`
- Cache-check + blit in `draw_layout_box_row`, budget (4KB total, 1KB/layer)

### Phase 3: Backdrop Blur
- Switched to library `blur_1d` → reverted (alloc-fail on bump allocator)
- Restored zero-allocation `blur_backdrop_segment` + `blur_row_horizontal`

### Phase 4: Cursor-BG Save/Restore
- `save_cursor_bg_inline` (before blend), `restore_cursor_bg` (on move)
- `cursor_bg_saved` field was dead — now active

### Phase 5: Paint-Only Fast-Path
- Non-paint boxes skipped, `combined_panels` excluded, backdrop always active

### Phase 6a: ShadowCache + Zero-Alloc Blur
- `ShadowCache` (256-entry LRU) imported and wired
- Per-row cache key: `(box_id_hash << 32) | (rel_y << 16) | blur_r`
- **BEKANNTES PROBLEM:** `to_vec()` in `compute_shadow_row` + `Vec<u8>` in `CachedShadow`
  allokieren pro unique Shadow-Row auf dem Bump-Allocator → ~400KB pro Vollbild
  → `alloc-fail` nach ~500 Rows bei 512KB Heap. Nur sichtbar mit echtem Display.

### Rect-Based Damage (P0/P1)
- Row ranges removed, `queue_dirty_rect` with precise `DamageRect`
- `write_damage_rect` accepts `glass_quality` + `paint_only`
- Filter rects added to `LayoutHotPathIndex`
- Shadow/backdrop always rendered (no overwrite)
- vmo_write batching in 4-row bands
- ~150 lines deleted (`pending_damage_rows`, `queue_rows`, etc.)

### RISC-V Toolchain
- `rustup` + `nightly-2025-01-15` + `riscv64imac-unknown-none-elf` + `rust-src`
- `install-deps.sh`, `Containerfile`, `build.yml`, `ci.yml` updated

---

## Production-Grade Delta: What Remains

| Prio | Gap | Impact | Fix |
|------|-----|--------|-----|
| **P0** | **ShadowCache heap exhaustion** | **Crash (alloc-fail → windowd exit)** | ShadowArena (pre-alloc) + Per-Box-Caching |
| P0 | Mouse flicker (3-frame cursor) | Not smooth | Atomic cursor update or double-buffer |
| P0 | vmo_write per row | Line-by-line visible | Single vmo_write per rect |
| P1 | Backdrop cached layer | 440x more blur ops | Capture + blur once + cache |
| P2 | Double-buffer / VSync | Partial frames | Back-buffer flip, kernel signaling |
| — | Testing-Gap: kein Display | Rendering-Pfad ungetestet | QEMU mit Display-Backend im CI |

**P1 (Shadow per Box) und P2 (Zero-Alloc 2D Blur) wurden zu P0 eskaliert** — sie sind
nicht nur Performance/Quality, sondern verursachen einen Korrektheits-Bug auf dem
Bump-Allocator. Der Fix ist: ShadowArena + Per-Box-Caching + blur_separable_zero_alloc.

---

## QEMU Evidence (2026-05-21)

`QEMU_DISPLAY_BACKEND=none bash scripts/qemu-test.sh --profile full`:
- `SELFTEST: end`, `verify-uart: profile=full clean`
- Build: 0 errors, 8 warnings

**Achtung:** `DISPLAY_BACKEND=none` testet den Rendering-Pfad NICHT.
`write_current_frame()` und `compute_shadow_row()` werden nie aufgerufen.
Der ShadowCache-Leak ist nur mit echtem Display reproduzierbar.

## Test Status

| Suite | Count | Result |
|-------|-------|--------|
| windowd lib | 22 | OK |
| headless | 9 | OK |
| damage_pipeline | 2 | OK |
| OS check (RISC-V) | — | OK 0 errors |
| QEMU full (DISPLAY_BACKEND=none) | — | OK `SELFTEST: end` |
| QEMU mit Display | — | **CRASH** (alloc-fail in windowd) |

## Files Changed

```
source/services/windowd/src/os_lite.rs
source/services/windowd/src/live_runtime.rs
source/services/windowd/tests/damage_pipeline.rs (new)
tools/nx/tests/interactive_os_startup.rs
scripts/install-deps.sh
podman/Containerfile
.github/workflows/build.yml
.github/workflows/ci.yml
CHANGELOG.md
docs/rfcs/RFC-0058-*.md
docs/architecture/display-output-service-chain.md
tasks/TASK-0059-*.md
.cursor/current_state.md
.cursor/handoff/current.md
```
