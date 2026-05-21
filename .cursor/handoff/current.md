# Handoff — TASK-0059 Phases 1–6a (in progress)

Date: 2026-05-20
Session: implementation

## Status

| Item | State |
|------|--------|
| Phase 1: TileMap | Done |
| Phase 2: LayerCache API + safe population | Done for static non-paint boxes |
| Phase 3: Backdrop Blur (library) | Done |
| Phase 4: Cursor Save/Restore | Done |
| Phase 5: Paint-Only Fast-Path | Done |
| Phase 6a: Shadow Blur (library) | Done |
| Phase 6b/c: ShadowCache | Later (needs offscreen) |
| Backdrop 2D `blur_separable` in OS path | Later |
| RISC-V Toolchain | Done |
| QEMU visible-bootstrap | Build+Boot+Display OK |

## This session — completed

### os_lite.rs (~250 lines)
- TileMap wired into render loop (dirty-tile gating)
- LayerCache promoted from API-only to functional for stable, non-paint boxes (`record_layer_cache_row` + clean blit)
- First-frame glass budget uses `PROOF_PANEL_H`, so the bootstrap frame no longer forces full-screen `Opaque`
- Both blur paths (backdrop + shadow) switched from inline to nexus_effects::blur_1d
- Cursor-BG save/restore activated from dead fields
- Paint-only fast-path: non-paint boxes + backdrop blur skipped
- Dead code removed: blur_row_horizontal (inline sliding-window blur)

### Infrastructure
- rustup + nightly-2025-01-15 + riscv64imac + rust-src installed
- install-deps.sh, Containerfile, build.yml, ci.yml updated
- flake.nix: no change needed (reads rust-toolchain.toml)

### QEMU
- Build: 0 errors
- Boot: all display markers present (bootstrap on through v2b assets ok)
- Headless mode: QEMU_DISPLAY_BACKEND=none

## Next steps

1. Phase 6b/c: Implement offscreen shadow rendering + ShadowCache (needs full-box capture, not per-row)
2. Wire true OS backdrop 2D blur (`blur_separable`/vertical pass) behind the same budget gates
3. Fix `SELFTEST: display bootstrap guest ok` for auto-exit
4. Commit (split: os_lite changes vs infra changes)

## Resume commands

```bash
cargo test -p windowd
cargo check -p windowd --target riscv64imac-unknown-none-elf --features os-lite
just dep-gate
QEMU_DISPLAY_BACKEND=none RUN_UNTIL_MARKER=1 bash scripts/qemu-test.sh --profile visible-bootstrap
```
