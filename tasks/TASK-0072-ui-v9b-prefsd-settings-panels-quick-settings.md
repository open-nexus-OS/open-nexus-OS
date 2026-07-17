---
title: TASK-0072 UI v9b: settingsd typed registry (persisted via statefsd) + Options menu + settings panel + light/dark end-to-end
status: In progress
owner: @ui
created: 2025-12-23
updated: 2026-07-03 (full rewrite to IST + new scope; prefsd dropped for settingsd)
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Settings v2 design vocabulary: tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Settings v2 OS UI: tasks/TASK-0226-settings-v2b-os-settings-ui-deeplinks-search-guides.md
  - Persistence substrate (/state, Done): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Theme tokens baseline: tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - WM track twin (W-track): tasks/TASK-0070-ui-v8b-wm-resize-move-shortcuts-settings-overlays.md
  - Data formats rubric: docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - Testing contract: scripts/qemu-test.sh
---

## Rewrite note (2026-07-03)

The original draft proposed a new `prefsd` JSON store and quick-settings stubs. Its own update
note already deprecated that direction: the canonical substrate is **settingsd** (typed keys,
scopes, provider apply hooks — the `TASK-0225` vocabulary). Decision (2026-07-03): **extend the
existing `settingsd` crate; no prefsd.** Quick-settings stubs are dropped; the deliverable is a
real settings surface: an Options menu in the shell chrome opening a settings panel window, with
a light/dark appearance switch working end-to-end (typed key → persisted → applied live). This
task executes as the S-track of the combined window-management/settings track (`TASK-0070`
phases 1–7 = W-track; this file = phases 8–10).

## Progress ledger

- **Phase 8 service core DONE (in review, 2026-07-04)** — the typed registry service is built +
  host/riscv verified; the boot wiring (reboot-survival gate) is the one remaining focused pass.
  - `nexus-abi::settingsd` wire module: magic `ST`, v1, `OP_GET`/`OP_SET`, `TYPE_TEXT`, seed
    keys (`ui.theme.mode`, `ui.font.family`, prepared `ui.locale`), encode/decode for
    request + response. 4 golden-byte tests.
  - `settingsd/src/registry.rs` — `SettingsRegistry`: a `SPECS` table of `(key, default,
    validator)`; `get` serves the default until set; `set` is validate-then-store and reports
    whether the value changed; `to_prefs_blob`/`load_prefs_blob` persist only non-default
    overrides (a stale journal line is skipped, never bricks the registry). 5 tests.
  - `settingsd/src/os_lite.rs` — `service_main_loop` (mirrors sessiond): binds the server,
    loads persisted prefs at boot, serves GET/SET; a SET is atomic (validate → persist →
    apply). Markers `settingsd: load prefs (n=…)`, `settingsd: ready`,
    `settingsd: set key=… value=… persist=ok|fail`.
  - `settingsd/src/statefs_client.rs` — the first runtime statefsd client: the
    windowd→sessiond route + CAP_MOVE recipe, hand-encoding the stable statefs v1 frames, key
    `settingsd/prefs`, best-effort (unreachable statefsd degrades to code defaults).
  - Crate restructured dual-mode like sessiond: `no_std` under OS, `std` feature carries the
    legacy `InputSettingsSnapshot` (no external consumers) + CLI `run()`, `os-lite` feature
    carries the service. Registry on `alloc`.
  - Gates: 8 settingsd host tests, `nexus-abi` full suite green (incl. the 4 goldens), riscv
    `os-lite` check clean, host binary builds.
- **Phase 8 boot wiring DONE (in review, 2026-07-04)** — settingsd is now a booted service.
  The wiring proved fully declarative (RFC-0069), no orchestrator/endpoints surgery:
  discover-services auto-embeds it (it has the `nexus-service` metadata + cross-compiles);
  `service_topology` gained `ServiceId::Settingsd`, its `ServiceSpec` (server + statefsd route)
  and the `Settingsd→Statefsd` required route (the generic arm provisions the endpoint + route
  from the spec); `policies/base.toml` grants it `ipc.core`/`statefs.read`/`statefs.write`.
  Gates: nexus-init topology-consistency host tests, policyd host + riscv, nexus-init riscv,
  `GPU_MODE=virgl` build (init +28KB, the `settingsd: ready`/`load prefs` markers embedded).
  The reboot-survival gate needs a `SET` first — that arrives with the Phase 10 settings UI.

- **Settings UI opened (in review, 2026-07-04)** — the topbar **Edit** item now owns a
  dropdown with a **Settings** entry that opens a real **Settings window**. Non-parallel:
  - The window is a third `ShellWindow` (`WindowId::Settings`) — z-order, focus, click-to-raise,
    minimize/dock, drag and close come for free from the shared frame; it is a static glass
    panel (no scroll), fullscreen is a no-op (fixed-size surface). Atlas acquired on open /
    freed on close, like Search.
  - The body is the seed of the classic flat-framed panel: an "Appearance" section with sharp
    1px-framed Theme / Font rows (values read locally for now — `Dark` / the vendored face).
  - The topbar dropdown is generalized from an Apps-only flag to `open_topbar_menu:
    Option<usize>` (the open item's index); the Edit menu reuses the same `AppMenu` row model
    as the dynamic Apps menu, so one menu-bar component serves both. Clicking "Settings"
    dispatches to `toggle_settings()`.
  - Gates: windowd host tests green (no regression), riscv `os-lite` clean, `GPU_MODE=virgl`
    build green. Remaining for the Track DoD: the live light/dark toggle wired to settingsd
    (`windowd/src/settings_client.rs` GET/SET + the `Windowd→Settingsd` route/policy) and the
    dual theme tokens (Phase 9).

## IST (verified 2026-07-03)

- `source/services/settingsd/` exists but is only an input-settings snapshot feeder (~150 LOC,
  keyboard layout/repeat/pointer accel) — none of the typed Key/Value/scope/provider machinery.
- **Persistence works today**: `statefsd` is a journaled key-value store on virtio-blk
  (`OP_PUT/GET/DEL/SYNC/REOPEN`, policy-gated `statefs.write`) — the substrate `TASK-0225`
  plans to build `state:/prefs/*.nxs` on.
- `configd` is a separate config-snapshot authority (2PC reload). Not a prefs store; do not
  create a second parallel authority.
- **Theme**: authoring is complete (`nexus-theme` runtime + `resources/themes/{base,dark,light,
  highcontrast}.nxtheme.toml`, dark and light are full token sets), but the runtime side has
  exactly one hardcoded dark token table; windowd freezes the dark qualifier at build time into
  const colors, several surfaces use raw literals, and icons are baked with a fixed light tint.
  There is no runtime light/dark path. A `uitheme: switched (to=..)` marker exists.
- Service scaffolding pattern is established (sessiond template): nexus-abi wire module with
  golden-frame tests, `service_topology.rs` routes, bootstrap pre-minted endpoint pairs,
  `scripts/discover-services.sh`, `policies/base.toml`, embedded-TOML mini parser (no toml
  crate), and windowd's `session_client.rs` as the bounded request/reply client recipe.

## Goal

1. **settingsd typed registry** (Phase 8): extend `source/services/settingsd/` per the
   `TASK-0225` vocabulary — typed `Key { ns, kind, scope, default, doc }` with stable ordering,
   typed values, deterministic invalid/unknown errors, provider apply hooks (adapter invoked
   after a successful set), subscribe-by-prefix marker-only. Persistence: settingsd is a
   **statefsd client** writing a canonical snapshot to `state:/prefs/device.nxs` (user-scope
   shape `state:/prefs/user/<uid>.nxs` prepared), atomic write semantics, policy-gated.
   Wire protocol module in `nexus-abi` with golden-frame tests. Full service wiring
   (topology routes Settingsd→{Statefsd, Policyd}, Windowd→Settingsd; bootstrap pairs;
   discover-services; policy grants). Seed keys: `ui.theme.mode` (dark|light),
   `ui.font.family` (string); prepared but not wired: `ui.locale`, `mime.defaults.*`.
2. **Theme runtime** (Phase 9): both qualifier snapshots (dark + light) generated at build time
   from the theme manifests into runtime token tables; dual-tint icon bakes; raw color literals
   swept onto token lookups; a theme selector in windowd with full re-render on switch;
   `ui.theme.mode` applied at boot.
3. **Settings surface** (Phase 10, track DoD): the chrome topbar gains an **Options** menu with
   a **Settings** entry that opens the settings panel as a normal shell window (inherits
   z-order/focus/minimize/snap/scroll from the W-track). Panel styling: classic flat-framed
   desktop control-panel look — framed sections, crisp 1-px rules, immediate apply. Controls:
   **Appearance → Light/Dark** writes `ui.theme.mode` through a windowd settings client →
   settingsd validates + persists → apply hook notifies windowd → live theme switch + marker.
   **Fonts** row displays `ui.font.family` (applied at boot; live switch = follow-up).
   **Language** and **Default applications** rows are visible but disabled (prepared,
   out of scope).

## Non-Goals

- A new `prefsd` service (decision: settingsd is the substrate).
- Quick-settings overlay, Wi‑Fi/Bluetooth/brightness/volume backends, account system.
- Live font switching; language/locale switching; MIME default editing (rows prepared only).
- Kernel changes.
- searchd route registration / deep links (stays `TASK-0226` scope).

## Constraints / invariants (hard requirements)

- No company/product names in code/comments/docs/identifiers (the panel style inspiration is
  not named anywhere in the tree).
- Deterministic storage semantics: atomic snapshot write via statefsd, corrupt/missing snapshot
  falls back to defaults with a marker, bounded sizes.
- Typed keys only — no stringly JSON contract; unknown ns/key and type mismatch are
  deterministic errors with markers.
- Policy guardrails: settings writes policy-gated; only the settings surface path writes
  `ui.*` keys; all writes audited by marker.
- UI apply evidence must be visual (the same screen restyles), not marker-only.
- No `unwrap`/`expect`; no per-event heap allocations; honest markers.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- settingsd: key registration/order stability, type validation, canonical encoding stability,
  atomic-persist semantics, apply-hook ordering with mock adapters.
- nexus-abi settingsd module: golden-frame encode/decode tests.
- Theme: both qualifier tables resolve every token (completeness test); selector logic.
- Panel: layout/hit-test pure logic; settings round-trip against a mock client.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `settingsd: ready`
- `settingsd: load prefs n=..` (or the defaults-fallback marker)
- `settingsd: set ns=.. scope=.. apply=..`
- `windowd: options menu open` / `windowd: settings panel open`
- `uitheme: switched (to=light)` / `(to=dark)`

### Visual proof — required (user gtk boot)

- Options → Settings opens the panel as a window on the shared desktop.
- Toggling Light/Dark restyles the whole UI (chrome, windows, icons, text) live, no reboot.
- Rebooting preserves the chosen mode (persistence proof across restart).

## Touched paths (allowlist)

- `source/services/settingsd/` (typed registry + statefsd client + manifests)
- `source/libs/nexus-abi/` (settingsd wire module + goldens)
- `source/init/nexus-init/` (topology + bootstrap wiring), `scripts/discover-services.sh`,
  `policies/base.toml`
- `userspace/ui/theme/`, `userspace/ui/theme-tokens/` (dual snapshots)
- `source/services/windowd/` (settings client, options menu, settings panel window,
  theme selector + literal sweep, build.rs dual-tint bakes)
- `tasks/`, `docs/platform/settings.md`

## Plan (boot-gated phases; user boots + commits each)

8. settingsd typed registry + persistence + wire + wiring (parallelizable with W-track;
   gate: a set value survives reboot)
9. theme runtime: dual token snapshots + dual-tint icons + selector (gate: boot with
   persisted light mode renders the whole UI light)
10. Options menu + settings panel window + light/dark end-to-end (track DoD)

Relationship to `TASK-0225`/`TASK-0226`: this task ships the first real slice of the settingsd
direction (typed keys, apply hooks, statefsd persistence) and the first real settings surface;
0225's full schema/scope breadth and 0226's deep links/search/guides remain open on their own
timelines and must build on — not duplicate — what lands here.
