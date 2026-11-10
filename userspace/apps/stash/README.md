# stash — DSL app

The platform file manager, built on the window kit and the design artifacts' Stash window: a places
sidebar, a breadcrumb over the real folder listing (list or tiles, sort, live search), a properties
pane, a floating action bar whose items follow the selection, and a settings mode that replaces the
content. Every read and write is a real `svc.files.*` call against the nxfs home; a listing that
cannot load says so instead of showing fake rows.

## Routes

One page, one route: `/` → `StashPage` (`ui/pages/StashPage.nx`, `ui/pages/Routes.nx`).

## Stores and events

One `Event` binds to one `Store`, so the state is split by domain; a change that crosses domains is an
`@effect` that dispatches the other domain's event. The kit's events belong to the kit's store, so a
kit event this app must branch on (`WinMenuPick`, `WinNav`, `WinTool`) is handed to the store that
owns every field the branches write as its own event (`NavPick`, `NavStep`, `ToolPick`).

| Store (`ui/composables/`) | State | Events | Crosses via effect |
|---|---|---|---|
| `WindowStore` (window-kit's composable, compiled in with the chrome) | open menu, pane flags, window-control error | the kit's `Win*` events | `WinAct` → `svc.settings.set("window.control", …)`; `WinMenuPick` → `NavPick`, `WinNav` → `NavStep`, `WinTool` → `ToolPick` (effects into this app's stores) |
| `NavStore` (`nav.store.nx`) | lit place, path, folder label, one-level back history | `NavPick`, `NavStep`, `OpenDir` | → `ClearSelection`, `Reload` |
| `ListingStore` (`listing.store.nx`) | entries, loading, error, entry count | `LoadFiles` (root), `Reload`, `Relist`, `FilesLoaded`, `FilesFailed`, `CountLoaded` | `FilesLoaded` → `svc.files.count` |
| `SelectionStore` (`selection.store.nx`) | the selected entry (id, name, kind, size, date) | `Pick`, `ClearSelection` | — |
| `ToolbarStore` (`toolbar.store.nx`) | pressed tool, view mode, search field + query | `ToolPick`, `SearchChanged`, `SearchClear` | → `Relist`/`Reload` |
| `ViewStore` (`view.store.nx`) | sort key and direction, hidden files, settings mode | `CycleSort`, `SortDir`, `SortBy`, `ToggleHidden`, `SettingsOpen`, `SettingsClose` | → `WinMenu("")`, `Reload` |
| `OpsStore` (`fileops.store.nx`) | the move dialog | `NewFolder`, `TrashSel`, `RestoreSel`, `DeleteSel`, `CopySel`, `MoveStart`, `MoveTo`, `ModalClose` and their completions | → `WinMenu("")`, `ClearSelection`, `Reload` |

`Reload` marks the list loading and lists; `Relist` lists the same folder without the loading flash.
Both run the one `svc.files.list` effect, which reads the path, sort key, filter and hidden switch from
their own stores. A selected folder's next tap dispatches `OpenDir` instead of `Pick`: the row decides
(its `opens` prop), because the choice cannot be made in an effect.

## Services

`svc.files.list`, `count`, `mkdir`, `rename` (trash, restore, move), `remove`, `copy` — vfsd over the
nxfs home (`FILES` capability, ceiling-gated to the `filemanager` role); `svc.settings.set` for the
window controls (through the kit).

## Window

`Window { style: plain, mode: freeform, resizable: true }`. Freeform on purpose: a fullscreen window
paints an opaque base and loses the wallpaper behind the glass the design draws.

## Structure

```
ui/pages/StashPage.nx               top bar + the kit's three-zone window + the dialogs
ui/components/sidebar/              StashSidebar (places, account footer)
ui/components/browser/              ContentPane, BrowserBody, ColumnHeader, FileList/FileRow(+Body),
                                    FileGrid/FileTile(+Body), ListingState (loading, error, empty, no hits)
ui/components/header/               Crumbs, ListingCount, SortButtons/SortButton, SearchStrip
ui/components/properties/           PropsPane
ui/components/actionbar/            StashActionBar, IdleBarItems, SelectionBarItems, BarFace
ui/components/menus/                MoreMenu, FileMenu, ViewMenu, CheckRow, SelectionMenu
ui/components/dialogs/              MovePicker, MoveDest
ui/components/settings/             SettingsTitle, SettingsBack, SettingsView
ui/composables/*.store.nx           the seven stores above
i18n/de.json, en.json (+ ja, ko, zh)  the copy; the count is two keys (`stash.countOne`, `stash.countMany`)
```

## Tests

`tests/dsl_apps_conformance/tests/stash.rs`: panes and materials, the properties pane and the action
bar following the selection, settings mode replacing the content, the search field (opens, clears,
claims text focus across the whole strip, never opens at mount), the list/grid switch. The window
menu, arena and frame-budget tests cover it too. Run with `cargo test -p dsl_apps_conformance --test stash`.

## Known limits

Sorting is by column; the direction button flips a view-only flag. The forward button is drawn disabled (one
level of history, no forward stack). The special-folder artwork, the properties of the open folder, multi-selection and
the settings rows for file extensions, folders-first and the default view need state the file service
does not provide yet. New window, Open, Compress, Folders first and Help are shown disabled; the chord hints in the menus
(Ctrl+N, Ctrl+O, Del, Ctrl+H, Ctrl+,) are labels only until the window routes the chords. The breadcrumb is three
levels deep (This PC › previous › current), not the full path. The window kit has no segmented view
switch and no adaptive grid.
