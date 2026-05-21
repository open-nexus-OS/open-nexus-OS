# Current State — Open Nexus OS

Last updated: 2026-05-20

## Active focus

**TASK-0059 Phasen 1–6a implementiert; Cache/Blur-Aussagen nachgeschaerft.**  
QEMU visible-bootstrap: Build, Boot, Display all OK in prior proof; current delta rechecked with host contracts + OS check.

---

## Implementiert (working tree, uncommitted)

### Phase 1: TileMap in Render-Loop
- `has_dirty_in_row_range`, band-skip in `write_rows`, clear-AFTER-write, all-tiles-marked in `write_current_frame`

### Phase 2: LayerCache API + sichere Population
- `insert()`/`get()`/`get_mut()`/`invalidate()`/`mark_clean()`, cache-check + blit in `draw_layout_box_row`
- `record_layer_cache_row` befuellt stabile, nicht-paint-abhaengige Boxen zeilenweise und markiert sie erst nach vollstaendiger Box clean
- Zustandsabhaengige Hover/Click/Scroll/Key/Filter-Boxen sowie Backdrop-Blur-Boxen werden nicht gecached, um stale UI-Zustaende zu vermeiden

### Phase 3: Echter Blur (Backdrop)
- `nexus_effects::blur_1d` importiert, `blur_backdrop_segment` ersetzt
- Erstframe nutzt `select_glass_quality(PROOF_PANEL_H)` statt `select_glass_quality(self.mode.height)`, damit der sichtbare Panel-Blur nicht zu `Opaque` degradiert

### Phase 4: Cursor-BG Save/Restore
- `save_cursor_bg_inline` (vor Cursor-Blend), `restore_cursor_bg` (bei Bewegung), verdrahtet

### Phase 5: Paint-Only Fast-Path
- `paint_only` durch `draw_proof_surface_row` -> `draw_layout_box_row` gereicht
- Nicht-Paint-Boxen uebersprungen, Backdrop-Blur bei `paint_only` deaktiviert

### Phase 6a: Shadow-Blur via Library
- `blur_row_horizontal` geloescht, durch `nexus_effects::blur_1d` ersetzt
- `blur_row_buf` Parameter in `compute_shadow_row` -> `_blur_row_buf`

### RISC-V Toolchain
- `rustup` + `nightly-2025-01-15` + `riscv64imac-unknown-none-elf` + `rust-src`
- `install-deps.sh`, `Containerfile`, `build.yml`, `ci.yml` aktualisiert

---

## QEMU Evidence (2026-05-20)

Mit `QEMU_DISPLAY_BACKEND=none`:

```
display: bootstrap on          OK
display: mode 1280x800         OK
display: first scanout ok      OK
windowd: present visible ok    OK
SELFTEST: ui visible wheel ok  OK
SELFTEST: ui v2b assets ok     OK
```

---

## Test status

| Check | Result |
|-------|--------|
| `cargo test -p windowd` | OK 31 passed |
| `cargo check -p windowd` | OK |
| `cargo check --target riscv64imac-unknown-none-elf --features os-lite` | OK |
| `just dep-gate` | OK |
| QEMU visible-bootstrap | OK Display-Marker |

---

## Not done

- [ ] Phase 6b/c: ShadowCache wiring (needs offscreen shadow rendering)
- [ ] True OS backdrop 2D blur via `blur_separable` / vertical pass; current OS path is budgeted row blur via `blur_1d`
- [ ] `neuron-boot.map` verify
- [ ] Commit

---

## Files changed

```
source/services/windowd/src/os_lite.rs
tools/nx/tests/interactive_os_startup.rs
scripts/install-deps.sh
podman/Containerfile
.github/workflows/build.yml
.github/workflows/ci.yml
CHANGELOG.md
.cursor/current_state.md
.cursor/handoff/current.md
```
