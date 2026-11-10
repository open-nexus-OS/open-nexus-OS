# window-kit — DSL component library

The window UI every windowed app shares: the title chrome, the window body with its panes, the
menus, the action bar, the dialogs and the system toast — the design artifacts' `Window`,
`AppWindow`, `WindowPane`, `WindowMenu`, `Menu`, `WindowActionBar`, `Alert`, `Modal` and
`SystemToast`. One source for window UI (TASK-0337: one window path): windowd draws no title bar,
the app draws this kit inside its own surface.

A library, not an app (`bundle_type = "library"`): an app lists `dependencies = ["window-kit"]`
in its manifest, and the components it references are compiled into its program at build time
([`docs/dev/dsl/project-layout.md`](../../../docs/dev/dsl/project-layout.md), "Widget
libraries"). A library declares components (flat in `ui/components/`, as the build reads one
level) and the state they show (`ui/composables/`) — no pages or routes.

## Components

| Component | What it is |
|---|---|
| `WinTopBar` | the title chrome: app chip and window menu, up to four tools (`tool1`…`tool4`, each `…Active` and `…Disabled`), navigation, pane toggles, the window controls — pass every prop: an omitted string is not `""` |
| `WinAppChip`, `WinAppGlyph` | the top bar's app chip (open while the window menu is) and the app's glyph in it |
| `WinNavCell`, `WinToolCell`, `WinBarRule` | the toolbar's back · forward · refresh and tool cells (32 px each, hidden by `""`, dimmed without a tap by `disabled`), and the 1×16 rule between groups |
| `WinAppWindow` | the window body: left sidebar · content · right sidebar, inline or as overlays by width tier |
| `WinBodyRegions`, `WinSidebarOverlay`, `WinPropsOverlay`, `WinAppMenuOverlay` | its parts: the regions row with the floating action bar, the sidebar and the properties pane as overlays, the ⋮ app menu's place |
| `WinAppMenuLayer` | the window menu's layer under the app chip, closed by a tap beside it |
| `WinPane` | one zone of a window (sidebar, content, properties): header, body, the opaque pane surface |
| `WinPaneHead`, `WinPaneBody`, `WinPaneFoot` | its zones: the header strip with title, custom header and actions; the clipped or scrolling body; the footer strip |
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
| `WinActionSheet`, `WinSheetRow` | the action sheet — an actions card and a cancel card rising from the bottom edge over a scrim — and the handler-free choice row the app wraps in its tap (`SheetCancel(id)` and `WinNoop` are the app's events, like the alert's) |
| `WinFabFace` | the floating action button's face: a 56 px accent circle with the glyph; the app owns the tap and the place |

## The store contract

The kit ships its chrome state: `ui/composables/window.store.nx` — `WindowStore` (the open
chrome menu, the panes' visibility inline and as overlays, the last refused window control) and
`WindowEvent`. An app that uses the chrome gets the store with it: the compiler adds a library's
composable when the app or a component it pulled in reads a field of its store, so an app
declares none of it (TASK-0357 P4, RFC-0084 amendment).

| Event | Dispatched by | What the kit's store does |
|---|---|---|
| `WinMenu(Str)` | the app chip, the more button | opens (`"app"`, `"more"`, …) or closes (`""`) a chrome menu |
| `WinTool(Str)` | the top bar's tool buttons | closes an open menu |
| `WinAct(Str)` | window controls, menu rows, action-bar items | closes an open menu and forwards the control to windowd (`svc.settings.set("window.control", …)`) |
| `WinNav(Str)` | back / forward / refresh | closes an open menu |
| `WinMenuPick(Str)` | sidebar rows, jump entries | closes an open menu |
| `WinLeft`, `WinRight`, `WinPaneClose` | pane toggles, an overlay pane's close | shows or hides the side panes |
| `WinNoop` | inert rows | nothing (a consume-tap inside an overlay) |

What a tool, a navigation step or a pick MEANS is the app's: it reacts with an effect that
dispatches its own event — `@effect on WinMenuPick(id) { dispatch(NavPick(id)); }` (Settings,
Stash), `@effect on WinTool(t) { dispatch(ToolPick(t)); }` (Stash).

## Tests

In `tests/dsl_apps_conformance/tests/`: `window_kit.rs` (the chrome, panes, menus, dialogs and
action bar mounted through a host app), `overlays.rs` (alert, modal and toast layering and
dismissal), `settings_tiling_menu.rs` (the window menu in a real app: every tile forwards its zone
verb through `WinAct`). Run them with
`cargo test -p dsl_apps_conformance --test window_kit --test overlays --test settings_tiling_menu`.
The pixel goldens live in `tests/dsl_goldens/tests/`: the overlays (`overlays.rs`), the window
menu (`tiling_menu.rs`) and every other primitive — top bar resting and with its states on, button
face, app chip closed and open, tool cells, pane panel and bare, sidebar rows, property rows, menu,
action bar, action sheet, FAB — light and dark (`kit_primitives.rs`).

## Known limits

`WinPane` and `WinAppWindow` compose their parts by forwarding their slots into them (RFC-0084
amendment, 2026-11-10).

Not yet expressible, and so approximated (each is a platform item of TASK-0353 P7, not a kit
gap): the action bar's indicator moves with the `active` prop instead of sliding; menus and the
window menu enter with `slideDown`, not scaled from the anchor corner; an item's hover layer
covers the whole item, not only its content; an overlay's panel cannot be capped at the
container's height with a scrolling body (no relative length); the accent glow under an active
sidebar row has no shadow token in the DSL.
