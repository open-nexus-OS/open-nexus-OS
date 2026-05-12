# Handoff — TASK-0057 (In Progress)

Date: 2026-05-12

## Summary

TASK-0057 builds the complete content/asset stack for the Orbital-Level UX Gate:
resource directory (OHOS qualifiers + freedesktop icons), theme engine (.nxtheme.toml),
SVG rich subset, PNG/JPG pipeline, HarfBuzz text shaping, and BreezeX cursor pipeline.
RFC-0056 defines the architecture contract.

Phase 0 (resource directory + theme engine) has not started yet. All implementation
is ahead.

## What was done

- TASK-0056C set to Done; handoff archived to `.cursor/handoff/archive/TASK-0056C-20260512.md`
- `.cursor/current_state.md` updated to reflect TASK-0056C Done, TASK-0057 In Progress
- RFC-0056 expanded to match `docs/rfcs/RFC-TEMPLATE.md`

## What remains

TASK-0057 plan (8 steps, none started):

1. Resource directory + theme engine — `.nxtheme.toml` parser, qualifier resolver, Runtime API
2. SVG rich subset — parser, tessellator, BGRA8888 rasterizer
3. PNG/JPG pipeline — decoder, scaler, bounded memory
4. Text shaping — HarfBuzz, font fallback, glyph cache
5. Cursor pipeline — BreezeX SVG → bitmap → windowd cursor asset
6. Renderer integration — `draw_glyph_run`, `draw_svg_path`, `draw_image`
7. Proof surface — text target + cursor target + icon target visible in QEMU
8. Tests + docs — goldens, tolerance policy, schema docs

### Touched paths (allowlist)

- `resources/` (new: themes, icons, cursors, wallpapers, fonts)
- `userspace/ui/theme/` (new)
- `userspace/ui/svg/` (new)
- `userspace/ui/image/` (new)
- `userspace/ui/shape/` (new)
- `userspace/ui/cursor/` (new)
- `userspace/ui/renderer/` (extend: draw_glyph_run, draw_svg, draw_image)
- `source/services/windowd/` (extend: cursor asset loading)
- `tests/ui_v2b_host/` (new)
- `docs/dev/ui/foundations/layout/text.md`
- `docs/dev/ui/foundations/rendering/svg.md`
- `docs/dev/ui/foundations/rendering/image.md`

## Proofs

None yet. Expected proofs:

```bash
# Host
cargo test -p ui_v2b_host -- --nocapture
cargo test -p nexus-theme -- --nocapture
cargo test -p nexus-svg -- --nocapture

# OS/QEMU
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Required QEMU markers:
- `windowd: cursor svg loaded`
- `windowd: text target visible`
- `windowd: icon target visible`
- `SELFTEST: ui v2b assets ok`

## Open threads / risks

- HarfBuzz in no_std: Phase 1 host-first; OS path uses pre-baked glyph atlases if linking unavailable
- JPG codec in no_std: formalize existing ramfb bootstrap path
- SVG complexity: bounded node/segment limits; `test_reject_*` for oversized input
- DON'T add prints/logs/markers in kernel

## Next task

Continue with downstream UI tasks after TASK-0057 closes:
- TASK-0059 (scroll, clip, effects, IME/text-input)
- TASK-0062 (animation/runtime)
- TASK-0063 (virtualized list, theme tokens)
- TASK-0064 (window management, scene transitions)

## Files changed (this cycle)

- `.cursor/handoff/current.md` (this file)
- `.cursor/current_state.md`
- `tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md` (status → Done)
- `docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md` (expanded to template)
