<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Modifier Catalog

Modifiers style and lay out a view node. The naming rule is **utility vocabulary where
unambiguous, spelled out where a short form would be cryptic** — compact, familiar, and
deterministic. Arguments are **names from the one token set** (RFC-0116: colour roles, text
styles, radii, shadows, materials), **px on the 4-pt grid** for spacing, or typed scalars.

```nx
Page CtaPage {
    Stack {
        Icon { symbol: "plus" }.fg(onAccent)
        Text(@t("cta")).textStyle(body).fontWeight(medium).fg(onAccent)
    }
    .direction(row)
    .gap(gapInline)
    .paddingX(16)
    .height(36)
    .bg(accent)
    .rounded(full)
    .shadow(shadow2)
}
```

Rules:

- Duplicate modifiers on one node = error.
- Modifiers are pure (no IO, no `svc.*`).
- **Token arguments are validated at compile time** for every closed
  vocabulary (`TOKEN_VOCABULARIES` in `userspace/dsl/core/src/registry/vocab.rs`):
  `.fg(Label)` is an error with "did you mean `label`?", not a silent no-op. A name
  RFC-0116 retired is an error that names its replacement (`RETIRED_TOKENS`:
  `.fg(onSurface)` → `label`, `.material(windowPane)` → `pane`), and so is a retired
  modifier (`RETIRED_MODIFIERS`: `.textSize`, `.leading`, `.textShadow`). No alias of an
  old name is kept. Sizes (`.width(320)`) take raw px; spacing takes grid px (below).
- **A colour role may come from data.** A string literal naming a role (`.bg("accent")`)
  is checked like the bare name; an expression (`.bg($props.tint)`) is resolved when the
  node is emitted — a paint dependency — and a string that names no role paints nothing.
- **A number may come from data.** Every `Int` argument — sizes, `.grow`, `.shrink`,
  `.basis`, `.columns`, `.opacity`, `.truncate`, `.dismissAfter` — also takes an `Int`
  expression: a prop (`.columns($props.cols)`) or a store field (`.grow($state.share)`). It is
  resolved when the node is emitted and is a layout dependency: the node re-lays out when the
  value changes. A value that is not an `Int` reads as 0. (Before v8 only the size arguments
  read an expression; the others silently read 0.)
- Not every modifier reaches every widget. `Mods` is honoured in full by
  `Stack`/`List`/`Panel`/`Circle`; `Text` takes the paint set plus
  `fg`/`textStyle`/`fontWeight`/`textAlign` and `.shadow(halo)`; kit widgets
  (`Button`, `TextField`, `Icon`, `Avatar`, …) consume a documented subset and
  drop the rest.
- Every modifier has a **field class** driving invalidation: `layout` (re-layout),
  `paint` (repaint only), `semantics` (a11y tree only). The class in each heading below
  is normative and mirrors the compiler's catalog (`MODIFIERS` in
  `userspace/dsl/core/src/registry.rs`), whose row index is the `modId`: append-only, and a
  retired row keeps its id.

## Spacing (class: layout)

Spacing is px on the 4-pt grid — `0 2 4 6 8 12 16 20 24 32 40 48` (6 only between an icon
and its label) — or a semantic name: `gapInline` 8 · `gapGroup` 12 · `gapSection` 24 ·
`marginLayout` 16. An off-grid number or a fraction is a compile error. (Before RFC-0116 a
spacing was a count of 4 px steps: `.padding(4)` meant 16 px and now means 4 px.) An `Int`
expression is a layout dependency, clamped to 0..=1024 px.

| Modifier | Args | Meaning |
|---|---|---|
| `.padding(n)` | grid px \| spacing name | all edges |
| `.paddingX(n)` / `.paddingY(n)` | grid px \| spacing name | horizontal / vertical |
| `.paddingTop(n)` / `.paddingBottom(n)` / `.paddingLeading(n)` / `.paddingTrailing(n)` | grid px \| spacing name | single edge |
| `.gap(n)` | grid px \| spacing name | between children of a container (the column gap of a grid) |
| `.rowGap(n)` | grid px | the row gap of a `.columns(n)` grid (defaults to the column gap); the `Grid` widget's `rowGap:` prop is px too |
| `.margin(n)` | grid px \| spacing name | **not implemented** — accepted and ignored; use the parent's `.padding` or `.gap` |

## Sizing (class: layout)

| Modifier | Args | Meaning |
|---|---|---|
| `.width(v)` / `.height(v)` | `Int` px \| `Int` expression \| `full` | fixed size (`full` is a no-op: a cross axis stretches by default); an expression (`.width($state.selW)`, RFC-0095) is a LAYOUT dependency — the node re-lays out when it changes; a negative value clamps to 0. A stretched cross axis (the default `align`) overrides any width or height on that axis: size on the container's MAIN axis, or `.align(start)` the parent |
| `.minWidth(v)` / `.maxWidth(v)` / `.minHeight(v)` / `.maxHeight(v)` | `Int` px \| `Int` expression | constraints |
| `.grow(n)` / `.shrink(n)` | `Int` weight \| `Int` expression | flex participation (`.grow(2 * (len($state.keys) - 1))` weighs a row by its data); on a GRID cell (a `List` item of a `.columns(n)` grid), `.grow(1)` makes the cell fill its track instead of hugging its content (TASK-0067B — cards) |
| `.basis(n)` | `Int` px \| `Int` expression | flex BASE SIZE on the parent's main axis, replacing the child's measured content size in the parent's distribution |
| `.aspect(w, h)` | `Int, Int` | **not implemented** — accepted and ignored |

**Why `.basis` exists.** `.grow(n)` shares out only the space LEFT OVER after
every child has claimed its own measured size, so a row of keys labelled
`AC` / `7` / `8` / `9` never divides evenly — the two-character key stays wider
forever, at every window size. `.basis(0).grow(1)` on each child zeroes the
base and makes the split exact; `.basis(g).grow(2)` spans two tracks across a
gap `g` (a double-width key without needing grid spans). It works on BOTH axes,
so it is also how a column of rows gets equal heights.

Two properties are worth stating because they are contracts, not accidents:

- **Equal within 1px, tiles exactly.** When the space does not divide evenly the
  leftover pixels are handed out by largest remainder, so the children sum to
  the container exactly and differ by at most 1px. That is the same guarantee
  `Grid`'s `1fr` tracks give, and the two now agree.
- **A deliberate limit:** `.basis` does NOT change the node's intrinsic
  contribution when an ancestor is hugging its content. Only the parent's space
  distribution reads it.

## Layout (class: layout)

| Modifier | Args | Meaning |
|---|---|---|
| `.align(a)` | `start\|center\|end\|stretch` | cross-axis alignment |
| `.justify(j)` | `start\|center\|end\|between\|around` | main-axis distribution |
| `.direction(d)` | `row\|column` | stack direction (containers) |
| `.wrap(b)` | `Bool` | flex wrap |
| `.overflow(o)` | `visible\|hidden` | overflow behavior (`hidden` clips — wherever the container is painted, inside a scrolling list too) |
| `.scroll(a)` | `vertical\|horizontal\|paged` | makes THIS container a scroll layer (RFC-0109): its content is laid out and painted once into the layer's band, the app's physics turns a wheel notch into an offset, and the compositor moves the band — no re-layout, no re-raster per step. Several per page (four layers; more flatten into the page unscrolled, clipped to their viewport); a `.scroll` inside another's content clips like `overflow(hidden)` and does not scroll on its own. Pair with `on EndReached -> dispatch(...)` on the same container for lazy loading (fires once when its offset nears its content end; re-arms after each layout). See `docs/dev/ui/foundations/layout/scroll.md`. |
| `.overlay(kind?)` | `modal\|transient` (optional) | lifts THIS container OUT OF FLOW as a full-bleed layer over its parent's content (drop-down panels, dialogs). Anchor inside the layer with ordinary flex (rows/`Spacer`/`justify`); paint and hit-testing prefer the layer naturally (later node ids win every overlap — a handler on the layer itself is the outside-tap closer). **`modal`** (ADR-0068): the layer joins the bounded modal stack (max 4); while it is the topmost modal, hit-testing, hover and text focus are confined to its subtree, ESC and a tap on it that no handler claims fire its `on Dismiss` (a layer that must not close on a backdrop tap absorbs with `on Tap -> dispatch(Noop)`); app-host mirrors the depth to windowd, which refuses input on the app's other windows. **`transient`**: a toast-class layer that confines nothing and leaves through `on Dismiss` when `.dismissAfter` elapses. Both REQUIRE `on Dismiss` (NX0413). |
| `.dismissAfter(ms)` | `Int` (0..=60000) | on a `.overlay(transient)` layer: its declared lifetime — the HOST's one-shot timer fires the layer's `on Dismiss` after `ms` (the DSL holds no clock). Off a transient layer = NX0413. |
| `.zIndex(t)` | z-index token | **not implemented** — later siblings paint over earlier ones; `.overlay()` lifts a layer |

## Color & surface (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.bg(c)` | colour role | background |
| `.fg(c)` | colour role | text and icon colour (a text node defaults to `label`) |
| `.tint(c, s?)` | colour role, `fill`\|`border`\|`line` | the role as a wash background at 16 % (`fill`, the default), 30 % or 55 % — the only alphas a role takes (a tinted badge, an operator key, a bubble); wins over `.bg` |
| `.borderColor(c)` | colour role | border colour |
| `.borderTint(c)` | colour role | a border in the role at 30 % (the edge of a tinted bubble) |
| `.border(w)` | `hairline` (1 px) \| `ring` (2 px) | border width; without a colour modifier it draws in `separator`. Any border modifier replaces the material's edge |
| `.opacity(n)` | `0..255` | node opacity (`Fraction`, `OPAQUE = 255` — **not** a percentage; `.opacity(102)` is the design's 0.4). It multiplies down the subtree: the node and everything in it — fills, borders, shadows, text — paint at the product of their own and their ancestors' opacities |
| `.material(m)` | material (below) | the surface recipe: fill, edge, shadow and — on the wallpaper — the backdrop blur |
| `.bgGradient(top, bottom)` | two exprs → `"#rrggbb[aa]"` | vertical linear background gradient (top to bottom); wins over `.bg`. Args are EXPRESSIONS so both literals and props work — app-icon artwork colors ride the manifest → enumerate → props. Row-based painter: one lerped flat color per row, exact and alloc-free. |
| `.bgFade(top, bottom)` | two colour roles | the same vertical fill from roles, so it re-themes (`.bgGradient` keeps taking raw hex because artwork colors are data). Use `transparent` as a stop for a legibility fade. |

**Colour roles** — exactly `ColorToken::ALL` (`userspace/ui/theme-tokens/src/color_token.rs`),
one value per theme in `resources/themes/*.nxtheme.toml` `[tokens]`:

| Roles | Use |
|---|---|
| `background` | the opaque page area that is not on the wallpaper |
| `label` · `labelSecondary` · `labelTertiary` | text and icons: content · metadata and section labels · placeholders and disabled only |
| `separator` | every hairline (dividers, list separators, pane and chip borders) |
| `fill` · `fillHover` · `fillActive` | a control's resting fill · the hover and press layers, which the host shows by opacity (never a background) |
| `glassPanel` · `glassWindow` · `glassCard` · `glassEdge` | the material fills (below) · the 1 px edge of every glass layer |
| `surfacePane` · `surfaceOverlay` · `scrim` | the opaque pane in a window · every floating surface · the dimming behind a modal |
| `primary` · `onPrimary` | the brand's solid button and its label |
| `accent` · `accentGlow` · `onAccent` | the one tint (on-states, selection, links, focus) · its glow · text on it and on `danger` |
| `danger` · `success` · `warning` | status |
| `modeAirplane` · `modeDesktop` · `modeTablet` · `island` · `chart1`…`chart5` | the mode toggles' on-states · the activity runner · chart series |
| `wallpaperTint` · `labelShadow` · `transparent` | the wash over the wallpaper · the halo behind a label on it (`.shadow(halo)`) · no colour |

**Materials** — only the surface that sits on the wallpaper blurs, one per stack:

| Material | Fill | Backdrop | Edge · shadow |
|---|---|---|---|
| `panel` | `glassPanel` | regular: 40 px, saturation 180 % | `glassEdge` · `shadow4`, lit top edge |
| `window` | `glassWindow` | regular | `glassEdge` · `shadow4`, lit top edge |
| `dock` | `glassPanel` | thick: 64 px, 180 % | `glassEdge` · `shadow3`, lit top edge |
| `thin` / `ultraThin` | `glassPanel` | 20 px / 8 px | `glassEdge` |
| `card` | `glassCard` | none — a translucent layer on a blur | `glassEdge` |
| `pane` | `surfacePane` | none — opaque | — |
| `overlay` | `surfaceOverlay` | none — opaque | `glassEdge` · `shadow3`, lit top edge |

The recipe is `nexus_style::Style::material` (`userspace/ui/style/src/lib.rs`), the one
definition every kit widget calls too; an explicit `.bg`, border or `.shadow` on the node wins.
A blur material inside another one (or inside a `Panel`) is **NX0414**: a layer on a blur is
`card` or opaque. How the regions reach the compositor:
`docs/dev/ui/foundations/visual/materials.md`.

## Shape & elevation (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.rounded(r)` | `sm` 8 \| `md` 12 \| `lg` 16 \| `xl` 24 \| `full` **or** `Int` px | corner radius on all four corners; `full` is a capsule or a circle. The outermost surface is `xl`; a container inside is concentric (parent radius − padding, never below `sm`) — a computed radius has no token, which is why raw px stays expressible |
| `.corners(tl, tr, br, bl)` | four radii (tokens or `Int` px) | per-corner radius, clockwise from top-left (a bubble's tail); wins over `.rounded` |
| `.shadow(a, b?)` | `shadow1`…`shadow4` \| `shadowInset` \| `edgeHighlight` \| `halo` | up to two in one call, e.g. `.shadow(shadow3, edgeHighlight)` (a modifier appears once per node). `shadow1` knobs · `shadow2` icons, chips · `shadow3` menus, popovers, toasts, the dock · `shadow4` panels, modals, windows — painted as an analytic soft shadow behind the box, values from the theme's `[shadow.*]`. `edgeHighlight` (the lit top edge of a raised surface) and `shadowInset` (a well) draw inside the box as its one-pixel top line. `halo` puts `labelShadow` behind a text on the wallpaper: one extra glyph pass 1 px down, no blur |

## Typography (class: layout — text metrics affect measurement)

| Modifier | Args | Meaning |
|---|---|---|
| `.textStyle(s)` | a text style (below) | size, line height, weight and tracking together. A text node without one is `body`; `overline` sets its text in upper case |
| `.fontWeight(w)` | `light\|normal\|medium\|semibold\|bold` | the one override of the style's weight |
| `.textAlign(a)` | `left\|center\|right` | alignment |
| `.figures(f)` | `tabular\|proportional` | the digits' width: `tabular` gives every digit the face's widest-digit cell (a count, a clock, a value that changes in place never shifts); `proportional` undoes a style's tabular figures (`clock`) |
| `.textFit(pct, min, max)` | `Int` %, `Int` px, `Int` px | **container** modifier: derives ONE font size from this container's content-box height (`pct` %, clamped to `min`/`max`) and hands it down to its text descendants like an inherited font size; each text keeps its style's line-to-size ratio |
| `.truncate(n)` | `Int` lines | multi-line clamp — **no effect today**: it reaches the text node, but text is always one line, so every `n` ≥ 1 behaves the same. Single-line clipping needs no opt-in: the painter marks ANY run wider than its own box with `…`. |

The text styles (`TextStyleToken`; values in `resources/themes/base.nxtheme.toml`
`[textStyle.*]`, the same in every theme):

| Style | size / line (px) | weight | tracking |
|---|---|---|---|
| `clock` | 128 / 128 | 300 | −0.02 em, tabular digits |
| `display` | 64 / 64 | 300 | −0.02 em |
| `largeTitle` | 30 / 36 | 700 | −0.02 em |
| `title1` | 24 / 30 | 700 | −0.015 em |
| `title2` | 20 / 26 | 600 | −0.01 em |
| `title3` | 17 / 22 | 600 | −0.005 em |
| `headline` | 15 / 20 | 600 | 0 |
| `body` | 14 / 20 | 400 | 0 |
| `subheadline` | 13 / 18 | 400 | 0 |
| `footnote` | 12 / 16 | 400 | 0 |
| `caption` | 11 / 14 | 500 | +0.01 em |
| `caption2` | 10 / 13 | 500 | +0.02 em |
| `overline` | 11 / 14 | 600 | +0.06 em, upper case |

`clock` (the lock screen's time on the regular and wide size classes) and `display` (the same
clock on compact, the calculator's number) are digits only. The line box is the style's line,
authoritative: `body` is 20 px tall whatever the face's own ascent and descent; the painter centres the face in it (half-leading). Tracking is applied by
the same pen in measurement, ellipsis and drawing.

**Why `.textFit` is a ratio, and why it sits on the container.** A
design does not say "make the label as large as fits" — it says a 23px label
sits in a ~75px key, i.e. ~30% of the box. `.textFit(30, 11, 52)` states exactly
that, so the label tracks the key at every window size instead of at three
breakpoints. Two consequences worth knowing:

- It is a **container** modifier. On a text node the target would be measured
  against `constraints.max_height`, which is the PARENT's height while
  measuring and the text's own line box while placing — 30% of 75 → 23, then
  30% of 25 → 7. It oscillates. A container's box is settled before its
  children are placed, so the target is constant for the subtree.
- It needs a box somebody else decided (a stretched or grown child — the
  common case). A **hugging** container would size itself from the very text it
  is sizing, so there the fit is skipped and `max` applies.

After the height gives the target, each text steps DOWN one pixel at a time
until its run fits its own width, so a long number simply stops fitting at the
larger faces instead of being cut or written against a digit count.

**What the atlas can render.** Every style's size at its default weight has its
own baked face, and so has every weight override the apps use (`FontSize` in
`userspace/ui/text-baked/src/ladder.rs`):

| px | weights baked | charset |
|---|---|---|
| 10 | 500, 700 | Latin |
| 11 | 500, 600 | Latin |
| 12 | 400, 500, 600 | Latin |
| 13 | 400, 500, 600 | full (Latin + CJK) at 400; Latin otherwise |
| 14 | 400, 500, 600 | full at 400 (every face's fallback); Latin otherwise |
| 15 | 600 | Latin |
| 17 | 600 | Latin |
| 20 | 400, 500, 600 | Latin |
| 24 | 700 | Latin |
| 30 | 700 | Latin |
| 64, 128 | 300 | **digits only** (`0-9 : , . − - + × ÷ % =` and the space): `display`, `clock` |

A request between faces (a `.textFit` size, an unusual override) resolves through
`FontSize::nearest`: **size wins over weight** — the largest baked size NOT above the
request (text never draws larger than the box that asked for it), then the nearest weight
baked at that size, a tie going to the heavier face. A codepoint a face lacks comes from the
14 px full face, so CJK in a title renders small instead of blank, and letters on a
digits-only face fall back the same way. Adding a face is one line in `FACES`
(`userspace/ui/text-baked/build.rs`) plus its `FontSize` variant; a **full-charset** face
above 16 px fails the build (the hangul block alone would be hundreds of MB).

## Interaction (class: paint unless noted)

| Modifier | Args | Meaning |
|---|---|---|
| `.disabled(b)` | `Bool` | disables input + applies disabled styling |
| `.focusable(b)` | `Bool` | **not implemented** — keyboard focus participation (class: semantics) |
| `.autofocus(b)` | `Bool` | on a text field (`TextField`/`SearchBar`): while it is on screen — inside the topmost modal, if one is open — and NO field holds text focus, the host gives it focus through the same path a tap takes (the keyboard goes to it without a click). It also HOLDS the keyboard: a press on a control beside it (a category button) leaves the focus in the field — a press on another text field still moves it. Use it where typing is the surface's purpose (the shell search). (class: semantics, TASK-0067B) |
| `.hitSlop(n)` | grid px \| spacing name | grows the INPUT rect outward by n px; layout and pixels unchanged (class: layout) |

## Accessibility (class: semantics)

| Modifier | Args | Meaning |
|---|---|---|
| `.label(s)` | `Str` \| `@t(key)` | accessible name (required on unlabeled interactive nodes) |
| `.role(r)` | role enum | **not implemented** — semantic role override |
| `.hint(s)` | `Str` \| `@t(key)` | **not implemented** — accessible hint |

## Motion (class: paint)

Semantic motion tokens with explicit categories — no free-form animation language:

| Modifier | Args | Meaning |
|---|---|---|
| `.animate(t, value: expr)` | motion token + `Bool`/`Int`/`Fx` | animate state-driven property changes |
| `.transition(t)` | motion token | insert/remove/open/close lifecycle motion |
| `.effect(t, trigger: expr)` | motion token + any scalar | bounded attention effect on trigger change |

Reduced-motion behavior is part of each token's contract.

### `value:` is PRESENT/ABSENT — not "on change"

`.animate` binds a property to whether its driving value is **non-zero**, not to
the fact that the value moved. The host seeds the node at
`MotionToken::resting(prop, value != 0)`, so for every opacity-primary token
(`snappy`, `smooth`, `emphasized`, `fade`):

| `value:` | resting opacity |
|---|---|
| non-zero | `1.0` — drawn |
| `0` | `0.0` — **invisible**, and cascaded to every box inside the node |

That is the intended contract ("show this while `x`"), and it is sharp enough to
cut. `.animate(snappy, value: $state.disp)` on a display card — a `Str`, which
has no zero/non-zero reading — rested the card at opacity 0 and blanked the
number, the history line and the card's own glass together, while the store, the
scene, the layout and the glyph raster were all provably correct. The animation
transform is applied **last**, so it erases six correct stages silently.

Consequences, all enforced:

- **`value:` takes `Bool`/`Int`/`Fx` only** — the same rule `types.md` already
  states for modifier arguments. Anything else has no presence reading and
  stamps **no intent at all** (fail-open: no animation, never an invisible
  node). Never let it fold to `0`.
- **Motion on change is `.effect`**, not `.animate`. A node that is always
  present has no honest `value:`; give it `.effect(pulse|wiggle, trigger: …)`.
- **`trigger:` accepts any scalar including `Str`** — an effect only asks "did
  this change", so a string folds to a stable hash.

Regression signals: `userspace/dsl/runtime/tests/anim_intent_folding.rs` (the
folding rules) and `tests/dsl_apps_conformance/tests/calculator_pixels.rs`
(`nothing_rests_invisible_over_the_display` — the app-level invariant).

The token argument is validated against the curated motion set (`snappy, smooth, emphasized,
fade, slideUp, fadeScale, wiggle, pulse, slideDown, pushInline`). A `.transition` runs on the
compositor: app-host rasters the subtree once and windowd moves the layer (ADR-0097). An
`.animate` or `.effect` intent is interpolated by app-host's animation driver on the frame pulse
and painted per node. See `docs/dev/ui/foundations/animation.md` (token→physics mapping, scope).

## Keys (class: layout — identity)

| Modifier | Args | Meaning |
|---|---|---|
| `.key(expr)` | scalar/id expr | stable identity for items in collections (required) |

## Control props (kit widgets)

The control widgets take the design system's options as props (free `name: value` pairs; an
unknown value falls back to the widget's default, so a typo renders the default control).
Shared: `size: "xs" | "sm" | "md" | "lg" | "xl"` (the control size: one height per size) and
`tint: "<role>"` (a colour role by name). Implementation: `dsl/runtime/src/registry/controls.rs`
and the arms in `registry/widgets.rs`; proof: `dsl/runtime/tests/control_props.rs`.

| Widget | Props |
|---|---|
| `Button` | `label`, `variant` (`glass` default · `ghost` · `secondary` · `tinted` · `filled` · `prominent` — the primary action · `destructive`), `prominent: true`, `size`, `shape` (`rounded` · `circle`; default by role: capsule, a circle when icon-only), `tint`, `role` (`destructive` · `confirm` · `cancel` · `close`), `selected`, `fullWidth`. The legacy `.bg(danger)` / `.bg(fill)` still pick destructive / secondary. |
| `Toggle` | `checked`, `size`, `tint`, `appearance: "button"` (a selectable capsule instead of the switch); `label` stays the accessible name. |
| `Checkbox` | `checked`, `indeterminate`, `size`, `tint`. |
| `TextField` | as before, plus `size` `xs` / `xl`. |
| `SearchBar` · `Select` | `size`; `SearchBar` also `tint`. |
| `Badge` · `Chip` | `size`, `tint`. |
| `ProgressBar` | `tint` (or `.fg`), `size`, `label`, `showValue`. |
| `Spinner` | `tint` (or `.fg`). |

## Still declared but NOT implemented

These parse and check, and the runtime then ignores them: the emitter has no arm for their
`modId` (`userspace/dsl/runtime/src/emit/modifiers.rs`), so they never reach layout or paint.
Listed so nobody spends an afternoon on a modifier that was never wired:

| Modifier | State |
|---|---|
| `.margin` | ignored; use `.padding` on the parent or `.gap` |
| `.aspect` | ignored |
| `.zIndex` | ignored; later siblings paint over earlier ones, and `.overlay()` lifts a layer |
| `.focusable` | ignored |
| `.role`, `.hint` | ignored |
| `.label` | satisfies the accessibility lint (`NX0402`) and is stored in the IR, but nothing reads it yet |
| `.truncate` | accepted, no effect (see Typography) |
| `.justify(around)` | degrades to `start` (`between`, `center`, `end` work) |

`.key` has no paint arm either, and needs none: collection identity comes from the template's
key expression ([ir.md](ir.md)). `.textSize`, `.leading` and `.textShadow` are not
unimplemented but retired: any use is a compile error naming the replacement.

## Changelog

- **v8 (2026-11-09, TASK-0357)** — every `Int` argument takes an `Int` expression (a prop or a
  store field, a layout dependency): `.columns`, `.grow`, `.shrink`, `.basis`, `.opacity`,
  `.truncate` and `.dismissAfter` read it like the size arguments always did, instead of 0.
- **v7 (2026-11-09, TASK-0353)** — `.figures(tabular|proportional)` added (append-only id 62,
  tabular digits — RFC-0116 M-3). Control props: `Button` variant / size / shape / tint /
  role / selected / fullWidth, `Toggle` and `Checkbox` size / tint, `size` and `tint` on the other
  controls (see *Control props*).
- **v6 (2026-11-09, RFC-0116, TASK-0353 P2)** — one token set. `.textStyle` (append-only
  id 58, thirteen styles), `.tint` (59), `.borderTint` (60) and `.corners` (61) added;
  `.textSize`, `.leading` and `.textShadow` retired (their rows stay: ids are positional).
  Spacing is px on the 4-pt grid or a spacing name — no longer 4 px steps; an off-grid value
  does not compile. Colour roles, radii, shadows, borders and materials are the design
  artifacts' set, and every retired name is a compile error naming its replacement; a colour
  role may come from data; a blur material inside a blur material is NX0414. The atlas bakes
  one face per style and override, and tracking and the style's line box reach measurement
  and paint.
- **v5 (2026-07-31, TASK-0314)** — `.textFit(pct, min, max)` added
  (append-only id 55): box-relative type, inherited by the subtree. The ladder
  gained 44 Regular and 52 Light so it has somewhere to step — everything from
  30 to 72px used to resolve to the same 36px face, which made a fitted label
  stop growing the moment the window did. Both rungs were chosen because
  `nearest` moves NO existing token (a rung at 26 or 30 would silently
  re-render `xxl`/`xxxl` across every shipped app).
- **v4 (2026-07-31, TASK-0314)** — `.basis(n)` added (append-only id 54): the
  flex base size, so `.grow()` divides a row or column EXACTLY instead of only
  sharing out the leftover on top of unequal content widths. The grow path also
  stopped losing the indivisible remainder — an evenly grown row used to leave
  up to `total_grow − 1` pixels as a gutter on its trailing edge, while
  `place_grid` already redistributed its own; both now apportion by largest
  remainder and tile exactly.
- **v3 (2026-07-26, TASK-0306)** — `.hitSlop(n)` wired end to end. It grows
  only the hit rect, never the painted one, so a 28px status pill can be a
  44px target. Overlapping slop regions resolve to the NEAREST control, and an
  exact hit always beats a slop hit — slop can never steal a neighbour's tap.
- **v2 (2026-07-26, RFC-0082)** — `.textShadow` and `.bgFade` added
  (append-only ids 50/51); `.fontWeight`, `.leading`, `.textAlign`, `.border`,
  `.borderColor` and the four per-edge paddings stop being no-ops; token
  arguments of closed vocabularies are compile-checked; the type ramp table
  above documents what the baked faces can actually render.
- **v1 (2026-07-06)** — initial hybrid catalog (utility vocabulary + spelled-out forms),
  field classes assigned; supersedes the earlier `padding/bg/radius` sketch
  (`radius` → `rounded`).
