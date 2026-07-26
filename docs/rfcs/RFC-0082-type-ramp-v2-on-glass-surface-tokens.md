# RFC-0082: Type ramp v2 + on-glass surface tokens

- Status: In Progress
- Owners: @ui
- Created: 2026-07-26
- Last Updated: 2026-07-26
- Links:
  - Tasks: `tasks/TASK-0305-greeter-design-handoff-type-ramp-on-glass-tokens.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md` (token SSOT this extends)
    - `docs/rfcs/RFC-0080-shared-atlas-ro-vmo.md` (the atlas this grows)
    - `docs/rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md` (who paints what)
  - Design contract: `userspace/apps/greeter/docs/design_handoff_os_login/README.md`

## Status at a Glance

- **Phase 0 (type ramp)**: ✅ host-proven — the baked atlas carries a size AND weight ladder
- **Phase 1 (on-glass roles)**: ✅ host-proven — semantic color roles for text/icons over wallpaper
- **Phase 2 (paint primitives)**: ✅ host + visible-boot proven (dark) — text shadow, glass hairline, inset top-shine, token gradient

Definition: "Complete" means the **contract** is defined and the **proof gates** are
green (host tests + the QEMU marker ladder). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live
in `tasks/TASK-0305`.

- **This RFC owns**:
  - the **type ramp contract**: which (size, weight) pairs the platform can
    actually rasterize, and how a `TypographyToken` + `FontWeight` resolve to one
  - the **charset budget per face** (what keeps the shared atlas bounded)
  - the **on-glass color roles**: semantic names for content that sits on glass or
    directly on the wallpaper, and their light/dark values
  - the **text-shadow contract**: what `.textShadow(token)` means and, explicitly,
    what it is *not* (no Gaussian blur)
- **This RFC does NOT own**:
  - the greeter's layout or its session flow (`docs/dev/ui/shell/session.md`,
    authority stays in `sessiond`)
  - compositor blur radii or the glass wire levels (RFC-0067; unchanged here)
  - focus/blur input triggers, window transitions, or the unlock animation
  - locale data (RFC-0077 owns the locale packs)

### Relationship to tasks (single execution truth)

`tasks/TASK-0305` defines the stop conditions and proof commands for every phase
below and is the only place implementation status is tracked.

## Context

Every text run in a DSL app renders at **13 px or 16 px**, in **Regular 400**,
regardless of what the page asked for. `source/services/app-host/src/probe/paint.rs`
picks the face with `if font_size >= 15 { Body } else { Small }`, and
`userspace/ui/text-baked/build.rs` bakes exactly two faces from `InterVariable.ttf`
at the default instance. So:

- `.textSize(display)` (36 px in the type scale) paints identically to
  `.textSize(md)` (16 px) — the scale exists in the tokens and dies in the painter;
- `.fontWeight(...)` is declared in the modifier catalog (modId 33), type-checked,
  documented — and has no `apply_modifier` arm at all;
- `TextShadow` exists as a type in `nexus-layout-types` with **zero** producers and
  **zero** consumers; the glyph blitter takes no shadow parameter;
- `GlassSurface.border` is resolved from the theme and then **ignored** by
  `Mods::visual()`, so no glass surface has the 1 px hairline the design system
  specifies, and `.border()`/`.borderColor()` are silent no-ops.

The consequence is that any surface whose hierarchy comes from **type** rather
than from boxes cannot be built. The login greeter is the first such surface
(`userspace/apps/greeter/docs/design_handoff_os_login/`): a 132 px / weight-300
clock over a wallpaper, legible only because of text shadows, with 44 px glass
pills that need a hairline and an inset top-shine. It will not be the last —
lock screen, OOBE, media full-screen and notification hero states all read the
same way.

The reason this was never noticed is that the type scale is *layout-truthful*:
measurement uses the token size, so boxes are laid out at 36 px while glyphs
paint at 16 px. It looks like a spacing bug, not a missing capability.

## Goals

- A DSL page can express a **legible 120 px numeral** and a **600-weight label**
  and get exactly that on screen.
- The atlas stays **bounded and explicit**: every new face declares its charset,
  and the total is asserted in a test.
- Text over the wallpaper is legible through **semantic tokens**, not per-app
  hex — light and dark switch with the theme like everything else.
- Existing UI does not shift: the 13 px and 16 px full-charset faces stay
  **byte-identical**.

## Non-Goals

- Free-form font sizes. The ramp is a **closed set**; a page picks a token.
- Runtime font parsing or font loading. Everything stays build-time baked.
- Gaussian text shadow. See the failure model below.
- Letter-spacing / tracking and OpenType feature control (incl. tabular figures).
  Called out because the design asks for both; they are follow-ups, not this RFC.
- A new glass wire level. The existing `GLASS_*` levels are re-tuned in the theme
  SSOT, not extended.

## Constraints / invariants (hard requirements)

- **Determinism**: glyph coverage is baked at build time from a pinned face; the
  same source tree produces byte-identical atlases. No runtime rasterization.
- **Bounded resources**: each baked face declares one of three charsets —
  `Full` (Latin + EXTRAS + the CJK wide tail), `Latin` (Latin + EXTRAS), or
  `Digits` (a listed handful). A `Full` face above 16 px is **forbidden**: the
  hangul syllable block alone would make it hundreds of megabytes. The total
  atlas size is asserted by a host test, because it is mapped into every
  app-host as one shared RO VMO (RFC-0080) and the arena has already been
  blown once by atlas duplication.
- **No fake success**: a missing glyph renders blank (fail-visible), never a
  substituted box or a silently different size.
- **No silent fallback in the ramp**: resolving (size, weight) to a face is a
  total, documented function. Where it degrades — a CJK codepoint in a `Latin`
  face — it degrades to a **named** fallback face, and that is part of the
  contract, not an accident.
- **Security floor**: unchanged. The atlas is read-only (RFC-0080); nothing here
  parses untrusted input. Secure text fields keep substituting bullets *before*
  the value enters the layout tree.
- **Stubs policy**: no stubs. A face that is not baked is not selectable.

## Proposed design

### Contract / interface (normative)

#### 1. The type ramp

`nexus-text-baked` exposes faces as `(px, weight, charset)`. The resolver is:

```
face(px, weight) = the baked face with the same weight whose px is nearest to
                   the requested px; ties round down. If no face exists for the
                   requested weight, fall back to Regular at the same rule.
```

The baked set (Phase 0):

| px  | weight | charset | rationale |
|-----|--------|---------|-----------|
| 13  | 400    | Full    | existing — chrome labels, list rows |
| 16  | 400    | Full    | existing — body text |
| 13  | 600    | Latin   | emphasized captions |
| 16  | 600    | Latin   | emphasized body / section labels |
| 21  | 400    | Latin   | `xl` step |
| 21  | 600    | Latin   | display names, titles |
| 36  | 600    | Latin   | `display` step, avatar initials |
| 120 | 300    | Digits  | hero numerals (clock) |

`Digits` = `0-9`, `:`, `.`, `-`, space. Nothing else. (`-` is there because
`"--:--"` is the clock's literal state until walltime anchors; without it the
placeholder silently dropped to the 16px face.) A hero face is for numerals;
prose at that size is not a supported case.

**CJK fallback (normative):** a codepoint outside a `Latin`/`Digits` face's
charset resolves against the nearest **`Full`** face (16 px). Mixed-script runs
therefore render CJK smaller than the surrounding Latin. This is a deliberate,
bounded degradation — the alternative is a blank glyph or a multi-hundred-MB
atlas. It is visible, documented, and testable.

**Adding a face** is a four-line change in `text-baked/build.rs` plus a size
assert. Adding a *`Full`* face above 16 px requires amending this RFC.

#### 2. Weight instancing

`InterVariable.ttf` is a variable font; `fontdue` 0.9 has no axis API
(`FontSettings` exposes only `collection_index` and `scale`). Weighted faces are
instanced with `ttf-parser` (`set_variation(b"wght", …)`, already in the
lockfile) and rasterized through the in-tree `nexus-svg` rasterizer.

The two existing `Full` faces keep using `fontdue` so their coverage bytes and
every golden that depends on them stay unchanged. Two rasterizers in one build
script is the deliberate price of a zero-regression migration; the seam is one
function and is documented at the call site.

#### 3. On-glass color roles

New `ColorToken` roles, authored in `resources/themes/*.nxtheme.toml` and
reachable from the DSL by name:

| role | meaning |
|---|---|
| `onGlass` / `onGlassMuted` / `onGlassStrong` | text on glass or on the wallpaper (primary / secondary / high-emphasis) |
| `glassIcon` | icon stroke on glass |
| `glassPlaceholder` | placeholder text in a glass field |
| `glassFocus` | the border color a focused glass control takes |
| `glassFill` | the flat translucent fill of a control *inside* glass (never blurred — nested blur is not a thing) |
| `wallpaperTint` | full-bleed wash that keeps content legible without killing the image |
| `wallpaperVignette` | the bottom stop of the legibility fade |
| `textShadow` / `textShadowStrong` | the two shadow tints |
| `transparent` | `#00000000`; the top stop of a fade |

`glassTextPrimary`/`glassTextSecondary`/`glassTextStrong` are already authored in
the theme TOML with **no `ColorToken` role** — they are promoted, not duplicated.

Scale additions: `[typography] hero` and `[leading] flat`.

#### 4. Text shadow (normative — and what it is not)

`.textShadow(none | soft | strong)` sets `VisualStyle.text_shadow`. The painter
renders it as **one additional `draw_text_row` pass** at a 1 px offset in the
shadow tint, before the main pass.

It is **not** a Gaussian blur. The CSS contract the design describes
(`0 2px 24px`) needs an offscreen pass, which the row-based painter has no place
for. The token names are therefore about *emphasis*, not about a blur radius, and
the docs must say so. If a real blurred shadow is ever needed it is a new token
and a new pass, not a redefinition of these.

#### 5. Glass hairline + inset top-shine

`Mods::visual()` starts honoring `GlassSurface.border` (a 1 px rounded-rect
edge) and paints the material's `edgeHighlight` as a **1 px inset line on the top
edge**. The existing soft top-to-bottom shine gradient is capped at
`min(edge.a, 0.15)`: once `edgeHighlightAlpha` carries the real inset value
(0.60 in light), reusing it for a full-height gradient would wash the whole
surface white.

This splits one authored value into its two real jobs. It changes the look of
every glass surface, which is intended — the shine was previously either absent
or accidental.

### Phases / milestones (contract-level)

- **Phase 0 — type ramp**: the face table above is baked; `face(px, weight)` is
  total; the `>= 15` threshold is gone from every call site. Proof: host tests
  per face + the atlas size assert.
- **Phase 1 — on-glass roles**: the roles above exist in all four theme layers,
  in `ColorToken`, and in the DSL name table; `.fontWeight`/`.leading`/
  `.textAlign` and the per-edge padding modifiers stop being no-ops. Proof:
  theme golden tests + DSL conformance.
- **Phase 2 — paint primitives**: `.textShadow`, `.bgFade`, the glass hairline
  and the inset shine render. Proof: host paint tests + a visible QEMU boot.

## Security considerations

- **Threat model**: unchanged. No new input is parsed at runtime; the atlas is
  build-time data mapped read-only.
- **Mitigations**: the atlas VMO stays `VmoRo` (RFC-0080); the added faces do not
  change its permissions, only its length.
- **Open risks**: atlas growth is an availability concern, not a confidentiality
  one — it is bounded by the charset rule and asserted by a test.

## Failure model (normative)

- A codepoint missing from the selected face's charset falls back to the nearest
  `Full` face. If it is missing there too, it renders **blank** — never a
  substituted glyph and never a crash.
- A `.textSize()`/`.fontWeight()` token that does not resolve is a **compile
  error**, not a silent no-op. (Today unknown token arguments are silently
  dropped at runtime; this RFC closes that hole for the vocabularies it touches.)
- `.textShadow` on a node with no text is a no-op, not an error.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-text-baked
cd /home/jenning/open-nexus-OS && cargo test -p nexus-theme-tokens
cd /home/jenning/open-nexus-OS && cargo test -p nexus-dsl-runtime
cd /home/jenning/open-nexus-OS && just test-host
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Plus one visible boot (`just start`) checking both themes: the greeter's hero
clock, the glass pill's hairline, and — because the glass re-tune is OS-wide —
the shell top bar, control center and launcher.

### Deterministic markers (if applicable)

No new markers. The existing session ladder must stay green:
`sessiond: greeter (n=…)` → `windowd: session shell visible (product=…)`.

## Alternatives considered

- **MSDF text** (`userspace/ui/msdf` exists, 22 tests, wired to nothing):
  resolution-independent, would make any size free. Rejected *for now* because it
  replaces the row-based A8 blitter that windowd, app-host and every shell
  surface share — a much larger change than the greeter needs, and one that
  should be its own RFC with its own perf proof. The ramp does not foreclose it.
- **Integer upscaling of the 16 px atlas** for large type: free, and looks it —
  an 8× scaled 16 px glyph is unusable at 120 px. Rejected.
- **Vendoring static `Inter-Light.ttf` / `Inter-SemiBold.ttf`**: simpler than
  axis instancing, but the repo vendors the Inter *source* repo, which ships only
  `.woff2` statics plus the variable TTF. Adding binary artifacts that are not in
  the vendored upstream is worse provenance than instancing the one TTF that is.
  Kept as the fallback if instancing produces bad outlines.
- **A new `lock` glass level** for the login surface: cleanest separation, but it
  costs an append-only wire level plus two compositor match arms, and the
  handoff's login values are the ones we want for dock/control-center/launcher
  anyway. Re-tuning the existing levels was chosen deliberately, accepting the
  OS-wide visual change.

## Open questions

- ~~Does axis instancing produce usable coverage?~~ **Resolved**: `nexus-svg`
  was the wrong tool (no nonzero winding — every counter would fill), so the
  build script grew its own scanline rasterizer: 16 vertical subsamples with
  exact horizontal span coverage and the nonzero rule. Proven by
  `counters_are_not_filled_nonzero_winding` and by eye at 13–120px.
- ~~Tabular figures~~ **Resolved**: the `Digits` face is baked unkerned with
  every numeral widened to the widest numeral's advance and centred in that
  cell. `hero_digits_are_tabular` pins it; `13:16` and `08:59` measure equal.
- ~~Does `.material(panel)` glass composite on a DESKTOP-band surface?~~
  **Resolved**: yes. `GLASS_PANEL` is skipped in the desktop pass and drawn in
  pass 2b, which is unconditional; the greeter's pill and session buttons
  render with their hairline and top-shine on the visible boot.
- The light-theme values are covered by the theme golden tests but not by a
  screenshot — the Control Center would not open on the proof boot (an
  unrelated long-idle `windowd: STALL present stuck`). (@ui — carry into the
  next visible run.)

---

## Implementation Checklist

- [ ] **Phase 0**: face table baked, `face(px, weight)` total, `>= 15` threshold
      removed — proof: `cargo test -p nexus-text-baked`
- [ ] **Phase 1**: on-glass roles + scale steps in all theme layers and the DSL
      name table; text/padding modifiers no longer no-ops — proof:
      `cargo test -p nexus-theme-tokens && cargo test -p nexus-dsl-runtime`
- [ ] **Phase 2**: text shadow, token fade, glass hairline, inset shine render —
      proof: `just test-host` + visible boot
- [ ] Task linked with stop conditions + proof commands (`tasks/TASK-0305`)
- [ ] Atlas size assert exists and is under the documented budget
- [ ] Unknown `.textSize`/`.fontWeight` tokens are compile errors
