# desktop-shell — DSL app

The shell: the desktop itself — the top bar with its pills and the island, the icon field, the
drop-down panels (Control Center, notifications, calendar, Wi-Fi, sound, battery), the
launcher, the dock or taskbar, the shell search, the screenshot tool and the tiling drop hint
(design artifacts: launcher, panels, TaskbarDock, ActivityRunner). It is chromeless and full
screen on the desktop band, on a transparent base over the wallpaper plane. Two independent
axes shape it: the desktop MODE (the Control Center's toggle, a platform override of the home
page) and the WIDTH class (`device.sizeClass`: compact phone, regular tablet portrait, wide
landscape).

## Routes

`ui/pages/Routes.nx`: `"/" -> ShellPage` (the home: thin composition of the pieces below;
`ui/platform/desktop/pages/ShellPage.nx` is the desktop mode's override with the taskbar) and
`"/control" -> ControlCenterPage` (the Control Center as a page). The launcher and the panels
are overlays on the home, not routes: one panel at a time (`PanelStore.panel`).

## Stores and events

| Store (file) | State | Events |
|---|---|---|
| `LauncherStore` (`launcher.store.nx`) | the installed apps (from the registry, filtered AT the service), the query, the launching app, the clock and date, the launcher's page | `LauncherEvent`: `Refresh`, `Loaded`, `LoadFailed`, `QueryChanged`, `Launch`, `LaunchDone`, `LaunchFailed`, `Activate`, `ActivateDone`, `WindowsMoved`, `PageNext`, `PagePrev`, `SetPage`, `PageSynced`, `PageSyncFailed` · `ClockEvent`: `Tick` |
| `PanelStore` (`panels.store.nx`) | which drop-down is open (one at a time; a second tap on its pill closes it) | `PanelEvent`: `SetPanel`, `PanelNoop` |
| `ControlStore` (`controlcenter.store.nx`) | theme and shell mode (requests to windowd), brightness, volume, mute | `ControlEvent`: `SyncMode`, `SetTheme`, `SetMode`, `SetBrightness`, `SetVolume`, `ToggleMute`, `Applied`, `ApplyFailed` |
| `SystemStore` (`power.store.nx`) | the energy profile, standby inhibit, the shutdown alert, the system toast, and the screenshot tool's state (`cap*`) that answers with that toast | `PowerEvent`: `PickProfile`, `ToggleNoStandby`, `PowerAsk`, `AlertConfirm`, `AlertCancel`, `ToastDismiss`, `WinNoop` · `CaptureEvent` (`capture.store.nx`): `CapOpen` … `CapNoop` · `CaptureSelectEvent` (`selection.store.nx`): `CapPickWindow`, `CapDragStart`, `CapDragMove`, `CapDragEnd` |
| `SearchStore` (`search.store.nx`) | the search field, its mode (apps · files · clipboard), the results | `SearchEvent`: `SearchOpen` … `SearchLaunchFailed` (16 cases) |
| `NotifStore` (`notifications.store.nx`) | which demo cards are dismissed, whether the silent tier is open | `NotifEvent`: `ClearAll`, `Dismiss`, `ToggleSilent`, `ToggleSheetSilent` |
| `CalStore` (`calendar.store.nx`) | the month the calendar shows | `CalEvent`: `CalPrev`, `CalNext`, `CalSelect` |
| `ConnStore` (`connectivity.store.nx`) | the connectivity tiles and the Wi-Fi list | `ConnEvent`: `ToggleWifi`, `ToggleBluetooth`, `ToggleAirplane`, `ToggleWiredOnly`, `PickNet` |
| `SoundStore` (`sound.store.nx`) | the picked output and input device | `SoundEvent`: `PickOutput`, `PickInput` |
| `MediaStore` (`media.store.nx`) | what plays, mirrored by the island, the mini player and the touch dock | — |

A reducer writes one store: what answers with the system toast (power, the screenshot tool)
lives in `SystemStore`, and the screenshot's events are split by domain across two files.

## Services

| Call | When |
|---|---|
| `svc.bundlemgr.enumerate(query)` | the launcher's and the search's app list (the registry filters) |
| `svc.ability.launch(id)` | a tile or a search result launches its app (abilitymgr decides, fail-closed) |
| `svc.shell.activate(id)`, `svc.shell.scrollToPage(k)` | a running app's tile; a page dot's glide |
| `svc.settings.set(key, value)` | theme, shell mode and window controls — windowd is the presentation authority |
| `svc.screencap.begin()`, `shoot(…)`, `shot(…)`, `cancel()` | the screenshot tool: freeze, save the selection, save at once, thaw |
| `svc.clipboard.list(query)`, `restore(seq)` | the search's clipboard mode |

The host ticks `ClockEvent::Tick` with the time-zone-converted wall time and pushes the window
list, the theme and the tiling drop hint.

## Window

`Window { style: plain, level: desktop, resizable: false }` — the desktop band, full screen
([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md)).

## Structure

```
ui/pages/ShellPage.nx, ControlCenterPage.nx   the home and the Control Center page
ui/platform/desktop/pages/ShellPage.nx        the desktop mode's home: the taskbar
ui/components/topbar/      the 36 px bar: pills (clock, status, battery, notifications), the island
ui/components/workspace/   the icon field (WorkspaceGrid, WorkspaceColumn)
ui/components/tiles/       an app's tile in the field and in the launcher
ui/components/launcher/    the launcher: windowed or fullscreen, its pages and page dots
ui/components/panels/      the drop-downs (PanelHost) and their bodies: calendar/, controlcenter/,
                           notifications/, parts/ (rows, toggles, meters shared by the panels)
ui/components/dock/        the dock per width class, the desktop taskbar, the mini player
ui/components/search/      the shell search: field, mode buttons, results
ui/components/capture/     the screenshot overlay, its hole and its panel
ui/components/overlays/    the tiling drop hint and the shell's other layers
ui/composables/            one file per domain (the table above)
i18n/                      de, en, ja, ko, zh
```

## Tests

In `tests/dsl_apps_conformance/tests/`: `shell_capture.rs` (the screenshot tool: freeze,
draw, pick a window, save, refuse), `shell_search_clipboard.rs` (the search and its clipboard
mode), `shell_panels.rs`, `shell_control_center.rs` and `shell_power_alert.rs` (the panels,
taps they absorb, the shutdown alert), `shell_hit_targets.rs` (the top bar's controls are
hittable), `shell_presentation.rs` (a theme change re-emits, never remounts),
`shell_tiling_lane.rs` (the QEMU lane's tap targets), `hover_sweep.rs`, and the budgets
`frame_arena_budget.rs`, `arena_invariant.rs`. In `userspace/dsl/runtime/tests/`:
`shell_window_feed.rs` (the window list the host pushes) and `shell_workspace_grid.rs` (the
icon field). Run them with `cargo test -p dsl_apps_conformance --test 'shell_*'` and
`cargo test -p nexus-dsl-runtime --test 'shell_*'`.

## Known limits

Demo content where no service exists yet: the notifications (no notification service), the
calendar's month (the DSL has no date arithmetic), the connectivity tiles and the Wi-Fi list
(no radio services) and the sound devices (no audio service) — each store says so, and none
claims a service was touched. The files mode of the search is inert. The background-jobs zone
(`dock/JobTile`) is built but not mounted: the shell has no jobs feed. The launcher's page size
and cell width are fixed per width class, as the DSL has no container measurement.
