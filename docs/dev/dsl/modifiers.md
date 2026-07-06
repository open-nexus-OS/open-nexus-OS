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
| `.overflow(o)` | `visible\|clip\|scroll` | overflow behavior |
| `.zIndex(t)` | z-index token | stacking layer |

## Color & surface (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.bg(t)` | color token | background |
| `.fg(t)` | color token | foreground/tint (text, icons) |
| `.borderColor(t)` | color token | border color |
| `.opacity(n)` | `0..100` | node opacity |
| `.material(m)` | material token | glass surface (panel/card/subtle/window/overlay) |

## Shape & elevation (class: paint)

| Modifier | Args | Meaning |
|---|---|---|
| `.rounded(t)` | radius token (`sm\|md\|lg\|xl\|full`) | corner radius |
| `.border(t)` | length token | border width |
| `.shadow(t)` | shadow token (`sm\|md\|lg`) | elevation |

## Typography (class: layout — text metrics affect measurement)

| Modifier | Args | Meaning |
|---|---|---|
| `.textSize(t)` | type-scale token (`xs\|sm\|base\|lg\|xl\|…`) | font size from the type scale |
| `.fontWeight(w)` | `regular\|medium\|semibold\|bold` | weight |
| `.textAlign(a)` | `left\|center\|right` | alignment |
| `.leading(t)` | leading token | line height |
| `.truncate(n)` | `Int` lines | line clamp with ellipsis |

## Interaction (class: paint unless noted)

| Modifier | Args | Meaning |
|---|---|---|
| `.disabled(b)` | `Bool` | disables input + applies disabled styling |
| `.focusable(b)` | `Bool` | keyboard focus participation (class: semantics) |
| `.hitSlop(n)` | spacing step | extends the touch target (class: layout) |

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

## Keys (class: layout — identity)

| Modifier | Args | Meaning |
|---|---|---|
| `.key(expr)` | scalar/id expr | stable identity for items in collections (required) |

## Changelog

- **v1 (2026-07-06)** — initial hybrid catalog (utility vocabulary + spelled-out forms),
  field classes assigned; supersedes the earlier `padding/bg/radius` sketch
  (`radius` → `rounded`).
