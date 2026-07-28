<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Icons

This document defines the **default icon contract** for Open Nexus OS UI,
including the **symbol registry** — the list of names developers can use
today (see [Symbol names](#symbol-names)).

See also: `docs/dev/ui/foundations/visual/icon-guidelines.md`.

## Default icon set

- **Lucide** (ISC License)
  - Style: outline/line icons
  - Constraint: keep the set visually consistent by using a **small number of canonical sizes and stroke widths**.

## Canonical sizes & stroke widths

Treat these as **tokens** (do not pick random per-icon values):

- **Icon size**: 16 / 20 / 24 (default) / 32
- **Stroke width**:
  - 16 → 1.5
  - 20 → 1.75
  - 24 → 2.0 (default)
  - 32 → 2.0

Rationale: this keeps icons “line-sharp” but **not too thin**, and avoids visual drift across the UI.

## Rendering rules (determinism + crispness)

- Use the canonical **24×24 viewBox** for source assets where possible.
- Prefer rendering at **integer pixel sizes** and snapping translation/placement to whole pixels.
- Do not mix outline and filled variants in the same surface unless explicitly designed (filled icons should be a deliberate, separate style choice).
- If an icon needs optical adjustment, fix it at the asset/token level (not as one-off per usage).

## How icons are wired (theme-linked import)

The maintained vendor SVGs live in **`resources/icons/lucide/icons/`** (our
own curated repo — licence-clean, updated deliberately). They are linked into
the build through the **theme**, not hard-coded:

- `resources/themes/base.nxtheme.toml` `[icons] path = "resources/icons/lucide/icons"`
  points at the SVG directory.
- `[icons.symbols]` maps **our SwiftUI-style symbol vocabulary** to file
  stems, e.g. `"magnifyingglass" = "search"`.
- `userspace/ui/widgets/icon/build.rs` reads the theme, imports **exactly the
  mapped symbols** (path flattening → vector contours), and generates the
  `LucideSymbol` enum, `LUCIDE_*` constants and `lucide_symbol_named(name)`.

Only names in `[icons.symbols]` exist at runtime — the other ~3400 Lucide
files are not baked (binary size stays flat).

## Using icons

In the `.nx` DSL, pass the symbol name to `Icon`:

```nx
Icon { symbol: "magnifyingglass" }
Icon { symbol: "chevron.left" }
```

In Rust widget code, use the generated constants / lookup:

```rust
use nexus_widget_icon::{LUCIDE_HOUSE, lucide_symbol_named};

let home = LUCIDE_HOUSE;                       // compile-time
let dyn_ = lucide_symbol_named("gearshape");   // runtime by name (Option)
```

## Symbol names

The registry as authored in `[icons.symbols]` (SSOT:
`resources/themes/base.nxtheme.toml` — regenerate this table when adding):

| Symbol name | Lucide file | Typical use |
|---|---|---|
| `airplane` | `plane` | flight mode toggle |
| `arrow.clockwise` | `refresh-cw` | refresh / reload |
| `arrow.counterclockwise` | `rotate-ccw` | undo / revert |
| `arrow.down` | `arrow-down` | sort / move down |
| `arrow.down.circle` | `download` | downloads |
| `arrow.left` | `arrow-left` | back navigation |
| `arrow.right` | `arrow-right` | forward navigation |
| `arrow.up.arrow.down` | `arrow-up-down` | sort direction |
| `arrow.up` | `arrow-up` | sort / move up |
| `arrow.uturn.backward` | `undo-2` | restore from trash |
| `battery.75` | `battery-full` | status battery |
| `bell` | `bell` | notifications |
| `bluetooth` | `bluetooth` | BT toggle |
| `calculator` | `calculator` | calculator app |
| `calendar` | `calendar` | dates (created / modified) |
| `checkmark` | `check` | confirmation, checkbox |
| `chevron.down` | `chevron-down` | expanders, dropdowns |
| `chevron.left` | `chevron-left` | back / previous |
| `chevron.right` | `chevron-right` | disclosure, next |
| `chevron.up` | `chevron-up` | collapse |
| `columns.2` | `columns-2` | split window mode |
| `desktopcomputer` | `monitor` | desktop shell mode |
| `doc` | `file` | generic file |
| `doc.on.doc` | `copy` | copy |
| `doc.text` | `file-text` | documents folder |
| `ellipsis` | `ellipsis` | overflow menu (action bar) |
| `ellipsis.vertical` | `ellipsis-vertical` | overflow menu (window toolbar) |
| `eye` | `eye` | view / visibility menu |
| `film` | `video` | videos folder |
| `folder` | `folder` | folders, file type |
| `gearshape` | `settings` | settings |
| `house` | `house` | home (nav pill) |
| `info.circle` | `info` | help / about |
| `internaldrive` | `hard-drive` | storage device |
| `ipad` | `tablet` | tablet shell mode |
| `list.bullet` | `list` | list view mode |
| `lock` | `lock` | lock / privacy / id |
| `magnifyingglass` | `search` | search |
| `maximize` | `maximize` | fullscreen window mode |
| `menu` | `menu` | overflow / hamburger |
| `message` | `message-circle` | chat |
| `minimize` | `minimize` | minimise window |
| `minus` | `minus` | decrement / minimise |
| `moon` | `moon` | dark mode |
| `move` | `move` | move file |
| `music.note` | `music` | audio folder |
| `paintpalette` | `palette` | theme / accent |
| `paperplane` | `send` | send message |
| `pencil` | `pencil` | edit |
| `person` | `user` | account / avatar fallback |
| `photo` | `image` | pictures folder |
| `play` | `play` | play media |
| `plus` | `plus` | increment / add / new |
| `power` | `power` | power / shutdown |
| `shippingbox` | `box` | size (bytes) |
| `sidebar.left` | `panel-left` | toggle left sidebar |
| `sidebar.right` | `panel-right` | toggle right sidebar |
| `speaker.wave` | `volume-2` | sound |
| `square.and.arrow.up` | `share-2` | share |
| `square.grid.2x2` | `grid-2x2` | tile view mode |
| `square.grid.3x3` | `grid-3x3` | grid view mode |
| `square.grid` | `layout-grid` | launcher / app grid |
| `square.pencil` | `square-pen` | modified / rename |
| `square` | `square` | freeform window mode / zoom |
| `star` | `star` | favourite / recent |
| `sun.max` | `sun` | light mode / brightness |
| `trash` | `trash` | delete (destructive) |
| `wifi` | `wifi` | Wi-Fi toggle |
| `xmark` | `x` | close / clear |


## Adding a new symbol

1. Check the SVG exists in `resources/icons/lucide/icons/<stem>.svg`.
2. Add one line to `[icons.symbols]` in `resources/themes/base.nxtheme.toml`:
   `"<our.name>" = "<lucide-stem>"` — pick the SwiftUI-style name if one
   exists (developer familiarity), otherwise a short semantic name.
3. Rebuild — the icon build regenerates the enum/constants; the name is
   immediately usable from `Icon { symbol: … }` and `lucide_symbol_named`.
4. Add the row to the table above.

**Why a missing entry is not obvious.** A symbol name travels as a `Str` prop,
so nothing in the compiler ever sees it: an unmapped name type-checks, lowers,
mounts, lays out, and then paints the honest grey placeholder box
(`dsl/runtime/src/registry/widgets.rs`). Four symbols shipped that way before
`tests/dsl_apps_conformance/tests/icon_symbols.rs` closed the hole — that test
scans every shipped `.nx` file and names the file, line and symbol. If you add a
component prop that forwards a symbol, add it to that test's `SYMBOL_PROPS`.
