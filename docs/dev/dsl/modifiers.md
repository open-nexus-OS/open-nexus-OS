<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Modifier Catalog

Modifiers style and lay out a view node. The naming rule is **utility vocabulary where
unambiguous, spelled out where a short form would be cryptic** — compact, familiar, and
deterministic. Arguments are always **tokens or typed scalars**, never raw values.

```nx
Button { label: @t("cta") }
  .padding(4)
  .paddingX(6)
  .bg(accent)
  .textSize(sm)
  .fg(onAccent)
  .rounded(md)
  .shadow(sm)
  .gap(2)
  .width(full)
```

Rules:

- Duplicate modifiers on one node = error.
- Modifiers are pure (no IO, no `svc.*`).
- **Token arguments are validated at compile time** for every closed
  vocabulary (`registry::TOKEN_VOCABULARIES`): `.fg(oNSurface)` is an error,
  not a silent no-op. Modifiers whose argument is a number in practice
  (`.padding(4)`, `.width(320)`) are exempt.
- Not every modifier reaches every widget. `Mods` is honoured in full by
  `Stack`/`List`/`Panel`/`Circle`; `Text` takes the paint set plus
  `fg`/`textSize`/`fontWeight`/`leading`/`textAlign`/`textShadow`; kit widgets
  (`Button`, `TextField`, `Icon`, `Avatar`, …) consume a documented subset and
  drop the rest.
- Every modifier has a **field class** driving invalidation: `layout` (re-layout),
  `paint` (repaint only), `semantics` (a11y tree only). The class column below is
  normative and mirrors the compiler's SSOT (`userspace/dsl/core/modifiers.toml`);
  once the compiler lands, this table is **generated** from that SSOT.

## Spacing (class: layout)

| Modifier | Args | Meaning |
|---|---|---|
| `.padding(n)` | spacing step | all edges |
| `.paddingX(n)` / `.paddingY(n)` | spacing step | horizontal / vertical |
| `.paddingTop(n)` / `.paddingBottom(n)` / `.paddingLeading(n)` / `.paddingTrailing(n)` | spacing step | single edge |
| `.gap(n)` | spacing step | between children of a container |
| `.margin(n)` (+X/Y/edge variants) | spacing step | outside spacing |

## Sizing (class: layout)

| Modifier | Args | Meaning |
|---|---|---|
| `.width(v)` / `.height(v)` | length token \| `full` \| `Int` px | fixed or full-bleed |
| `.minWidth(v)` / `.maxWidth(v)` / `.minHeight(v)` / `.maxHeight(v)` | length token \| `Int` px | constraints |
| `.grow(n)` / `.shrink(n)` | `Int` weight | flex participation |
| `.basis(n)` | `Int` px | flex BASE SIZE on the parent's main axis, replacing the child's measured content size in the parent's distribution |
| `.aspect(w, h)` | `Int, Int` | aspect ratio |

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
- **Deliberate deviation from CSS:** `.basis` does NOT change the node's
  intrinsic contribution when an ancestor is hugging its content. Only the
  parent's space distribution reads it.

## Layout (class: layout)

| Modifier | Args | Meaning |
|---|---|---|
| `.align(a)` | `start\|center\|end\|stretch` | cross-axis alignment |
| `.justify(j)` | `start\|center\|end\|between\|around` | main-axis distribution |
| `.direction(d)` | `row\|column` | stack direction (containers) |
| `.wrap(b)` | `Bool` | flex wrap |
| `.overflow(o)` | `visible\|hidden` | overflow behavior (`hidden` clips) |
| `.scroll(a)` | `vertical\|horizontal` | marks THIS container as the page's scroll viewport: content is clipped and wheel input pans it paint-only (no re-layout). Pair with `on EndReached -> dispatch(...)` on the same container for lazy loading (fires once when the offset nears the content end; re-arms after each layout). |
| `.overlay()` | — | lifts THIS container OUT OF FLOW as a full-bleed layer over its parent's content (drop-down panels, dialogs). Anchor inside the layer with ordinary flex (rows/`Spacer`/`justify`); paint and hit-testing prefer the layer naturally (later node ids win every overlap — a handler on the layer itself is the outside-tap closer). |
| `.zIndex(t)` | z-index token | stacking layer |

## Color & surface (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.bg(t)` | color token | background |
| `.fg(t)` | color token | foreground/tint (text, icons) |
| `.borderColor(t)` | color token | border color |
| `.border(t)` | `thin\|hairline\|medium\|thick` | border width (listed again under Shape below) |
| `.opacity(n)` | `0..255` | node opacity (`Fraction`, `OPAQUE = 255` — **not** a percentage; `.opacity(102)` is the handoff's 0.4) |
| `.material(m)` | material token | glass surface (panel/card/subtle/window/overlay) — tint + `--glass-shine` wash + `inset 0 1px 0` top-shine + 1px hairline, all from the theme (`Style::glass` is the single definition) |
| `.bgGradient(top, bottom)` | two exprs → `"#rrggbb[aa]"` | vertical linear background gradient (`linear-gradient(to bottom, …)`); wins over `.bg`. Args are EXPRESSIONS so both literals and props work — app-icon artwork colors ride the manifest → enumerate → props. Row-based painter: one lerped flat color per row, exact and alloc-free. |
| `.bgFade(top, bottom)` | two color tokens | the same vertical fill from TOKENS, so it re-themes (`.bgGradient` keeps taking raw hex because artwork colors are data). Use `transparent` as a stop for a legibility fade. |
| `.textShadow(t)` | `none\|soft\|strong` | legibility for text sitting on a wallpaper. An EMPHASIS step, **not** a blur radius: the row painter draws one extra glyph pass at a 1px offset in `textShadow`/`textShadowStrong` — there is no offscreen buffer to blur in (RFC-0082). |

## Shape & elevation (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.rounded(v)` | radius token (`sm` 6 \| `md` 8 \| `lg` 10 \| `xl` 14 \| `xxl` 16 \| `full`) **or** `Int` px | corner radius, uniform on all four corners. Takes raw px for the same reason `.width` does: the design handoffs treat their literal radii (30 · 26 · 24 · 20 · 18 · 15 · 13 · 11) as geometry contract, and a five-rung scale cannot carry them. Prefer the token where one fits. |
| `.border(t)` | length token | border width |
| `.shadow(t)` | shadow token (`sm\|md\|lg\|xl\|xxl`) | elevation — the design-handoff scale (md `0 4 12 .15` … xxl `0 25 50 .25`), painted as an analytic soft rounded-rect shadow behind the box |

## Typography (class: layout — text metrics affect measurement)

| Modifier | Args | Meaning |
|---|---|---|
| `.textSize(t)` | `xs\|sm\|base\|md\|lg\|xl\|xxl\|xxxl\|display\|hero` | font size from the type scale |
| `.fontWeight(w)` | `light\|regular\|medium\|semibold\|bold` | weight |
| `.textAlign(a)` | `left\|center\|right` | alignment |
| `.leading(t)` | `flat\|tight\|snug\|normal\|relaxed` | line height as a percentage of the font size; omit it and the baked face's own line height is used |
| `.textFit(pct, min, max)` | `Int` %, `Int` px, `Int` px | **container** modifier: derives ONE font size from this container's content-box height (`pct` %, clamped to `min`/`max`) and hands it down to its text descendants like an inherited CSS `font-size` |
| `.truncate(n)` | `Int` lines | multi-line clamp — **not implemented** (nothing wraps, so there is only ever one line). Single-line clipping needs no opt-in: the painter marks ANY run wider than its own box with `…`. |

**Why `.textFit` is a ratio, and why it sits on the container.** A design
handoff does not say "make the label as large as fits" — it says a 23px label
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

After the height gives the target, each text steps DOWN through the ladder
until its run fits its own width. That is what reproduces the calculator
handoff's 52/38/30 display ramp without hard-coding digit counts: a long number
simply stops fitting at the larger rungs.

**What the type ramp can actually render** (RFC-0082). The platform bakes a
sparse ladder of `(size, weight)` faces and resolves a request to the nearest
one — **size wins over weight**:

| px | weights baked | charset |
|---|---|---|
| 11 | Regular, SemiBold | Latin (`xs` — sub-labels, group labels, captions) |
| 13 | Regular, SemiBold | full (Latin + CJK) at Regular; Latin at SemiBold |
| 16 | Regular, SemiBold | full at Regular; Latin at SemiBold |
| 21 | Regular, SemiBold | Latin |
| 36 | SemiBold | Latin |
| 44 | Regular | Latin (a `.textFit` rung) |
| 52 | Light | Latin (a `.textFit` rung — the calculator display numeral) |
| 120 | Light | **digits only** (`0-9 : .`) |

So `.textSize(hero)` is for numerals — letters at that size fall back to the
16px face. `.fontWeight(light)` at a small size gives Regular, because a size
miss is far more visible than a weight miss. Adding a rung is a four-line
change in `userspace/ui/text-baked/build.rs`; adding a **full-charset** face
above 16px is forbidden (the hangul block alone would be hundreds of MB).

A tie rounds **up**: `sm` (12) sits exactly between the 11 and 13 rungs and
resolves to 13. That is what makes the ladder extensible — baking a smaller
rung stays an addition instead of silently shrinking every label already
written against the rung above it.

Practically, the small end resolves like this: `xs` (11) → 11 · `sm` (12) and
`base` (14) → 13 · `md` (16) and `lg` (18) → 16. Two distinct sizes carry the
whole panel/chrome density, so hierarchy below 16px comes from **11 vs 13
plus weight and color**, not from the token number you wrote.

## Interaction (class: paint unless noted)

| Modifier | Args | Meaning |
|---|---|---|
| `.disabled(b)` | `Bool` | disables input + applies disabled styling |
| `.focusable(b)` | `Bool` | keyboard focus participation (class: semantics) |
| `.hitSlop(n)` | spacing step | grows the INPUT rect outward by n steps; layout and pixels unchanged (class: layout) |

## Accessibility (class: semantics)

| Modifier | Args | Meaning |
|---|---|---|
| `.label(s)` | `Str` \| `@t(key)` | accessible name (required on unlabeled interactive nodes) |
| `.role(r)` | role enum | semantic role override |
| `.hint(s)` | `Str` \| `@t(key)` | accessible hint |

## Motion (class: paint)

Semantic motion tokens with explicit categories — no free-form animation language:

| Modifier | Args | Meaning |
|---|---|---|
| `.animate(t, value: expr)` | motion token + driving value | animate state-driven property changes |
| `.transition(t)` | motion token | insert/remove/open/close lifecycle motion |
| `.effect(t, trigger: expr)` | motion token + trigger | bounded attention effect on trigger change |

Reduced-motion behavior is part of each token's contract.

**Status: implemented** (Tier 2, TASK-0062/0075). The token argument is validated
against the curated motion set (`snappy, smooth, emphasized, fade, slideUp,
fadeScale, wiggle, pulse`); an intent is bound at runtime and interpolated by the
app-host `AnimationDriver` on the compositor frame pulse, then painted per-node.
See `docs/dev/ui/foundations/animation.md` (token→physics mapping, scope, demo).
Whole-window/layer compositor transforms are the open Track C (Tier 1).

## Keys (class: layout — identity)

| Modifier | Args | Meaning |
|---|---|---|
| `.key(expr)` | scalar/id expr | stable identity for items in collections (required) |

## Still declared but NOT implemented

These parse and type-check and then do nothing at paint time. Listed so nobody
spends an afternoon on a modifier that was never wired:

`paddingTop`/`paddingBottom`/`paddingLeading`/`paddingTrailing` are wired
(RFC-0082); `.hitSlop` is wired (TASK-0306). `.margin`, `.aspect`, `.zIndex`,
`.truncate`, `.focusable`, `.role`, `.hint` are not. `.label` satisfies the a11y lint and
carries the accessible name, but nothing renders it yet. `.justify(around)`
silently degrades to `start`.

## Changelog

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
