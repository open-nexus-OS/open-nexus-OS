# RFC-0056: UI v2b asset pipeline + theme system + cursor/text contract seed

- Status: Done
- Last Updated: 2026-05-15 (TASK-0057 Minimal DisplayServer v0 actual state)
- Owners: @ui @runtime
- Created: 2026-05-12
- Links:
  - Tasks: `tasks/TASK-0057-ui-v2b-text-shaping-svg-pipeline.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
    - `docs/rfcs/RFC-0055-ui-v2a-embedded-reactor-runtime-floor-present-input-perf-contract.md`
    - `docs/rfcs/RFC-0046-*.md`

## Status at a Glance

- **Phase 0 (resource manager + theme tokens)**: ✅
- **Phase 1 (SVG + PNG/JPG + text shaping)**: ✅ host-first, OS proof overlay generated from Inter
- **Phase 2 (cursor pipeline + proof surface integration)**: ✅ Mocu SVG cursor + v2b proof scene
- **Phase 3 (service-owned live display chain)**: ✅ `hidrawd -> inputd -> windowd -> fbdevd -> ramfb`
- **Phase 4 (manifest discovery)**: ✅ implemented for current services via resource manifests and service ordering
- **Phase 5 (Minimal DisplayServer v0)**: ✅ `windowd` is a standalone os-lite service and scene authority

Definition:

- "Complete" means the contract is defined AND the proof gates are green (host tests pass, QEMU markers fire). It does not mean "never changes again".
- "Implementation in progress" means the v2b slice is behaviorally wired, but broader production display features such as GPU, full IME, multi-window WM, and full HarfBuzz-in-OS remain out of scope.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - resource directory layout (locale/density qualifiers + freedesktop icon spec)
  - theme token schema (`.nxtheme.toml`) and qualifier resolution rules
  - SVG rich subset grammar (allowed elements, attributes, transforms; what is rejected)
  - PNG/JPG decode + scale pipeline with bounded memory contract
  - HarfBuzz text shaping contract (glyph cache bounds, font fallback chain)
  - Mocu cursor pipeline (SVG source → hotspot bitmap → `windowd` integration)
  - proof surface targets: Inter-rendered text, SVG cursor, SVG icon, and JPG wallpaper visible on one `windowd` scene
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

TASK-0056C established the present/input floor. During TASK-0057 the original
library-style display path was promoted into a service-owned Minimal
DisplayServer v0 chain:

``` text
hidrawd -> inputd -> windowd -> fbdevd -> ramfb
```

Current implementation state:

- `windowd` is the DisplayServer authority. It receives visible-input state from
  `inputd`, receives a framebuffer VMO capability from `fbdevd`, composes rows,
  and owns cursor/wallpaper/text/icon pixels.
- `fbdevd` is scanout-only. It configures `ramfb`, registers the framebuffer VMO
  with `windowd`, and reports visible-state evidence after observing service
  state from `windowd`.
- `selftest-client` is observer-only. It does not render, synthesize final
  success, or own display/input authority.
- The visible scene uses the full 1280x800 bootstrap mode, a JPEG wallpaper from
  `resources/wallpapers/base/default.jpeg`, a normalized Mocu cursor from
  `resources/cursors/mocu/src/svg/default.svg`, and Inter proof text generated
  from `resources/fonts/inter/docs/font-files/InterVariable.ttf`.
- Hover/click/key/scroll targets are transient service state: hover only while
  the routed pointer is over the target, click only while the primary button is
  held, keyboard only while a non-modifier key is held, and scroll up/down pulses
  are distinguishable and expire on a bounded `inputd` tick.

## Goals

1. Qualifier-based resource directory layout
2. Theme token engine: `.nxtheme.toml` parser → runtime token resolver with qualifier resolution
3. SVG rich subset: parse → tessellate → BGRA8888 rasterizer (reject scripts, external refs, filters)
4. PNG/JPG decode + scale pipeline with decompression bomb detection
5. HarfBuzz text shaping with font fallback chain and bounded glyph cache
6. Mocu cursor pipeline: SVG cursor → rasterize → BGRA8888 + hotspot → windowd integration
7. Proof surface: real shaped text + Mocu SVG cursor + SVG icon + JPG wallpaper visible
8. `windowd`-owned live cursor composition: cursor bitmap and position are
   composed in the DisplayServer, not in `fbdevd`
9. Manifest/resource discovery: service manifests provide resources and
   deterministic service ordering for the live chain
10. `windowd` as standalone IPC service: cap-based IPC, scene composition,
    cursor tracking, framebuffer VMO registration, `fbdevd` scanout-only

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

#### Mocu cursor pipeline API

``` text
CursorSet::load("mocu") → HashMap<CursorName, CursorAsset>
CursorAsset { frames: Vec<CursorFrame>, hotspot: (u16, u16) }
CursorFrame { rgba: BGRA8888, width: u16, height: u16, delay_ms: u16 }
```

TASK-0057 OS-lite implementation note: `windowd` currently normalizes
`resources/cursors/mocu/src/svg/default.svg` into the bounded SVG subset at build
time because the upstream Mocu source uses `<defs>`, `<use>`, style attributes,
and stroke semantics that the minimal OS rasterizer does not yet implement at
production quality. The normalized cursor preserves the Mocu colors
(`#0a0b0c` shadow, `#1a1b1c` stroke, `#fafbfc` fill), uses a 32x32 canvas, and
keeps the source hotspot at `(2, 2)` after scaling. The golden test must reject a
32px canvas that contains a collapsed glyph.

#### Text shaping API

``` text
ShapeContext::new(font_dir) → ShapeContext
ShapeContext::shape(text, attributes) → GlyphRun
GlyphRun { glyphs: Vec<Glyph>, cluster_map: Vec<u32> }
Glyph { index: u32, x: i32, y: i32, advance: i32 }
```

Glyph cache: bounded size (configurable, default 4096 entries). Deterministic LRU
eviction on overflow.

TASK-0057 OS-lite implementation note: host text shaping remains in
`userspace/ui/shape`. The current visible OS proof does not link HarfBuzz into
the OS image; instead, `windowd` build-time rasterizes an Inter proof overlay
from `resources/fonts/inter/docs/font-files/InterVariable.ttf` using `fontdue`.
This is a real Inter-derived text asset, not the former hardcoded bitmap atlas.
Full runtime shaping in OS remains follow-up scope.

### Phases / milestones (contract-level)

- **Phase 0**: resource directory structure created. `.nxtheme.toml` parser with
  schema validation. Theme Runtime API resolves tokens through qualifier chain.
  Proof: `cargo test -p nexus-theme` passes.
- **Phase 1**: SVG rich subset parser + tessellator + rasterizer. PNG/JPG decode +
  scale. HarfBuzz text shaping + glyph cache. Security reject tests for all three
  pipelines. Proof: `cargo test -p ui_v2b_host` passes.
- **Phase 2**: Mocu cursor loading + rasterization + `windowd` integration. Proof
  surface shows text target + cursor target + icon target in QEMU. Proof:
  `RUN_UNTIL_MARKER=1 just test-os` passes with all markers fired.
- **Phase 3**: Service-owned live display chain. `inputd` forwards bounded
  visible-input state to `windowd`; `windowd` composes cursor/proof-scene rows;
  `fbdevd` stays scanout-only. Proof: `cargo test -p fbdevd -- --nocapture` +
  QEMU marker `fbdevd: cursor overlay on`.
- **Phase 4**: Manifest/resource discovery. Service/resource manifests avoid
  adding the same service in multiple hardcoded locations and maintain
  deterministic boot order for `windowd`, `inputd`, and `fbdevd`.
- **Phase 5**: `windowd` as standalone IPC service. `inputd` sends visible state
  via `OP_UPDATE_VISIBLE_STATE`; `fbdevd` registers the framebuffer VMO with
  `windowd`; `windowd` writes composed rows directly into that VMO. Service
  contract tests per hop. Proof: `cargo test -p windowd -p fbdevd -p inputd` +
  QEMU marker `windowd: cursor svg loaded` + `fbdevd: cursor overlay on`.

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
cargo test -p nexus-svg --test cursor_golden -- --nocapture
```

Expected:
- Text shaping: multilingual LTR+RTL → stable glyph cluster ordering; OS proof
  overlay is generated from InterVariable.ttf
- Glyph cache: repeated draws hit cache; eviction at configured cap
- SVG: rich subset parses; unsupported features rejected; renders match goldens
- PNG/JPG: decode + scale match goldens; oversized images rejected
- Cursor: normalized Mocu SVG → bitmap + hotspot correct, with a 32px cursor
  using practical on-screen extents rather than a narrow 24px glyph
- Theme: token resolution deterministic; dark/light/highcontrast switch correct

### Proof (OS/QEMU)

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

Required markers:
- `display: mode 1280x800 argb8888`
- `windowd: wallpaper visible`
- `windowd: cursor svg loaded`
- `windowd: text target visible`
- `windowd: icon target visible`
- `fbdevd: cursor overlay on`
- `SELFTEST: ui v2b assets ok`

### Visual proof handoff

- `just start` shows full-resolution JPG wallpaper, Inter text target, Mocu SVG
  cursor, SVG icon/proof targets, and live pointer movement.
- Hover/click/key targets are not permanent latches; scroll up/down are visually
  distinguishable and expire after the bounded pulse window.
- `selftest-client` remains observer-only; visual success must come from
  service-owned `windowd`/`fbdevd` state.

## Alternatives considered

- **PNG-first icons instead of SVG-first**: rejected. Launcher/SystemUI icons must
  be resolution-independent. SVG-sourced with PNG derived fallbacks at fixed sizes
  follows freedesktop conventions.
- **Own text shaping instead of HarfBuzz**: rejected. HarfBuzz is the industry
  standard for complex text layout (LTR/RTL, ligatures, script-specific rules).
  Custom shaping would duplicate decades of work. TASK-0057's OS-lite proof uses
  build-time Inter rasterization, not a custom shaper or hardcoded atlas.
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

- **HarfBuzz in OS-lite**: current decision for TASK-0057 is build-time Inter
  rasterization for the proof overlay. Full runtime HarfBuzz in OS remains a
  follow-up.
- **JPG codec in no_std**: current decision for TASK-0057 is build-time JPEG
  decode/scale for `systemui`, embedded as BGRA for the OS scene. Runtime JPEG
  decode in OS remains follow-up.
- **SVG golden tolerance**: pixel-exact or SSIM > 0.99? SSIM allows minor
  antialiasing differences across platforms. Owner: @ui. Decide during Phase 1 test
  development.
- **Cursor animation frames**: Mocu cursors may have multi-frame (animated)
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

- [x] **Phase 0**: Resource directory + theme engine (`resources/`, `userspace/ui/theme/`).
  `.nxtheme.toml` parser with schema validation, qualifier resolver, Runtime API.
  Proof: `cargo test -p nexus-theme -- --nocapture`
- [x] **Phase 1a**: SVG rich subset (`userspace/ui/svg/`). Parser, tessellator, BGRA8888 rasterizer.
  Security reject tests. Proof: `cargo test -p nexus-svg -- --nocapture`
- [x] **Phase 1b**: PNG/JPG pipeline (`userspace/ui/image/`). Decoder, scaler, decompression bomb detection.
  Proof: `cargo test -p ui_v2b_host image:: -- --nocapture`
- [x] **Phase 1c**: Text shaping (`userspace/ui/shape/`). rustybuzz shaping, fontdue rasterization primitives.
  Proof: `cargo test -p ui_v2b_host shape:: -- --nocapture`
- [x] **Phase 2a**: Cursor pipeline (`userspace/ui/cursor/`). Mocu SVG → bitmap → windowd asset (git submodule: `sevmeyer/mocu-xcursor`, CC0).
  Proof: `cargo test -p ui_v2b_host cursor:: -- --nocapture`
- [x] **Phase 2b**: Renderer integration. `draw_image`, `draw_svg`, `draw_glyph_run` (stub) in `userspace/ui/renderer/src/draw.rs`.
  Proof: existing renderer tests pass with new deps.
- [x] **Phase 2c**: QEMU markers. `CURSOR_SVG_LOADED_MARKER`, `TEXT_TARGET_VISIBLE_MARKER`,
  `ICON_TARGET_VISIBLE_MARKER`, `WALLPAPER_VISIBLE_MARKER`, and
  `SELFTEST_UI_V2B_ASSETS_OK_MARKER` are wired through service-owned evidence.
  Markers: `cursor svg loaded`, `wallpaper visible`, `text target visible`,
  `icon target visible`, `SELFTEST: ui v2b assets ok`.
- [x] **Phase 3a**: Cursor handoff corrected into `windowd` composition.
  `fbdevd` no longer owns a second cursor truth for the live chain; it observes
  DisplayServer-composed cursor state and reports scanout evidence.
- [x] **Phase 3b**: Full-resolution scanout. `visible-bootstrap` reports
  `display: mode 1280x800 argb8888`; the wallpaper is decoded/scaled at build
  time and embedded as BGRA for deterministic OS use.
- [x] **Phase 3c**: Transient input target semantics. Hover, click, key, and
  scroll pulse state are not permanent latches. Scroll up/down are distinct.
- [x] **Phase 4**: Manifest/resource discovery for current services. `windowd`
  is a service with resources; boot order keeps `windowd` before `inputd` and
  `fbdevd`.
- [x] **Phase 5a**: `windowd` as IPC service. `os_lite::service_main_loop()`
  receives framebuffer registration and visible-input updates, then composes
  rows into the framebuffer VMO.
- [x] **Phase 5b**: `inputd -> windowd` visible-state IPC. `inputd` sends
  `OP_UPDATE_VISIBLE_STATE` with cursor and transient target state.
- [x] **Phase 5c**: `fbdevd` scanout-only. `fbdevd` registers the framebuffer
  VMO with `windowd`, waits for `STATUS_OK`, then emits flush/overlay evidence.
- [x] Task linked with stop conditions + proof commands (TASK-0057).
- [x] Service contract and reject-path tests exist for the display/input protocol
  and observer-owned evidence paths.
- [x] `docs/rfcs/README.md` updated with RFC-0056 status.
