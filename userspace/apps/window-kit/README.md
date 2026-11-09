# window-kit — DSL component library

The window UI every windowed app shares: the title chrome, the window body with its panes, the
menus, the action bar, the dialogs and the system toast — the design artifacts' `Window`,
`AppWindow`, `WindowPane`, `WindowMenu`, `Menu`, `WindowActionBar`, `Alert`, `Modal` and
`SystemToast`. One source for window UI (TASK-0337: one window path): windowd draws no title bar,
the app draws this kit inside its own surface.

A library, not an app (`bundle_type = "library"`): an app lists `dependencies = ["window-kit"]`
in its manifest, and the components it references are compiled into its program at build time
([`docs/dev/dsl/project-layout.md`](../../../docs/dev/dsl/project-layout.md), "Widget
libraries"). A library declares components only — no pages, stores, events or routes — and its
files stay flat in `ui/components/`, as the build reads one level.

## Components

| Component | What it is |
|---|---|
| `WinTopBar` | the title chrome: app chip and window menu, tools, navigation, pane toggles, the window controls |
| `WinAppChip`, `WinAppGlyph` | the top bar's app chip (open while the window menu is) and the app's glyph in it |
| `WinNavCell`, `WinToolCell`, `WinBarRule` | the toolbar's back · forward · refresh and tool cells (32 px each, hidden by `""`), and the 1×16 rule between groups |
| `WinAppWindow` | the window body: left sidebar · content · right sidebar, inline or as overlays by width tier |
| `WinAppMenuLayer` | the window menu's layer under the app chip, closed by a tap beside it |
| `WinPane` | one zone of a window (sidebar, content, properties): header, body, the opaque pane surface |
| `WinPaneTitle`, `WinPaneRule` | a pane header's title and a zone's hairline |
| `WinSideItem`, `WinSideHeader` | a navigation-sidebar row and a section header |
| `WinPropRow` | a properties-pane row: label left, value right |
| `WinAppMenu`, `WinTileGlyph` | the window menu the app chip opens (move and resize, tiling arrangements) and its tiling glyphs |
| `WinMenu`, `WinMenuItem`, `WinMenuRow` | a menu panel on the overlay surface, its dispatching row and the handler-free row |
| `WinMenuFace`, `WinMenuIcon`, `WinMenuMark` | a row's face (colours as role names), its icon slot and its trailing mark |
| `WinActionBar`, `WinActionItem`, `WinActionFace` | the floating action bar at the bottom of a content pane, its items and their look |
| `WinButtonFace` | the chrome icon button (a ghost circle) |
| `WinAlert`, `WinModal` | a compact confirmation and a centred dialog over a scrim |
| `WinToast` | the system toast: a transient confirmation at the left edge |

## The store contract

The components dispatch the kit's window events; the HOST APP declares them, in a window store
of its own (RFC-0084 — a library cannot declare stores today, TASK-0357 P4):

| Event | Dispatched by | What the host does |
|---|---|---|
| `WinMenu(Str)` | the app chip, the more button | opens (`"app"`, `"more"`, …) or closes (`""`) a chrome menu |
| `WinTool(Str)` | the top bar's tool buttons | the app's own tool action |
| `WinAct(Str)` | window controls, menu rows, action-bar items | forwards window controls to windowd (`svc.settings.set("window.control", …)`) |
| `WinNav(Str)` | back / forward / refresh | the app's navigation |
| `WinMenuPick(Str)` | sidebar rows, jump entries | a navigation pick (an app with a navigation store handles it there) |
| `WinLeft`, `WinRight`, `WinPaneClose` | pane toggles, an overlay pane's close | shows or hides the side panes |
| `WinNoop` | inert rows | nothing |

`userspace/apps/calculator/ui/composables/window.store.nx` is the reference host store.

## Tests

In `tests/dsl_apps_conformance/tests/`: `window_kit.rs` (the chrome, panes, menus, dialogs and
action bar mounted through a host app), `overlays.rs` (alert, modal and toast layering and
dismissal), `settings_tiling_menu.rs` (the window menu in a real app: every tile forwards its zone
verb through `WinAct`). Run them with
`cargo test -p dsl_apps_conformance --test window_kit --test overlays --test settings_tiling_menu`.
The pixel goldens of the overlays and the window menu live in `tests/dsl_goldens/tests/`
(`overlays.rs`, `tiling_menu.rs`).

## Known limits

`WinPane` and `WinAppWindow` are over the structure gate's component size (listed in
`tests/dsl_apps_conformance/app_structure.baseline`): a slot cannot be forwarded into another
component (`Slot x` inside a slot body is refused), so the zones that render a slot cannot move
into parts — `WinPane` repeats its zones for the bare and the panel arm (the panel's surface
sits on the root), and `WinAppWindow` renders the left sidebar inline, as a regular overlay and
as a compact sheet. Slot forwarding is the language change that would let both shrink. Every
app carries its own copy of the window store until a library may ship one (TASK-0357 P4).
