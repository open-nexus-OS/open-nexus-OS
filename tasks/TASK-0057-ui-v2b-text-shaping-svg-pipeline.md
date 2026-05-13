---
title: TASK-0057 UI v2b: asset pipeline + theme system + SVG/PNG/JPG + text shaping + cursor pipeline
status: In Progress
owner: @ui
created: 2025-12-23
updated: 2026-05-13
depends-on:
  - TASK-0054
  - TASK-0056
  - TASK-0056C
follow-up-tasks:
  - TASK-0058
  - TASK-0059
  - TASK-0062
  - TASK-0063
  - TASK-0146
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC (contract seed): docs/rfcs/RFC-0056-ui-v2b-asset-theme-cursor-text-pipeline.md
  - UI v1a renderer (baseline): tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - UI v2a (present/input baseline): tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - UI v2a perf floor: tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md
  - Cursor themes: docs/dev/ui/foundations/visual/cursor-themes.md
  - Colors/tokens: docs/dev/ui/foundations/visual/colors.md
  - Materials: docs/dev/ui/foundations/visual/materials.md
  - Freedesktop icon spec: https://specifications.freedesktop.org/icon-theme-spec/
  - OHOS reference: docs/architecture/display-output-service-chain.md
---

## Context

TASK-0056C is In Review (120Hz, NonBlocking IPC, fastpath coalescing active in live OS path).
The display chain (`hidrawd→inputd→fbdevd→ramfb`) is responsive at 82-98Hz. Now we need real
content to make the UI usable before the Orbital-Level UX Gate (Launcher, Dock, real apps).

Current state:
- The visible proof surface shows colored rectangles as cursor/target placeholders.
- No text rendering, no SVG pipeline, no icon system, no theme engine.
- `docs/dev/ui/foundations/visual/` defines color tokens and cursor themes — docs-only.
- `make run` / `just start` already use a JPG wallpaper via `ramfb` bootstrap — proving
  image decode is a live dependency that nobody else will add before fast-path closure.

RFC-0056 defines the architecture contract: OHOS-aligned resource qualifier model,
freedesktop icon structure, BreezeX cursor pipeline, theme token engine.

This task is the complete asset/theme/cursor/text stack — everything needed for a real
Launcher by end of the UI fast lane.

## Goal

Deliver:

1. **Resource directory structure** (RFC-0056):
   - OHOS-aligned qualifier-based layout: `resources/themes/`, `icons/`, `cursors/`, `wallpapers/`, `fonts/`
   - freedesktop icon theme spec: `<ThemeName>/scalable/`, `<size>/`, `index.theme`

2. **Theme token engine** (`userspace/ui/theme/`):
   - `.nxtheme.toml` parser → runtime token resolver
   - Semantic tokens: `accent`, `bg`, `fg`, `surface`, `border`, `muted`, `danger`, etc.
   - Qualifier resolution: base → dark/light/highcontrast → density → locale
   - Schema validation; unknown keys rejected

3. **SVG rich subset** (`userspace/ui/svg/`):
   - Parse: `<svg>`, `<g>`, `<path>`, `<rect>`, `<circle>`, `<ellipse>`, `<line>`, `<polygon>`,
     `<defs>`, `<linearGradient>`, `<stop>`, basic transforms
   - Reject: scripts, external refs, filters, animations
   - Tessellate → BGRA8888 rasterizer
   - Bounded: max nodes, max path segments, max dimensions

4. **PNG / JPG pipeline** (`userspace/ui/image/`):
   - Decode PNG (deflate) and JPG (DCT) → RGBA8
   - Scale: bilinear + nearest-neighbor (deterministic)
   - Bounded: max decode size, decompression bomb detection
   - JPG wallpaper already used in live path — formalize as proper pipeline

5. **Text shaping** (`userspace/ui/shape/`):
   - HarfBuzz-based shaping with font fallback chain
   - Glyph cache: grayscale alpha bitmaps, bounded size, deterministic eviction
   - Host-first; OS path with pre-baked glyph atlases if HarfBuzz linking unavailable

6. **Cursor pipeline** (`userspace/ui/cursor/`):
   - Parse BreezeX SVG cursors → rasterize → BGRA8888 + hotspot
   - Integrate into `windowd` cursor display → visible on proof surface

7. **Proof surface targets**:
   - Real shaped text (multilingual LTR+RTL) rendered on shared proof surface
   - BreezeX SVG cursor visible as mouse pointer
   - SVG icon from freedesktop structure visible on proof surface
   - JPG wallpaper visible behind UI

8. **Host tests** (`tests/ui_v2b_host/`):
   - Text shaping goldens (JSON glyph runs)
   - SVG render goldens (PNG, pixel-exact or SSIM)
   - PNG/JPG decode goldens
   - Cursor render goldens
   - Theme resolution determinism

## Non-Goals

- Kernel changes
- Full SVG spec (animations, filters, scripts)
- LCD subpixel text
- GPU-accelerated rasterization
- Complete i18n/l10n locale switching (TASK-0174/0175)
- A detached demo scene — all assets must integrate into shared proof surface
- SVG-first wallpaper (JPG is acceptable for wallpaper)

## Constraints / invariants (hard requirements)

- Deterministic outputs: stable shaping, stable SVG/PNG/JPG rasterization, stable theme resolution
- Strict safety: SVG parser rejects unsupported features before rasterization
- Asset posture: launcher/SystemUI icons are SVG-sourced; PNG only as derived fallback
- JPG only for wallpaper; no JPG icons
- No `unwrap`/`expect` on untrusted input
- Bounded memory: caps on caches, image dimensions, SVG complexity
- freedesktop icon spec for directory structure

## Resource directory structure (normative)

``` text
resources/
├── themes/
│   ├── base.nxtheme.toml
│   ├── dark.nxtheme.toml
│   ├── light.nxtheme.toml
│   └── highcontrast.nxtheme.toml
├── icons/
│   └── <ThemeName>/               # e.g. "breezeX", "default"
│       ├── index.theme            # freedesktop manifest
│       ├── scalable/              # SVG (resolution-independent)
│       │   ├── apps/
│       │   ├── actions/
│       │   ├── status/
│       │   ├── devices/
│       │   ├── places/
│       │   ├── mimetypes/
│       │   └── cursors/
│       ├── 16x16/                 # PNG fallbacks
│       ├── 22x22/
│       ├── 32x32/
│       ├── 48x48/
│       ├── 64x64/
│       ├── 128x128/
│       └── 256x256/
├── cursors/
│   └── breezeX/
│       ├── base/                  # SVG cursor source
│       ├── dark/
│       └── light/
├── wallpapers/
│   ├── base/
│   │   └── default.jpg
│   ├── dark/
│   └── light/
├── fonts/
│   ├── inter/
│   ├── noto/
│   └── monospace/
└── sounds/                        # deferred
```

## Security considerations

### Threat model
- Malicious SVG: scripts, external refs, filters, unbounded path complexity
- Malformed fonts: buffer overflow, unbounded allocation
- Decompression bombs: oversized PNG/JPG
- Theme file injection: crafted `.nxtheme.toml` overriding tokens
- Glyph cache exhaustion: crafted text sequences

### Security invariants
- SVG parser rejects unsupported features before rasterization
- Image decoders have explicit size limits; decompression bombs detected
- Font parsing bounded with error propagation (no unwrap/expect)
- Theme files validated against schema; unknown keys rejected
- All caches have explicit capacity bounds
- Rasterization outputs carry no authority/identity signal

### DON'T DO
- Do not execute SVG scripts or process external references
- Do not accept PNG-first launcher/system icons
- Do not leak font paths, glyph metrics, or image metadata
- Do not use rasterization outputs for access control or identity

### Security proof expectation
- `test_reject_*` for: SVG scripts, SVG external refs, SVG filters, oversized fonts,
  malformed font headers, decompression-bomb images, invalid theme TOML
- Boundedness: glyph cache eviction, image decode limits, SVG node limits

## Red flags / decision points

- **YELLOW (HarfBuzz in OS)**: If OS-lite cannot link HarfBuzz, use pre-baked glyph atlases.
  *Neutralized*: Phase 1 starts host-first; Phase 2 OS path uses atlas fallback.
- **YELLOW (SVG complexity)**: Rich subset still allows complex paths.
  *Neutralized*: explicit node/segment limits in parser; `test_reject_*` for oversized input.
- **YELLOW (JPG codec in no_std)**: OS-lite JPG decode needs `no_std` library.
  *Neutralized*: JPG already used live in `ramfb` bootstrap; formalize existing path.
- **YELLOW (Wallpaper rendering perf)**: Full-screen JPG decode + scale at 120Hz cadence.
  *Neutralized*: decode once at boot, cache scaled bitmap; static wallpaper has zero per-frame cost.

## Stop conditions (Definition of Done)

### Proof (Host) — required

```bash
cargo test -p ui_v2b_host -- --nocapture
cargo test -p nexus-theme -- --nocapture
cargo test -p nexus-svg -- --nocapture
```

- Text shaping: multilingual LTR+RTL → stable glyph cluster ordering
- Glyph cache: repeated draws hit cache; eviction at configured cap
- SVG: rich subset parses; unsupported features rejected; renders match goldens
- PNG/JPG: decode + scale match goldens; oversized images rejected
- Cursor: BreezeX SVG → bitmap + hotspot correct
- Theme: token resolution deterministic; dark/light/highcontrast switch correct

### Proof (OS/QEMU) — required

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

- QEMU markers: `windowd: cursor svg loaded`, `windowd: text target visible`,
  `windowd: icon target visible`, `SELFTEST: ui v2b assets ok`
- Shared proof surface shows: real shaped text, BreezeX SVG cursor, SVG icon

### Visual proof handoff — required

- `just start` shows JPG wallpaper + real text target + SVG cursor + SVG icon
- Cursor switches from colored rectangle to BreezeX SVG asset
- Launcher/SystemUI proof surface uses SVG-source fixtures

## Touched paths (allowlist)

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

## Plan

1. **Resource directory + theme engine** — `.nxtheme.toml` parser, qualifier resolver, Runtime API
2. **SVG rich subset** — parser, tessellator, BGRA8888 rasterizer
3. **PNG/JPG pipeline** — decoder, scaler, bounded memory
4. **Text shaping** — HarfBuzz, font fallback, glyph cache
5. **Cursor pipeline** — BreezeX SVG → bitmap → windowd cursor asset
6. **Renderer integration** — `draw_glyph_run`, `draw_svg_path`, `draw_image`
7. **Proof surface** — text target + cursor target + icon target visible in QEMU
8. **Tests + docs** — goldens, tolerance policy, schema docs
