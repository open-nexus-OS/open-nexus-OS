# RFC-0056: UI v2b asset pipeline + theme system + cursor/text contract seed

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-05-12
- Last Updated: 2026-05-12
- Links:
  - Tasks: `tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
    - `docs/rfcs/RFC-0055-ui-v2a-embedded-reactor-runtime-floor-present-input-perf-contract.md`
    - `docs/rfcs/RFC-0046-*.md`

## Status at a Glance

- **Phase 0 (resource manager + theme tokens)**: ⬜
- **Phase 1 (SVG + PNG/JPG + text shaping)**: ⬜
- **Phase 2 (cursor pipeline + proof surface integration)**: ⬜

Definition:

- "Complete" means the contract is defined AND the proof gates are green (host tests pass, QEMU markers fire). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - resource directory layout (OHOS qualifiers + freedesktop icon spec)
  - theme token schema (`.nxtheme.toml`) and qualifier resolution rules
  - SVG rich subset grammar (allowed elements, attributes, transforms; what is rejected)
  - PNG/JPG decode + scale pipeline with bounded memory contract
  - HarfBuzz text shaping contract (glyph cache bounds, font fallback chain)
  - BreezeX cursor pipeline (SVG source → hotspot bitmap → windowd integration)
  - proof surface targets: shaped text, SVG cursor, SVG icon all visible on shared surface
  - golden test contract (pixel-exact or SSIM tolerance, deterministic outputs)
- **This RFC does NOT own**:
  - full SVG spec (animations, filters, scripts, external refs)
  - LCD subpixel text rendering
  - GPU-accelerated rasterization
  - complete i18n/l10n locale switching (TASK-0174/0175)
  - scroll, clip, effects, IME/text-input (TASK-0059)
  - animation/runtime (TASK-0062)
  - virtualized lists, theme token consumption (TASK-0063)
  - window management, scene transitions (TASK-0064)

### Relationship to tasks (single execution truth)

- `TASK-0057` is the execution SSOT for this contract seed.
- `TASK-0057` must implement all phases and prove them via host tests + QEMU markers.

## Context

TASK-0056C established the 120Hz present/input floor. The display chain
(`hidrawd → inputd → fbdevd → ramfb`) is responsive at 82-98Hz. Now we need real
content to make the UI usable before the Orbital-Level UX Gate (Launcher, Dock,
real apps).

Current state:
- The visible proof surface shows colored rectangles as cursor/target placeholders.
- No text rendering, no SVG pipeline, no icon system, no theme engine.
- `docs/dev/ui/foundations/visual/` defines color tokens and cursor themes — docs-only.
- `make run` / `just start` already use a JPG wallpaper via `ramfb` bootstrap — proving
  image decode is a live dependency.

## Goals

1. OHOS-aligned resource directory with qualifier-based layout
2. Theme token engine: `.nxtheme.toml` parser → runtime token resolver with qualifier resolution
3. SVG rich subset: parse → tessellate → BGRA8888 rasterizer (reject scripts, external refs, filters)
4. PNG/JPG decode + scale pipeline with decompression bomb detection
5. HarfBuzz text shaping with font fallback chain and bounded glyph cache
6. BreezeX cursor pipeline: SVG cursor → rasterize → BGRA8888 + hotspot → windowd integration
7. Proof surface: real shaped text + BreezeX SVG cursor + SVG icon + JPG wallpaper visible

## Non-Goals

- Full SVG spec (animations, filters, scripts — explicitly rejected)
- LCD subpixel text rendering
- GPU-accelerated rasterization
- Complete i18n/l10n locale switching (TASK-0174/0175)
- A detached demo scene — all assets must integrate into shared proof surface
- SVG-first wallpaper (JPG is acceptable for wallpaper)
- Kernel changes

## Constraints / invariants (hard requirements)

- **Determinism**: stable shaping, stable SVG/PNG/JPG rasterization, stable theme resolution.
  No timing-fluke "usually ok". Golden tests must be pixel-exact or SSIM-bounded.
- **Bounded resources**: explicit caps on glyph cache size, image dimensions (max decode W×H),
  SVG node count, SVG path segment count, theme file size. Decompression bomb detection.
- **Security floor**: SVG parser rejects unsupported features BEFORE rasterization.
  No `unwrap`/`expect` on untrusted input. Theme files schema-validated; unknown keys rejected.
- **Asset posture**: launcher/SystemUI icons are SVG-sourced; PNG only as derived fallback.
  JPG only for wallpaper; no JPG icons.
- **No fake success**: never emit `windowd: cursor svg loaded` etc. unless the real asset
  was parsed and rasterized.
- **Stubs policy**: any stub must be explicitly labeled, non-authoritative, and must not
  claim success.

## Proposed design

### Contract / interface (normative)

#### Theme token schema (`.nxtheme.toml`)

```toml
[theme]
name = "base"
version = 1

[tokens]
accent = "#3b82f6"
bg = "#ffffff"
fg = "#1a1a2e"
surface = "#f8f9fa"
border = "#e2e8f0"
muted = "#94a3b8"
danger = "#ef4444"
# ... extensible token set
```

- Qualifier resolution order: base → dark/light/highcontrast → density → locale
- Missing qualifier levels fall back to base
- Unknown keys at any level → rejection (schema validation)
- Runtime API: `Theme::resolve("accent") → RGBA8`, `Theme::active_qualifier() → Qualifier`

#### SVG rich subset (allowed grammar)

Allowed: `<svg>`, `<g>`, `<path>`, `<rect>`, `<circle>`, `<ellipse>`, `<line>`,
`<polygon>`, `<defs>`, `<linearGradient>`, `<stop>`, basic transforms (`translate`,
`scale`, `rotate`, `matrix`).

Rejected: `<script>`, `<foreignObject>`, `<use>` (external), `<filter>`, `<animate>`,
`<animateTransform>`, `<set>`, `url()` references to external files, `data:` URI.

Bounded: max nodes (default 4096), max path segments (default 16384), max dimensions
(default 2048×2048).

#### BreezeX cursor pipeline API

```
CursorSet::load("breezeX/base") → HashMap<CursorName, CursorAsset>
CursorAsset { frames: Vec<CursorFrame>, hotspot: (u16, u16) }
CursorFrame { rgba: BGRA8888, width: u16, height: u16, delay_ms: u16 }
```

#### Text shaping API

```
ShapeContext::new(font_dir) → ShapeContext
ShapeContext::shape(text, attributes) → GlyphRun
GlyphRun { glyphs: Vec<Glyph>, cluster_map: Vec<u32> }
Glyph { index: u32, x: i32, y: i32, advance: i32 }
```

Glyph cache: bounded size (configurable, default 4096 entries). Deterministic LRU
eviction on overflow.

### Phases / milestones (contract-level)

- **Phase 0**: resource directory structure created. `.nxtheme.toml` parser with
  schema validation. Theme Runtime API resolves tokens through qualifier chain.
  Proof: `cargo test -p nexus-theme` passes.
- **Phase 1**: SVG rich subset parser + tessellator + rasterizer. PNG/JPG decode +
  scale. HarfBuzz text shaping + glyph cache. Security reject tests for all three
  pipelines. Proof: `cargo test -p ui_v2b_host` passes.
- **Phase 2**: BreezeX cursor loading + rasterization + windowd integration. Proof
  surface shows text target + cursor target + icon target in QEMU. Proof:
  `RUN_UNTIL_MARKER=1 just test-os` passes with all markers fired.

## Security considerations

- **Threat model**:
  - Malicious SVG: scripts, external refs, filters, unbounded path complexity
  - Malformed fonts: buffer overflow, unbounded allocation
  - Decompression bombs: oversized PNG/JPG (e.g. 1×1 pixel claiming 100000×100000)
  - Theme file injection: crafted `.nxtheme.toml` overriding tokens
  - Glyph cache exhaustion: crafted text sequences forcing eviction churn
- **Mitigations**:
  - SVG parser rejects unsupported features before rasterization (deny-list at parse time)
  - Image decoders enforce explicit W×H limits; decompression ratio checked against output buffer
  - Font parsing bounded with error propagation (no `unwrap`/`expect`)
  - Theme files validated against schema; unknown keys rejected
  - All caches have explicit capacity bounds with deterministic eviction
  - Rasterization outputs carry no authority/identity signal
- **DON'T DO**:
  - Do not execute SVG scripts or process external references
  - Do not accept PNG-first launcher/system icons
  - Do not leak font paths, glyph metrics, or image metadata
  - Do not use rasterization outputs for access control or identity
- **Security proof expectation**:
  - `test_reject_*` for: SVG scripts, SVG external refs, SVG filters, oversized fonts,
    malformed font headers, decompression-bomb images, invalid theme TOML
  - Boundedness: glyph cache eviction, image decode limits, SVG node limits

## Failure model (normative)

Every pipeline must return `Result<T, Error>` with explicit, non-overlapping error
variants. No silent fallback: if a fallback exists, it must be explicit and proven.

| Pipeline | Error conditions | Required behavior |
|---|---|---|
| Theme | Parse error, schema violation, unknown key | Reject file; return error |
| Theme | Qualifier chain incomplete | Fall back to base; log (host) / marker (OS) |
| SVG | Unsupported element/attribute | Reject before tessellation |
| SVG | Node/segment limit exceeded | Reject; return `SvgError::ComplexityExceeded` |
| PNG/JPG | Decompression bomb detected | Reject; return `ImageError::DecompressionBomb` |
| PNG/JPG | Decode failure | Return `ImageError::Decode(...)`; no partial bitmap |
| Text | Font not found | Skip glyph; fallback to next font in chain |
| Text | Glyph cache full | Evict LRU; bounded by configured cap |
| Cursor | SVG parse/rasterize failure | Return error; windowd keeps prior cursor |

## Proof / validation strategy (required)

### Proof (Host)

```bash
cargo test -p ui_v2b_host -- --nocapture
cargo test -p nexus-theme -- --nocapture
cargo test -p nexus-svg -- --nocapture
```

Expected:
- Text shaping: multilingual LTR+RTL → stable glyph cluster ordering
- Glyph cache: repeated draws hit cache; eviction at configured cap
- SVG: rich subset parses; unsupported features rejected; renders match goldens
- PNG/JPG: decode + scale match goldens; oversized images rejected
- Cursor: BreezeX SVG → bitmap + hotspot correct
- Theme: token resolution deterministic; dark/light/highcontrast switch correct

### Proof (OS/QEMU)

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Required markers:
- `windowd: cursor svg loaded`
- `windowd: text target visible`
- `windowd: icon target visible`
- `SELFTEST: ui v2b assets ok`

### Visual proof handoff

- `just start` shows JPG wallpaper + real text target + SVG cursor + SVG icon
- Cursor switches from colored rectangle to BreezeX SVG asset
- Launcher/SystemUI proof surface uses SVG-source fixtures

## Alternatives considered

- **PNG-first icons instead of SVG-first**: rejected. Launcher/SystemUI icons must
  be resolution-independent. SVG-sourced with PNG derived fallbacks at fixed sizes
  follows freedesktop conventions.
- **Own text shaping instead of HarfBuzz**: rejected. HarfBuzz is the industry
  standard for complex text layout (LTR/RTL, ligatures, script-specific rules).
  Custom shaping would duplicate decades of work. OS-lite fallback uses pre-baked
  glyph atlases, not a custom shaper.
- **TinyVG or NanoSVG instead of custom SVG subset**: considered. Rejected because
  we need bounded, deterministic, no_std-compatible processing. A custom rich subset
  parser gives precise control over what is accepted/rejected.
- **JPG everywhere instead of JPG-only-for-wallpaper**: rejected. Icons and UI
  elements need alpha and lossless reproduction. PNG for raster fallbacks, SVG for
  primary assets. JPG is acceptable only for wallpaper (no alpha needed,
  lossy-friendly content).
- **Theme engine as runtime plugin**: rejected for now. Static `.nxtheme.toml`
  parsing at boot is sufficient. Hot-reload of themes is deferred to TASK-0063.
- **LCD subpixel text**: rejected. Requires knowledge of physical pixel layout.
  Grayscale antialiasing is portable across displays and consistent with no_std
  constraints.

## Open questions

- **HarfBuzz in OS-lite**: Can we link HarfBuzz in the no_std OS path? If not,
  pre-baked glyph atlases must cover the needed codepoints. Owner: @runtime.
  Decision needed before Phase 2.
- **JPG codec in no_std**: Which library for JPG decode in the OS path? The existing
  ramfb bootstrap path uses host-side decode. Owner: @ui. Decision needed before
  Phase 2.
- **SVG golden tolerance**: pixel-exact or SSIM > 0.99? SSIM allows minor
  antialiasing differences across platforms. Owner: @ui. Decide during Phase 1 test
  development.
- **Cursor animation frames**: BreezeX cursors may have multi-frame (animated)
  definitions. Phase 2 scope is single-frame static cursors. Multi-frame deferred
  to TASK-0062.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: Resource directory + theme engine (`resources/`, `userspace/ui/theme/`).
  `.nxtheme.toml` parser with schema validation, qualifier resolver, Runtime API.
  Proof: `cargo test -p nexus-theme -- --nocapture`
- [ ] **Phase 1a**: SVG rich subset (`userspace/ui/svg/`). Parser, tessellator, BGRA8888 rasterizer.
  Security reject tests. Proof: `cargo test -p nexus-svg -- --nocapture`
- [ ] **Phase 1b**: PNG/JPG pipeline (`userspace/ui/image/`). Decoder, scaler, decompression bomb detection.
  Proof: `cargo test -p ui_v2b_host image:: -- --nocapture`
- [ ] **Phase 1c**: Text shaping (`userspace/ui/shape/`). HarfBuzz, font fallback, glyph cache.
  Proof: `cargo test -p ui_v2b_host shape:: -- --nocapture`
- [ ] **Phase 2a**: Cursor pipeline (`userspace/ui/cursor/`). BreezeX SVG → bitmap → windowd asset.
  Proof: `cargo test -p ui_v2b_host cursor:: -- --nocapture`
- [ ] **Phase 2b**: Renderer integration + proof surface. `draw_glyph_run`, `draw_svg_path`,
  `draw_image` in renderer. Proof: `cargo test -p ui_v2b_host -- --nocapture`
- [ ] **Phase 2c**: QEMU markers + visual proof. `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`.
  Markers: `cursor svg loaded`, `text target visible`, `icon target visible`, `SELFTEST: ui v2b assets ok`.
- [ ] Task linked with stop conditions + proof commands (TASK-0057).
- [ ] Security-relevant negative tests exist (`test_reject_*` for SVG scripts/refs/filters,
  malformed fonts, decompression bombs, invalid theme TOML).
- [ ] `docs/rfcs/README.md` updated with RFC-0056 status.
