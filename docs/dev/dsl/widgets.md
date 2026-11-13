<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Widget Reference

Every widget a `.nx` view can name, one entry each: what it is, how you declare it, every prop
with its type, default and allowed values, what it binds or dispatches, which modifiers it
honours, what accessibility requires, and an example that compiles. The catalog is the
compiler's `WIDGETS` table (`userspace/dsl/core/src/registry/widgets.rs`); each widget is built
by an arm in `userspace/dsl/runtime/src/registry/widgets.rs` (the controls in
`registry/controls.rs`), which calls the design system's kit widget in `userspace/ui/widgets/`.

`tests/dsl_conformance/tests/docs_examples.rs` keeps this page honest: every widget of the
catalog must have its `###` entry here, and every example must compile.

## Reading an entry

- **Declaration** — the minimal form. A widget with a *positional prop* takes it in parentheses:
  `Text("Hello")` is `Text { value: "Hello" }`.
- **Props** — free `name: value` pairs. A prop the arm does not read is ignored; a value outside
  the listed set falls back to the default (a typo renders the default widget, never nothing).
  `Str`, `Int`, `Bool`, `Fx` are the [types](types.md); a *role* is a colour role name
  (`accent`, `danger`, …, the token set of RFC-0116).
- **Interaction** — a widget whose primary prop is a value the user edits **binds**: write
  `checked: $state.on` and the tap writes the new value back (the two-way bind rule, `BindRule`
  in the catalog). Any widget can carry handlers: `on Tap -> dispatch(Event)` directly after the
  widget belongs to it.
- **Modifiers** — every widget takes flex participation (`.grow`, `.shrink`, `.basis`). A
  container (`Stack`, `List`, `Panel`, `Circle`, `Grid`) honours every [modifier](modifiers.md);
  a kit widget honours only the ones its entry lists and draws the rest of its look itself.
- **Accessibility** — an interactive widget needs an accessible name: its label prop, or
  `.label("…")`. Without one the compiler reports NX0402. A container that takes a tap (`Stack`,
  `Circle`, … with `on Tap`) needs one too: `.label(@t(…))`, or something a reader can speak
  inside it — a `Text`, a widget's label prop, a component — else NX0415 (an icon-only close
  circle is `.label(@t("win.close"))`; a backdrop that closes a sheet is named for what it does).
- **Disabled** — `.disabled(true)` dims a widget by its family (the design system's
  `nexus_style::Dim`): actions and text entry — `Button`, `TextField`, `SearchBar`,
  `ProgressBar` — to 50 % opacity; value and selection controls — `Toggle`, `Checkbox`,
  `Slider`, `Chip`, `Select`, `ListItem` — to 45 %; a container to 50 %.

## Catalog

| Group | Widgets |
|---|---|
| Layout | [`Stack`](#stack), [`Spacer`](#spacer), [`Grid`](#grid), [`List`](#list), [`Panel`](#panel), [`Card`](#card), [`Circle`](#circle) |
| Content | [`Text`](#text), [`Icon`](#icon), [`Image`](#image), [`Avatar`](#avatar), [`Badge`](#badge), [`Skeleton`](#skeleton) |
| Controls | [`Button`](#button), [`Toggle`](#toggle), [`Checkbox`](#checkbox), [`Slider`](#slider), [`Chip`](#chip), [`Select`](#select) |
| Inputs | [`TextField`](#textfield), [`SearchBar`](#searchbar) |
| Feedback | [`Spinner`](#spinner), [`ProgressBar`](#progressbar), [`Toast`](#toast), [`Banner`](#banner) |
| Navigation and rows | [`ListItem`](#listitem), [`Toolbar`](#toolbar), [`Breadcrumbs`](#breadcrumbs) |
| Platform | [`NativeWidget`](#nativewidget) |

## Layout

### `Stack`

A container that lays its children out in a column or a row — the building block of every
layout.

```nx-view
Stack {
    Text("Wi-Fi").textStyle(headline)
    Text("Connected").textStyle(footnote).fg(labelSecondary)
}
.gap(4)
.padding(16)
```

| | |
|---|---|
| Children | any number |
| Positional prop | — |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — all of them. Defaults: `.direction(column)`, `.align(stretch)`,
`.justify(start)`, no gap, no padding. `.columns(n)` turns it into an `n`-column grid,
`.scroll(vertical)` into a scroll layer the compositor moves, `.overlay(modal | transient)`
into an out-of-flow layer that fills its parent.

**Discussion** — to share a row evenly, give each child `.basis(0).grow(1)`: the basis makes
the division exact instead of sharing out only the space left over by unequal contents. A grown
child is allocated its share, but its own content keeps its natural width: content that must
fill the share sits in a `.direction(row)` stack whose content grows (the key wrappers in
`userspace/apps/calculator/ui/components/keypad/CalcKeypad.nx`).

**See also** — [`Grid`](#grid), [`List`](#list), [`Spacer`](#spacer), [`Panel`](#panel).

### `Spacer`

Flexible empty space: it takes whatever room is left on its parent's main axis. Use it to push
siblings apart, such as a title to the leading edge and a button to the trailing one.

```nx-view
Stack {
    Text("Title").textStyle(headline)
    Spacer
    Button { label: "Done" }
}
.direction(row)
.align(center)
```

| | |
|---|---|
| Children | none |
| Positional prop | — |
| Interactive | no |

**Modifiers** — flex only. A spacer has a grow factor of 1 by default; `.grow(n)` weights it
against other grown siblings. Paint modifiers have nothing to act on.

**See also** — [`Stack`](#stack) (`.justify(between)` spreads children without a spacer).

### `Grid`

A container that places its children in a fixed number of equal-width columns, filling row by
row. Use it for tiles, launchers and galleries; use [`Stack`](#stack) for a single row or
column.

```nx-view
Grid(3) {
    Circle { size: 40 }
    Circle { size: 40 }
    Circle { size: 40 }
    Circle { size: 40 }
    Circle { size: 40 }
}
.gap(12)
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `columns` | `Int` | `1` | the column count, clamped to 1–12; each column is one equal (`1fr`) track |
| `rowGap` | `Int` | the column gap | px on the 4-pt grid; the gap between rows |

| | |
|---|---|
| Children | any number |
| Positional prop | `columns` |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — all of them that apply to a box: `.gap(n)` is the column gap, `.padding`, the
size pins, `.scroll`, `.overlay`, materials and paint. `.direction`, `.align`, `.justify` and
`.wrap` have no meaning on a grid (it has no main axis) and are ignored.

**Discussion** — `.columns(n)` on a [`Stack`](#stack) or a [`List`](#list) produces the same
grid; on a `List` the items become the cells, which is how data-driven tile grids are written.
`rowGap` and `.rowGap(n)` are equivalent.

**See also** — [`Stack`](#stack), [`List`](#list).

### `List`

A container for a data-driven collection: the children of a `for` loop over a collection become
its items. Structurally it is a [`Stack`](#stack) with the same defaults; the name states the
intent.

```nx-view
List {
    ListItem { title: "Wi-Fi", showChevron: true }
    ListItem { title: "Bluetooth", showChevron: true }
}
```

| | |
|---|---|
| Children | any number |
| Positional prop | — |
| Interactive | no |

**Modifiers** — all of them, exactly as [`Stack`](#stack). `.columns(n)` turns the list into an
`n`-column grid whose cells are the items; `.scroll(vertical)` makes it a scroll layer.

**Discussion** — a collection item needs a stable `.key(expr)` on its template root
(see [state.md](state.md)). `List` draws no separators or surface of its own: rows bring their
own look ([`ListItem`](#listitem)), and a surface comes from `.material` or a
[`Panel`](#panel) around the list.

**See also** — [`Stack`](#stack), [`Grid`](#grid), [`ListItem`](#listitem).

### `Panel`

A container with the panel surface already applied: a blurred glass material, the outermost
radius (`xl`) and the layout inset (`marginLayout`) as padding. Use it for window content
panes, Control Center tiles and sidebars.

```nx-view
Panel {
    Text("Display").textStyle(headline)
    Text("Brightness and appearance").textStyle(footnote).fg(labelSecondary)
}
.gap(4)
```

| | |
|---|---|
| Children | any number |
| Positional prop | — |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — all of them. Without them, `Panel` is a column stack with `.material(panel)`,
`.rounded(xl)` and `.padding(marginLayout)`; an explicit `.material`, `.rounded` (or
`.corners`) and `.padding` replace those defaults.

**Discussion** — a `panel` material is a backdrop blur at the compositor. A blur material
inside another blur material is NX0414: use [`Card`](#card) or `.material(card)` for a layer on
top of a panel.

**See also** — [`Card`](#card), [`Stack`](#stack).

### `Card`

A glass container for grouped content: the design system's card surface, a column of children
(or a row) with inner padding. Use it for a self-contained block on a panel or a window.

```nx-view
Card {
    Text("Storage").textStyle(headline)
    Text("48 GB of 128 GB used").textStyle(footnote).fg(labelSecondary)
}
.padding(16)
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `variant` | `Str` | `panel` | `panel` (regular blur, `xl` radius) · `dock` (thick blur) · `card` (a painted layer, `shadow2`, `lg` radius) · `inner` (no shadow) · `subtle` (no edge, no shadow) · `overlay` (the opaque floating surface) |
| `material` | `Bool` | `true` | `false` drops the blur of a `panel` or `dock` that sits on another blur surface (never blur over blur) |

| | |
|---|---|
| Children | any number |
| Positional prop | — |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — the arm reads two besides flex: `.padding(n)` (one inset on every side, taken
from the top edge; the default is `gapGroup`, 12 px, and a padding of zero also gives that
default) and `.direction(row)` (children side by side). The surface — material, edge, shadow,
radius — is the card's own and cannot be restyled; the card clips its content.

**Discussion** — without a `variant` the kit's `GlassCard` builds at the `panel` level: a blurred
surface with the `xl` radius. A card on a blur surface says `variant: "card"` (or `inner` /
`subtle`): those are painted layers and never blur. For a surface the variants do not cover, use
a [`Stack`](#stack) with `.material`, `.rounded` and `.padding`.

**See also** — [`Panel`](#panel), [`Stack`](#stack).

### `Circle`

A perfectly round container: a square box of a fixed size with a full corner radius, content
centred on both axes. Use it for round icon backgrounds, dots and avatar-like elements.

```nx-view
Circle {
    size: 44
    Icon { symbol: "wifi", size: 20 }
}
.bg(accent)
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `size` | `Int` | `40` | the side of the square in px (`.width(n)` is the fallback when `size` is absent) |

| | |
|---|---|
| Children | any number |
| Positional prop | `size` |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — all of them. The box size is pinned to `size`, the radius to `full`, and
`.align` and `.justify` default to `center`; give them explicitly to change that. Colour with
`.bg(role)`, `.tint(role, level)` or a `.material`.

**See also** — [`Avatar`](#avatar), [`Icon`](#icon), [`Stack`](#stack).

## Content

### `Text`

A run of text. It is set in one of the thirteen text styles (`body` unless the modifiers say
otherwise), wraps freely, and takes its colour from the `label` role.

```nx-view
Text("Battery").textStyle(headline)
Text("82 %").textStyle(title2).figures(tabular)
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `value` | `Str` | `""` | the text; an `Int` or `Bool` is shown as its decimal or `true`/`false` form |

| | |
|---|---|
| Children | none |
| Positional prop | `value` |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — the type modifiers: `.textStyle(style)` (size, line height, weight and tracking
together; `overline` also sets the text in upper case), `.fontWeight(w)`, `.figures(tabular |
proportional)`, `.textAlign(left | center | right)`, `.truncate(n)` (at most `n` lines, the
last cut with an ellipsis), `.fg(role)`; plus every box modifier (`.bg`, `.padding`,
`.rounded`, size pins) — a text node paints its own box.

**Accessibility** — the text is its own accessible name; no further label is needed.

**See also** — [modifiers.md](modifiers.md) (text styles, colour roles), [`Icon`](#icon),
[`Badge`](#badge).

### `Icon`

A scalable vector symbol, sized in px and coloured by a role.

```nx-view
Icon { symbol: "wifi", size: 24 }.fg(accent)
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `symbol` | `Str` | `""` | a symbol name of the theme's icon set (see [icons](../ui/foundations/visual/icons.md)); the built-in `plus`, `minus`, `close`, `star`, `chevronRight`, `chevronLeft`, `chevronDown`, `chevronUp` also resolve |
| `size` | `Int` | `16` | the glyph's side in px, clamped to 8–96 |
| `stroke` | `Fx` or `Int` | the set's own (`2`) | the line weight of a line symbol in the units of its 24-unit grid — `2.6` is a menu's check; clamped to 1–4. A built-in filled shape ignores it |

| | |
|---|---|
| Children | none |
| Positional prop | `symbol` |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — `.fg(role)` is the glyph colour (default `labelSecondary`). Nothing else is
read: size comes from `size`, not from `.width`.

**Discussion** — a symbol name that neither the theme nor the built-in set knows renders as a
tinted placeholder box, so a missing glyph is visible rather than blank. Names with dots
(`chevron.right`) come from the theme's `[icons.symbols]` table. Every line symbol of the set
shares one weight; `stroke` changes it for one icon — where a design gives a mark more weight
than its neighbours:

```nx-view
Icon { symbol: "checkmark", size: 14, stroke: 2.6 }.fg(accent)
```

**Accessibility** — an icon is decorative. An icon that is tappable sits inside a
[`Button`](#button), which takes `.label("…")`.

**See also** — [`Button`](#button), [`Circle`](#circle), [`Image`](#image).

### `Image`

A baked raster picture: an application's icon, or the icon of a file type. It is not a general
image loader.

```nx-view
Image { source: "mime:inode-directory", size: 32 }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `source` | `Str` | `""` | `"mime:<token>"` — a file-type icon (a mime type, an extension or an already-resolved name); anything else — an application id whose bundle ships its icon |
| `size` | `Int` | `64` | the baked size and the side of the square box in px, clamped to 8–256 |

| | |
|---|---|
| Children | none |
| Positional prop | `source` |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — flex only; the box is a fixed `size` square. A radius, a shadow or an opacity
on the picture goes on a wrapping `Stack` (`Stack { Image { … } }.rounded(16).shadow(shadow2)`, as
the shell's app icons do): `Image` itself draws the raster and nothing else.

**Discussion** — an unknown source renders a transparent box of the same size, so layout stays
stable while the owning view shows its own fallback.

**See also** — [`Icon`](#icon), [`Avatar`](#avatar).

### `Avatar`

A person's tile: initials on a glass backing, as a circle or a rounded square, with an optional
presence dot.

```nx-view
Avatar { name: "Ada Lovelace", size: 48, status: "online" }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `name` | `Str` | — | a display name; the initials are the first letters of up to two words, upper-cased |
| `initials` | `Str` | — | explicit initials; used when `name` is absent or empty |
| `size` | `Int` | `40` | the tile's side in px; the initials' text style follows the size |
| `square` | `Bool` | `false` | a rounded square instead of a circle (radius 8 up to 32 px, 12 up to 56, 16 above) |
| `status` | `Str` | — | `online` · `busy` · `away` · `offline`: a presence dot in the bottom-right corner (`success` · `danger` · `warning` · `labelSecondary`) inside a 2 px window-glass ring |

| | |
|---|---|
| Children | none |
| Positional prop | `initials` |
| Interactive | no; an `on Tap` handler makes it a hit target |

**Modifiers** — `.material(m)` (replaces the glass backing), `.fg(role)` (the initials' colour)
and `.textStyle(style)` (overrides the size-picked style), plus flex.

**Discussion** — bind `name` to the display name the page already has: the DSL has no string
operations, so deriving initials is the widget's job.

**See also** — [`Circle`](#circle), [`Image`](#image), [`Badge`](#badge).

### `Badge`

A small status chip or count, set in a capsule.

```nx-view
Badge { label: "3", tint: "danger" }
Badge { label: "Beta", variant: "outline" }
Badge { label: "120", max: 99, tint: "danger" }
Badge { dot: true, variant: "success" }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `label` | `Str` | `""` | the text, in the size's text style (`caption2` at `xs`/`sm`, `caption` at `md`, `footnote` at `lg`, `subheadline` at `xl`); a label made of digits is a count: semibold, tabular figures, tighter padding |
| `variant` | `Str` | `default` | `default` (the primary fill) · `secondary` · `glass` · `destructive` · `success` · `warning` · `outline` · `active` (the accent fill) · `filled` (filled in the `tint`) · `tinted` (a wash of the `tint`, the tint as text) |
| `tint` | role | — | the fill colour when no `variant` is given, so `tint: "danger"` is a red count |
| `size` | `Str` | `md` | `xs` 16 px · `sm` 18 · `md` 22 · `lg` 26 · `xl` 30 (the badge's height) |
| `dot` | `Bool` | `false` | a bare status dot (6 / 8 / 10 / 12 / 14 px by size) in the look's colour instead of a chip |
| `max` | `Int` | — | caps a count: above `max` it reads `max+` (`max: 99` turns 120 into `99+`) |

| | |
|---|---|
| Children | none |
| Positional prop | `label` |
| Interactive | no |

**Modifiers** — flex only; the look is set by `variant`, `tint`, `size` and `dot`.

**Discussion** — `success` and `warning` are washes of the status colour with the status as
text, not solid fills. `tint` without a `variant` paints a solid fill in that role.

**See also** — [`Chip`](#chip), [`Avatar`](#avatar), [`Toast`](#toast).

### `Skeleton`

A loading placeholder: a block in the control fill with a shimmer band the host sweeps across
it (one crossing in 1.4 s, ease-in-out). Use it in place of content that has not arrived.

```nx-view
Stack {
    Skeleton { width: 160, height: 16 }
    Skeleton { height: 12 }
    Skeleton { circle: true, width: 40, height: 40 }
}
.gap(8)
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `width` | `Int` | the container's width | px, at least 4 |
| `height` | `Int` | `14` | px, at least 4 (the diameter of a circle) |
| `radius` | `Int` | `12` | the corner radius in px of the block (the `md` radius by default) |
| `circle` | `Bool` | `false` | a disc instead of the rounded block |

| | |
|---|---|
| Children | none |
| Positional prop | — |
| Interactive | no |

**Modifiers** — flex only.

**See also** — [`Spinner`](#spinner), [`ProgressBar`](#progressbar).

## Controls

### `Button`

A control that runs an action when tapped: the design system's button, with five looks, five
control sizes and the shape its role gives it — a capsule with a label, a circle when it is
icon-only.

```nx
Store Counter { count: Int = 0, }
Event CounterEvent { Add, }
reduce CounterEvent { Add => state.count = state.count + 1, }

Page CounterPage {
    Stack {
        Text("Count")
        Button { label: "Add one", variant: "prominent", size: "lg" }
        on Tap -> dispatch(Add)
    }
    .gap(12)
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `label` | `Str` | `""` | the visible label, set in the size's control label style; also the accessible name |
| `variant` | `Str` | `glass` | `glass` · `ghost` (alias `plain`) · `secondary` · `tinted` · `filled` · `prominent` (the primary action: the filled look) · `destructive` |
| `prominent` | `Bool` | `false` | the filled look, whatever the variant |
| `size` | `Str` | `md` | `xs` 24 px · `sm` 30 · `md` 36 · `lg` 44 · `xl` 52 (one height per size) |
| `shape` | `Str` | by role | `rounded` (the size's rounded rectangle, for tiles in a rectangular bar) · `circle` |
| `tint` | role | `accent` | the fill of the filled look, the wash of the tinted one, the label of the quiet ones |
| `role` | `Str` | — | `destructive` (tints `danger`) · `confirm` (the filled look; a check circle when empty) · `cancel` · `close` (a cross circle when empty) |
| `selected` | `Bool` | `false` | the selected state of a toggle button: a persistent fill layer, a tinted label |
| `fullWidth` | `Bool` | `false` | stretches across its container at the size's height |

| | |
|---|---|
| Children | yes: declared children follow the label inside the button's content row |
| Positional prop | `label` |
| Interactive | yes; no two-way bind (the label is not a value) |

**Interaction** — a tap runs the button's own `on Tap` handler. Hover and press are drawn by
the compositor as a layer over the button; the button itself never repaints for them.

**Modifiers** — `.disabled(true)` (the disabled look, 50 % opacity); `.bg(danger)` and
`.bg(fill)` still select the destructive and secondary looks. Every other look comes from the
props.

**Accessibility** — `label` is the accessible name; an icon-only button needs `.label("…")`
(NX0402).

**See also** — [`Toggle`](#toggle) (`appearance: "button"` for an on/off button),
[`Chip`](#chip), [`ListItem`](#listitem).

### `Toggle`

An on/off control. By default it is the switch: a capsule track with a sliding thumb; with
`appearance: "button"` it is a selectable capsule.

```nx
Store Prefs { wifi: Bool = true, }

Page PrefsPage {
    Stack {
        Text("Wi-Fi")
        Toggle { checked: $state.wifi, label: "Wi-Fi" }
    }
    .gap(8)
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `checked` | `Bool` | `false` | the on state |
| `label` | `Str` | — | the accessible name only: the switch draws no text, a row beside it draws the title |
| `size` | `Str` | `md` | `xs` · `sm` · `md` · `lg` · `xl` (the switch from 32 × 19 to 58 × 35 px; the button at the control height) |
| `tint` | role | `accent` | the track colour when on |
| `appearance` | `Str` | `switch` | `switch` · `button` |

| | |
|---|---|
| Children | none |
| Positional prop | `checked` |
| Interactive | yes |
| Two-way bind | `checked`, written on `Tap` as the negated value (a `Bool` store field) |

**Interaction** — write `checked: $state.wifi` and a tap flips the field; no handler is needed.
Press stretches the thumb and hover is a compositor layer; the toggle itself never repaints
for them.

**Modifiers** — `.disabled(true)` (the disabled look, 45 % opacity).

**Accessibility** — `label` or `.label("…")` is required (NX0402).

**See also** — [`Checkbox`](#checkbox), [`Button`](#button) (`selected` for a one-off toggle
button), [`Slider`](#slider).

### `Checkbox`

A rounded square that is checked, unchecked or indeterminate.

```nx
Store Form { agree: Bool = false, }

Page FormPage {
    Stack {
        Checkbox { checked: $state.agree, label: "I agree" }
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `checked` | `Bool` | `false` | the checked state |
| `indeterminate` | `Bool` | `false` | a dash instead of the check mark, whatever `checked` says |
| `label` | `Str` | — | the accessible name only; the arm draws the box without text |
| `size` | `Str` | `md` | `xs` · `sm` · `md` · `lg` · `xl` (box 14 / 18 / 22 / 25 / 28 px) |
| `tint` | role | `accent` | the checked fill |

| | |
|---|---|
| Children | none |
| Positional prop | `checked` |
| Interactive | yes |
| Two-way bind | `checked`, written on `Tap` as the negated value (a `Bool` store field) |

**Interaction** — a tap writes the negated `checked`. A tap on an indeterminate box therefore
reports the negation of its `checked` value; the page clears `indeterminate` itself.

**Modifiers** — `.disabled(true)`.

**Accessibility** — `label` or `.label("…")` is required (NX0402).

**See also** — [`Toggle`](#toggle), [`Chip`](#chip).

### `Slider`

A range control with a value from 0 to 100.

```nx
Store Display { brightness: Int = 60, }

Page DisplayPage {
    Stack {
        Slider { value: $state.brightness, icon: "sun.max" }.label("Brightness")
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `value` | `Int` | `0` | the position in percent, clamped to 0–100 |
| `variant` | `Str` | `control` | `control` (the Control Center bar) · `thumb` (the thin slider: a 6 px track, a tint fill with its glow, a 20 px white thumb) |
| `icon` | `Str` | — | `control` only: a symbol name of the theme's icon set, drawn inside the fill; an unknown name draws nothing |
| `tint` | role | white fill (`control`), `accent` (`thumb`) | the fill colour; on the control bar an explicit tint replaces the white fill |
| `ticks` | `Int` | `0` | `thumb` only: that many tick marks below the track, ends included |

| | |
|---|---|
| Children | none |
| Positional prop | `value` |
| Interactive | yes |
| Two-way bind | `value`, written on `Tap` as the fraction of the track the tap landed on (an `Int` store field) |

**Interaction** — a tap at a point across the slider writes that point's fraction of the
track, 0–100, to the bound field. The slider has no drag yet: it moves on taps.

**Modifiers** — `.disabled(true)`, and flex. The slider takes the row's free width; it has no
fixed width of its own.

**Discussion** — the default `control` look is a 28 px capsule with a white fill that grows from
the leading edge, never narrower than its 28 px cap; `thumb` is the design's default slider. Both
split the track into flex weights of `value` and `100 - value`, so the layout needs no pixel
width. Both take the value from taps only: there is no drag, no `min`/`max`/`step`, and no
labels beside the track yet.

**Accessibility** — the slider has no label prop: `.label("…")` is required (NX0402).

**See also** — [`ProgressBar`](#progressbar), [`Toggle`](#toggle).

### `Chip`

A capsule token for filters and tags that can be selected.

```nx
Store Filters { unread: Bool = false, }
Event FilterEvent { Flip, }
reduce FilterEvent { Flip => state.unread = !state.unread, }

Page FilterPage {
    Stack {
        Chip { label: "Unread", selected: $state.unread }
        on Tap -> dispatch(Flip)
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `label` | `Str` | `""` | the text, in medium weight; also the accessible name |
| `selected` | `Bool` | `false` | the solid tint fill with its glow |
| `size` | `Str` | `md` | `xs` 24 px · `sm` 24 · `md` 30 · `lg` 36 · `xl` 44 (the chip's height) |
| `tint` | role | — | the selected fill and the label colour of the resting chip |
| `appearance` | `Str` | `neutral` | `neutral` (the `fill` capsule) · `tinted` (the tint at 16 %) · `filled` (the solid tint) · `outline` (a hairline: `separator`, or the tint at 55 % with a `tint`) |

| | |
|---|---|
| Children | none |
| Positional prop | `label` |
| Interactive | yes; no two-way bind (the label is not a value) |

**Interaction** — a tap runs the chip's own `on Tap` handler; the page owns `selected` and
flips it there. The kit widget's leading icon or avatar and its built-in remove button are not
props: the remove button's tap target is a static id, which a view cannot name.

**Modifiers** — `.disabled(true)`.

**Accessibility** — `label` is the accessible name.

**See also** — [`Button`](#button), [`Badge`](#badge), [`Toggle`](#toggle).

### `Select`

The closed trigger of a drop-down: a pill showing the current value and a chevron.

```nx
Store Lang { current: Str = "English", }
Event LangEvent { Open, }
reduce LangEvent { Open => state.current = state.current, }

Page LangPage {
    Stack {
        Select { value: $state.current, placeholder: "Language" }
        on Tap -> dispatch(Open)
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `value` | `Str` | — | the current value, in the control label style |
| `placeholder` | `Str` | — | shown in `labelTertiary` while there is no value; also the accessible name |
| `size` | `Str` | `md` | `xs` · `sm` · `md` · `lg` · `xl` (the control height; the pill is at least 3.3 times its height wide) |
| `open` | `Bool` | `false` | the page's option panel is open: the trigger carries the persistent fill layer and the chevron turns up |

| | |
|---|---|
| Children | none |
| Positional prop | `value` |
| Interactive | yes; no two-way bind |

**Interaction** — a tap runs the select's own `on Tap` handler, which opens the page's option
panel; the select never writes `value` itself.

**Modifiers** — `.disabled(true)`.

**Discussion** — `Select` is the trigger only. The option panel is the page's own
`.overlay(...)` list, which sets the value in its own handler.

**Accessibility** — `placeholder` is the accessible name (or `.label("…")`), even when a
value is shown (NX0402).

**See also** — [`Button`](#button), [`TextField`](#textfield), [`ListItem`](#listitem).

## Inputs

### `TextField`

A single-line text field with an optional label, helper line and error line.

```nx
Store Account { name: Str = "", }

Page AccountPage {
    Stack {
        TextField { label: "Name", value: $state.name, placeholder: "Your name" }
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `value` | `Str` | `""` | the text |
| `label` | `Str` | — | the caption above the field; also the accessible name |
| `placeholder` | `Str` | — | shown while the field is empty, in `labelTertiary` (`labelSecondary` with `shape: "capsule"`, the lock screen's field on the wallpaper) |
| `helper` | `Str` | — | a line below the field |
| `error` | `Str` | — | a line below the field in `danger`, with a `danger` border; replaces `helper` when non-empty |
| `validation` | `Str` | — | `error` (the `danger` border alone) · `success` (the green border, a check and a green helper); an `error` message wins |
| `variant` | `Str` | `boxed` | `boxed` · `glass` (alias `pill`) · `bare` |
| `appearance` | `Str` | `filled` | `filled` · `glass` (the design's name for the field on a wallpaper; `variant` wins when both are given) |
| `shape` | `Str` | by size | `capsule` (the lock-screen password exception); otherwise the size's radius (8 / 8 / 12 / 12 / 16) |
| `size` | `Str` | `md` | `xs` · `sm` · `md` · `lg` · `xl` (the control height) |
| `tint` | role | `accent` | the focus outline and the caret |
| `prefix` | `Str` | — | text before the input, in `labelSecondary` |
| `suffix` | `Str` | — | text after the input (a unit, a currency sign) |
| `icon` | `Str` | — | a symbol name of the theme's icon set, drawn before the input in `labelSecondary` |
| `readOnly` | `Bool` | `false` | no fill, a `separator` hairline |
| `secure` | `Bool` | `false` | masks the characters |

| | |
|---|---|
| Children | none |
| Positional prop | `value` |
| Interactive | yes |
| Two-way bind | `value`, written on `Change` as the new text (a `Str` store field) |

**Interaction** — write `value: $state.name` and every edit writes the new text back. A
handler on `Change` runs beside that write.

**Modifiers** — `.disabled(true)`, `.autofocus(true)` (the host focuses the field when none
holds focus), and flex: a `.grow(1)` field fills the rest of its row.

**Discussion** — the clear control, the reveal eye of a password field and number formats are
not props yet: each needs its own tap target or an input filter the host does not have.

**Accessibility** — `label` or `.label("…")` is required (NX0402).

**See also** — [`SearchBar`](#searchbar), [`Select`](#select).

### `SearchBar`

A search field: a capsule with a magnifier, the input and a clear control.

```nx
Store Find { query: Str = "", }

Page FindPage {
    Stack {
        SearchBar { value: $state.query, placeholder: "Search apps" }
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `value` | `Str` | — | the query text |
| `placeholder` | `Str` | — | shown while the query is empty; also the accessible name |
| `size` | `Str` | `md` | `xs` · `sm` · `md` · `lg` · `xl` (the control height) |
| `tint` | role | `accent` | the focus outline |
| `appearance` | `Str` | `filled` | `filled` · `glass` (the field on a wallpaper: the glass card layer, never a blur) |
| `loading` | `Bool` | `false` | the field spinner replaces the magnifier |

| | |
|---|---|
| Children | none |
| Positional prop | `value` |
| Interactive | yes |
| Two-way bind | `value`, written on `Change` as the new text (a `Str` store field) |

**Interaction** — as [`TextField`](#textfield): `value: $state.query` writes every edit back.

**Modifiers** — `.disabled(true)`, and flex.

**Accessibility** — `placeholder` or `.label("…")` is required (NX0402).

**See also** — [`TextField`](#textfield).

## Feedback

### `Spinner`

An indeterminate activity indicator: twelve spokes the host rotates.

```nx-view
Spinner { size: 28 }
Spinner { size: 20, tint: "accent" }
Spinner { size: "lg" }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `size` | `Int` or `Str` | `28` | the diameter in px (at least 8), or a control size: `xs` 14 · `sm` 20 · `md` 28 · `lg` 36 · `xl` 44 |
| `tint` | role | `labelSecondary` | the spoke colour |

| | |
|---|---|
| Children | none |
| Positional prop | — |
| Interactive | no |

**Modifiers** — `.fg(role)` is the spoke colour when `tint` is absent; flex.

**Discussion** — the rotation is the host's per-spoke opacity wash: the spinner node itself
is static.

**See also** — [`ProgressBar`](#progressbar), [`Skeleton`](#skeleton).

### `ProgressBar`

A linear progress indicator: a track with a fill at a percentage, or an indeterminate pip (the
host moves it across the track, one crossing in 1.1 s on the smooth curve).

```nx-view
ProgressBar { value: 64, label: "Download", showValue: true }
ProgressBar { }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `value` | `Int` | indeterminate | the percentage, clamped to 0–100; absent (or not an `Int`) renders the indeterminate pip |
| `height` | `Int` | by size | the bar's height in px (the size's 3 / 4 / 6 / 8 / 10) |
| `size` | `Str` | `md` | `xs` · `sm` · `md` · `lg` · `xl` (the bar height) |
| `tint` | role | `accent` | the fill colour |
| `label` | `Str` | — | a caption in a header above the bar |
| `showValue` | `Bool` | `false` | the percentage beside the label |
| `appearance` | `Str` | `linear` | `circular` draws a ring (give it a `value`; the indeterminate ring is not animated yet) |

| | |
|---|---|
| Children | none |
| Positional prop | `value` |
| Interactive | no |

**Modifiers** — `.fg(role)` is the fill colour when `tint` is absent; `.disabled(true)` (50 %
opacity); flex. The bar is as wide as its container.

**See also** — [`Slider`](#slider), [`Spinner`](#spinner).

### `Toast`

A transient notification capsule: a status dot, a message and an optional action. It is the
view only; showing, placement and dismissal belong to the overlay around it.

```nx
Store Notes { shown: Bool = true, }
Event NotesEvent { Hide, }
reduce NotesEvent { Hide => state.shown = false, }

Page NotesPage {
    Stack {
        if $state.shown {
            Stack {
                Toast { message: "Saved", variant: "success" }
            }
            .overlay(transient)
            .dismissAfter(3000)
            on Dismiss -> dispatch(Hide)
        }
    }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `message` | `Str` | `""` | the text; also the accessible name |
| `variant` | `Str` | `default` | `default` · `success` · `warning` · `destructive` |
| `action` | `Str` | — | the label of the action at the trailing edge |
| `icon` | `Str` | — | a symbol name of the theme's icon set, before the message in the accent (the status colour for a status variant); an unknown name draws nothing |

| | |
|---|---|
| Children | none |
| Positional prop | `message` |
| Interactive | yes; no two-way bind |

**Interaction** — an `on Tap` handler runs when the toast is tapped.

**Modifiers** — flex only.

**Discussion** — wrap the toast in a `.overlay(transient)` container with `.dismissAfter(ms)`
and an `on Dismiss` handler (NX0413 requires it). The toast is opaque, at most 440 px wide.

**Accessibility** — `message` is the accessible name.

**See also** — [`Banner`](#banner), [`Badge`](#badge).

### `Banner`

An inline status strip: a status icon, a title, a message and an optional action. It takes
layout space; its floating sibling is [`Toast`](#toast).

```nx-view
Banner { title: "Update available", message: "Restart to install.", action: "Restart", variant: "warning" }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `title` | `Str` | — | the first line, also the accessible name |
| `message` | `Str` | — | the second line |
| `variant` | `Str` | `info` | `info` · `success` · `warning` · `destructive` |
| `action` | `Str` | — | the label of the action under the message |
| `icon` | `Str` | — | a symbol name of the theme's icon set in place of the status icon, 18 px in the status colour; an unknown name keeps the status icon |

| | |
|---|---|
| Children | none |
| Positional prop | `message` |
| Interactive | yes; no two-way bind |

**Interaction** — an `on Tap` handler runs when the banner is tapped.

**Modifiers** — flex only.

**Accessibility** — `title` is the accessible name. A banner with only a `message` needs
`.label("…")` (NX0402), because the positional `message` is not the label prop.

**See also** — [`Toast`](#toast), [`Badge`](#badge).

## Navigation and rows

### `ListItem`

One row of a list: a title, an optional subtitle, a leading glyph, a trailing value or badge and
an optional chevron.

```nx-view
List {
    ListItem { title: "Wi-Fi", subtitle: "Connected", icon: "wifi", trailing: "Home", showChevron: true }
    ListItem { title: "Mail", icon: "envelope", badge: "3" }
    ListItem { title: "Reset", destructive: true }
}
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `title` | `Str` | `""` | the row's text, in medium weight; also the accessible name |
| `subtitle` | `Str` | — | a second line in `labelSecondary` |
| `icon` | `Str` | — | a leading symbol (18 px, `labelSecondary`; `danger` on a destructive row) |
| `trailing` | `Str` | — | a value after the text, body `labelSecondary`, truncating before the title does |
| `badge` | `Str` | — | a count in a 20 px capsule at the end |
| `badgeTint` | role | `danger` | the badge's colour role |
| `showChevron` | `Bool` | `false` | a trailing chevron |
| `destructive` | `Bool` | `false` | the title and the leading symbol in `danger` |
| `selected` | `Bool` | `false` | the persistent `fill` layer (the sidebar look) — or, on a `selectable` row, the checked state |
| `selectable` | `Bool` | `false` | a selection mark: a 22 px circle (an accent disc with a check when selected) |
| `selectionStyle` | `Str` | `circle` | `circle` · `checkmark` (a trailing accent check on the selected row) |
| `reorderable` | `Bool` | `false` | draws the reorder handle (the drag itself is a runtime gesture) |
| `separator` | `Str` | none | `inset` (the hairline from the text column to the trailing edge) · `full` |

| | |
|---|---|
| Children | none |
| Positional prop | `title` |
| Interactive | yes; no two-way bind |

**Interaction** — a tap runs the row's own `on Tap` handler.

**Modifiers** — `.disabled(true)` (the row dims to 45 %) and flex. The row is at least 48 px
high, 60 with a subtitle. Content below the line, swipe actions and the grouped appearances of
the kit's `List` builder are not reachable from a view.

**Accessibility** — `title` is the accessible name.

**See also** — [`List`](#list), [`Button`](#button), [`Toggle`](#toggle).

### `Toolbar`

The bar of a panel or sheet: a title with an optional subtitle in a 52 px row.

```nx-view
Toolbar { title: "Settings", subtitle: "Display", centerTitle: true }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `title` | `Str` | — | the headline |
| `subtitle` | `Str` | — | a caption under the title |
| `centerTitle` | `Bool` | `false` | centres the title instead of leading it |
| `variant` | `Str` | `panel` | `panel` (a glass card with a hairline underneath) · `transparent` (over the content) |

| | |
|---|---|
| Children | none |
| Positional prop | `title` |
| Interactive | no |

**Modifiers** — flex only. The default bar is the kit's `panel` variant: a glass card surface
with a hairline underneath.

**Discussion** — the arm builds a title-only bar: leading and trailing clusters of the kit
widget are not reachable from a view. Put actions beside the bar in a row.

**See also** — [`Breadcrumbs`](#breadcrumbs), [`Stack`](#stack).

### `Breadcrumbs`

A path trail: crumbs separated by chevrons, the last one the current page.

```nx-view
Breadcrumbs { items: ["Home", "Documents", "Reports"] }
```

| Prop | Type | Default | Values |
|---|---|---|---|
| `items` | `List<Str>` | empty | the crumbs in order; a single non-list value becomes one crumb |

| | |
|---|---|
| Children | none |
| Positional prop | `items` |
| Interactive | no |

**Modifiers** — flex only.

**Discussion** — the whole trail is one node, so individual crumbs are not tap targets. To
navigate, wrap the trail and handle the tap on the wrapper.

**See also** — [`Toolbar`](#toolbar), [`ListItem`](#listitem).

## Platform

### `NativeWidget`

A reserved escape hatch for a surface the catalog cannot express, named by a `handle`.

```nx-view
NativeWidget { handle: "org.example.widgets.ChartV1" }
```

| | |
|---|---|
| Children | none |
| Positional prop | — |
| Interactive | no |

**Discussion** — the compiler accepts the node and its `handle`, but no host renderer binds a
handle yet: the runtime builds it as an empty [`Stack`](#stack) (so it takes the modifiers
of one, and draws nothing of its own). Treat it as a reserved extension point; see
[syntax.md](syntax.md) for the contract it will have to meet.

**See also** — [`Stack`](#stack).
