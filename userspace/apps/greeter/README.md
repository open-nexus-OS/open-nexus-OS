# greeter — DSL app

The lock screen and login: the time, the user the password unlocks, the password capsule, the
other users and the power buttons, on the wallpaper (design artifacts: lock screen). The
greeter renders the login contract of `docs/dev/ui/shell/session.md` — authority stays in
sessiond: the greeter asks (`svc.session.*`), shows the answer, and decides nothing.

## Routes

One page: `"/" -> GreeterPage` (`ui/pages/Routes.nx`, `ui/pages/GreeterPage.nx`).

## Stores and events

| Store (file) | State | Events |
|---|---|---|
| `SessionStore` (`ui/composables/session.store.nx`) | the users (`{id, label}` records from sessiond), the selected user's id, the typed secret, the phase (0 idle · 1 authenticating · 2 failed), the last error, the clock and the date | `SessionEvent`: `Load`, `UsersLoaded`, `UsersFailed`, `ActiveLoaded`, `Pick`, `Submit`, `LoginOk`, `LoginFailed` · `ClockEvent`: `Tick(time, date)` |

`Load` is a root effect (nothing dispatches it), so it runs once at mount. The secret is cleared
on every answer and on a user switch, and nothing logs it.

## Services

| Call | When |
|---|---|
| `svc.session.users()` | at mount: the users to show |
| `svc.session.active()` | at mount: which user is pre-selected (sessiond's answer, never "the first one") |
| `svc.session.login(id, secret)` | on `Submit` |

The host ticks `ClockEvent::Tick` once a minute with the time-zone-converted wall time.

## Window

`Window { style: plain, mode: auto, level: desktop, resizable: false }` — the session's first
surface, full screen on the desktop band ([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md)).

## Structure

```
ui/pages/GreeterPage.nx              the composition: clock · login column · switcher and power row
ui/components/clock/LockClock        date and time (128 px `clock` style; 64 px on compact)
ui/components/login/SelectedUser     the selected user's avatar and name
ui/components/login/PasswordField    the secure field and the submit arrow; shakes on a failure
ui/components/login/LoginStatus      hint · authenticating · failed
ui/components/switcher/UserSwitcher  the other users; a tap picks one
ui/components/power/PowerButtons     sleep · restart · shut down (one PowerButton each)
ui/composables/session.store.nx      the session state and the calls to sessiond
i18n/de.json, en.json, ja.json, ko.json, zh.json
```

## Tests

`userspace/dsl/runtime/tests/greeter_surface.rs` (the 128 px clock on a regular screen),
`tests/dsl_apps_conformance/tests/long_session.rs` (a long session's arenas stay bounded),
`arena_invariant.rs` and `apps_compile.rs` (it compiles, mounts and renders). Run them with
`cargo test -p nexus-dsl-runtime --test greeter_surface` and
`cargo test -p dsl_apps_conformance --test long_session --test arena_invariant`.

## Known limits

The power buttons are drawn disabled: the greeter holds no power permission and no service backs
sleep, restart or shut down from the lock screen yet. There is no PIN unlock (decision Q14) and no
reveal button in the password field (decision Q4).
