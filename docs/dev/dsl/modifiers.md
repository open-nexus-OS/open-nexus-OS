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
| `.aspect(w, h)` | `Int, Int` | aspect ratio |

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
| `.opacity(n)` | `0..100` | node opacity |
| `.material(m)` | material token | glass surface (panel/card/subtle/window/overlay) — tint + `--glass-shine` wash + `inset 0 1px 0` top-shine + 1px hairline, all from the theme (`Style::glass` is the single definition) |
| `.bgGradient(top, bottom)` | two exprs → `"#rrggbb[aa]"` | vertical linear background gradient (`linear-gradient(to bottom, …)`); wins over `.bg`. Args are EXPRESSIONS so both literals and props work — app-icon artwork colors ride the manifest → enumerate → props. Row-based painter: one lerped flat color per row, exact and alloc-free. |
| `.bgFade(top, bottom)` | two color tokens | the same vertical fill from TOKENS, so it re-themes (`.bgGradient` keeps taking raw hex because artwork colors are data). Use `transparent` as a stop for a legibility fade. |
| `.textShadow(t)` | `none\|soft\|strong` | legibility for text sitting on a wallpaper. An EMPHASIS step, **not** a blur radius: the row painter draws one extra glyph pass at a 1px offset in `textShadow`/`textShadowStrong` — there is no offscreen buffer to blur in (RFC-0082). |

## Shape & elevation (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.rounded(t)` | radius token (`sm\|md\|lg\|xl\|full`) | corner radius |
| `.border(t)` | length token | border width |
| `.shadow(t)` | shadow token (`sm\|md\|lg\|xl\|xxl`) | elevation — the design-handoff scale (md `0 4 12 .15` … xxl `0 25 50 .25`), painted as an analytic soft rounded-rect shadow behind the box |

## Typography (class: layout — text metrics affect measurement)

| Modifier | Args | Meaning |
|---|---|---|
| `.textSize(t)` | `xs\|sm\|base\|md\|lg\|xl\|xxl\|xxxl\|display\|hero` | font size from the type scale |
| `.fontWeight(w)` | `light\|regular\|medium\|semibold\|bold` | weight |
| `.textAlign(a)` | `left\|center\|right` | alignment |
| `.leading(t)` | `flat\|tight\|snug\|normal\|relaxed` | line height as a percentage of the font size; omit it and the baked face's own line height is used |
| `.truncate(n)` | `Int` lines | line clamp with ellipsis |

**What the type ramp can actually render** (RFC-0082). The platform bakes a
sparse ladder of `(size, weight)` faces and resolves a request to the nearest
one — **size wins over weight**:

| px | weights baked | charset |
|---|---|---|
| 13 | Regular, SemiBold | full (Latin + CJK) at Regular; Latin at SemiBold |
| 16 | Regular, SemiBold | full at Regular; Latin at SemiBold |
| 21 | Regular, SemiBold | Latin |
| 36 | SemiBold | Latin |
| 120 | Light | **digits only** (`0-9 : .`) |

So `.textSize(hero)` is for numerals — letters at that size fall back to the
16px face. `.fontWeight(light)` at a small size gives Regular, because a size
miss is far more visible than a weight miss. Adding a rung is a four-line
change in `userspace/ui/text-baked/build.rs`; adding a **full-charset** face
above 16px is forbidden (the hangul block alone would be hundreds of MB).

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
