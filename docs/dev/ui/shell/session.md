# Session management and the login greeter

**Status:** Implemented (TASK-0065B, 2026-07-02) — greeter → login → session shell,
manifest-driven, with authority-side launch gating. Auth (password/keys) is a
designed-in seam, not yet implemented.

## Authority split (ADR-0036 discipline: one authority per service)

| Concern | Authority | Where |
|---|---|---|
| WHO exists, WHICH session is active | `sessiond` | `source/services/sessiond/` (state machine `state.rs`, user registry `users.rs`) |
| What the login window LOOKS like | SystemUI (library) | `source/services/systemui/src/greeter.rs` + `manifests/greeter/default/greeter.toml` |
| What a user's session means as a SHELL | SystemUI (library) | `resolve_product(product_id)` — the user's `product` string names a product manifest |
| Rendering, hit-testing, input relay | `windowd` | `compositor/runtime/greeter.rs`, `session_client.rs`, `interaction.rs` |
| Pre-session launch enforcement | `abilitymgr` | `handoff.rs` `SessionGate` + os-lite live gate |

windowd renders and relays only — it never forges session state. sessiond never
knows what "desktop chrome" is — it stores an opaque SystemUI product id.

## Wire protocol (`nexus_abi::sessiond`, magic `S,N`, v1, request/reply only)

- `OP_GET_STATE (1)` → `[status, state(0=greeter,1=active,2=locked), active_idx, count, entries…]`,
  entry = `[id_len,id, name_len,name, product_len,product]` (UTF-8).
- `OP_LOGIN (2)`: `[id_len, id…]` → `[status, product_len, product…]`.
  Auth docks HERE later (credential payload + keystored verification); the
  `Locked` state and `OP_LOCK (3)` (reserved, `STATUS_UNSUPPORTED`) pin the
  lock-screen seam.
- Statuses: OK, MALFORMED, UNSUPPORTED, UNKNOWN_USER, WRONG_STATE.
- Codecs host-tested with golden frames in `nexus-abi`.

## Manifests

- **Users** (`source/services/sessiond/manifests/users.toml`): one
  `[user.<id>]` section per user (`display_name`, `product`); optional
  `[session] auto_login = "<id>"` runs the SAME `login()` transition without a
  greeter (proof lanes, kiosk deployments). Validation: ≥1 user, unique ids,
  non-empty fields, auto_login must be registered.
- **Greeter appearance**
  (`source/services/systemui/manifests/greeter/default/greeter.toml`):
  `blur_radius`, `dim`, avatar `diameter`/`ring_stroke`/`label_gap` — bounded
  by `validate_greeter`; windowd falls back to `GreeterConfig::fallback()` on
  any manifest error (and the shipped manifest is host-tested to parse).

## Flow

1. Boot: windowd brings up the desktop base (wallpaper, present) exactly as
   before; after the framebuffer handoff its session probe asks sessiond
   (250 ms cadence, ~6 s bound).
2. `sessiond: greeter (n=…)` → windowd bakes the greeter INTO Plane 1: one-time
   separable box blur of the wallpaper + dim, centered avatar (SDF circle +
   ring + Lucide `circle-user` + name) → `windowd: greeter visible`. No atlas
   use; hover redraws only the card from a saved backdrop.
3. While the greeter owns the display, ALL shell affordances are dead
   (host-tested `interaction::resolve_click_session`): no topbar, no corner
   hotspot, no windows. Additionally `abilitymgr` refuses `OP_LAUNCH` with
   `STATUS_DENIED` unless sessiond reports an active session (fail-closed;
   marker `abilitymgr: launch denied (session)`).
4. Avatar click → `OP_LOGIN` → `sessiond: session start (user=… product=…)` →
   windowd restores the pristine base, resolves the product through SystemUI
   and applies it → `windowd: session shell visible (product=…)`.
5. sessiond unreachable (e.g. removed from the image): bounded probe, then
   `windowd: session unavailable (auto shell)` — today's default shell; the
   boot NEVER bricks on the session authority.

## Marker ladder

`sessiond: ready` → `sessiond: greeter (n=…)` (or `… session start (… auto)`) →
`windowd: greeter visible` → `sessiond: session start (user=… product=…)` →
`windowd: session shell visible (product=…)`. Wiring: `init: windowd
route->sessiond ok`, `init: abilitymgr route->sessiond ok`. Degradations:
`windowd: session unavailable (auto shell)`, `windowd: login failed`,
`abilitymgr: launch denied (session)`, `abilitymgr: session gate unreachable (deny)`.

## Proof lanes

The interactive proof injector (`tools/qmp_visible_input_inject.py`) logs in
exactly like a user: it waits (bounded) for `windowd: greeter visible`, clicks
the display center, waits for `windowd: session shell visible`, then runs the
normal choreography. Lanes that need an unattended shell set `auto_login` in
the users manifest instead. The required marker ladder includes
`sessiond: ready` + `windowd: greeter visible`.

## Follow-ups (deliberately out of scope)

Credential auth behind OP_LOGIN (keystored-backed), lock/unlock UI (`Locked`
reserved), session switching, multi-user avatar grid, per-app session scoping
(TASK-0080D app runtime).
