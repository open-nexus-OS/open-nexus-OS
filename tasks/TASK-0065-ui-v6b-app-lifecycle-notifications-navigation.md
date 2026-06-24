---
title: TASK-0065 UI v6b: appmgrd ability-lite lifecycle + SystemUI navigation + notifd toasts/notifications
status: Done
owner: @ui
created: 2025-12-23
updated: 2026-06-23
depends-on:
  - tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md   # WM baseline (ShellWindow N-window + focus)
  - docs/rfcs/RFC-0002-process-per-service-architecture.md          # process-per-service; execd is the spawner
  - source/services/bundlemgrd/                                     # installed-app registry (the "which apps exist" SSOT)
  - source/services/execd/                                          # process spawn (our appspawn/launchd)
  - tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md  # notification limits via configd
  - tasks/TASK-0047-policy-as-code-v1-unified-engine.md             # launch/notify policy guards
follow-up-tasks:
  - tasks/TASK-0065B-session-login-greeter-v0.md                    # session/login authority (kept separate from lifecycle)
  - TASK-0234   # Ability/Lifecycle v1.1 (backoff/crash-loop/kill reasons/FG-BG policies) — host-first
  - TASK-0235   # OS extension of the lifecycle broker (abilitymgr)
links:
  - Design contract (RFC): docs/rfcs/RFC-0065-ui-v6b-app-lifecycle-registry-notifications-navigation-contract.md
  - Service-split decision (ADR): docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - Process-per-service: docs/rfcs/RFC-0002-process-per-service-architecture.md
  - Execd supervisor: source/services/execd/
  - Config broker (notification limits): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (launch/notify guards): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Updates (future integration): tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Testing contract: scripts/qemu-test.sh
---

## Closure (2026-06-23) — DONE

The v6b lifecycle spine shipped and is proven on QEMU + host tests:

- **Registry, not hardcoded:** `bundlemgrd` serves the installed-app set from `OP_LIST_APPS`, generated
  at build time from the real `bundles/<app>/manifest.toml` (`bundlemgrd/build.rs` → `APP_REGISTRY`; no
  hand-maintained list, no phantom apps — `notes` placeholder removed). windowd builds the dynamic Apps
  menu from it (`windowd: apps ok (n=…)`).
- **Lifecycle broker is a real service:** `abilitymgr` (not a new `appmgrd`) — live registry probe
  (`abilitymgr: registry ok (n=…)`), lifecycle state machine, recents.
- **Launch authority enforces manifest caps:** `abilitymgr::caps` + `Broker::launch_with_caps` fail closed
  on an unknown permission (`STATUS_DENIED`); boot self-check `abilitymgr: caps ok app=<id> (n=…)`.
- **Capability-gated routing:** registry access goes through `policyd` (`BundleQuery`); declarative policy
  SSOT in `policies/base.toml`; greppable `!route-deny` / `!cap-deny` markers.
- **Real `.nxb` bundles + Cap'n Proto manifests** for chat/search; per-app-surface model (ADR-0037)
  host-proven; `search-app` owns its data (no_std), windowd hosts it.

**Descoped to follow-ups (not v6b):** apps as separately *spawned processes* with their own surfaces
needs a userspace app runtime (today `execd` only runs hand-assembled stubs) + the surface handoff. That
is now the DSL App Runtime track — **`TASK-0080D`** (app-host + lifecycle/registry/caps bridge + per-app
surface) — plus `TASK-0234`/`TASK-0235` (lifecycle v1.1 / OS extension) and the SystemUI DSL phases.
notifd toasts/navigation transitions likewise continue under their own tasks. Design contract
**RFC-0065 is Done** (seed satisfied; SSOT was this task).

## Context

With a WM in place (v6a), we can implement a minimal “Ability-Lite” lifecycle broker and wire
SystemUI navigation and notifications.

> **Service split (ADR-0036 / RFC-0065):** the lifecycle broker lives in the existing **`abilitymgr`**
> service (today a stub), **not** a new `appmgrd`. OpenHarmony's *AppMgr* is the process layer
> (≈ our `execd`/appspawn); the ability-lifecycle layer is *AMS* = our `abilitymgr`. The static
> "which apps exist" registry is **`bundlemgrd`** (gains an enumerate op so the launcher/SystemUI
> learn the app set at runtime — never hardcoded). One authority per service: registry
> (`bundlemgrd`) vs. lifecycle (`abilitymgr`) vs. process-spawn (`execd`) vs. windows (`windowd`)
> vs. notifications (`notifd`).

This is intentionally userspace-only: app lifecycle is brokered, not kernel-enforced.
It must close the app-launch part of the Orbital-Level UX gate: a live pointer click in
the launcher/SystemUI path starts an app and yields a visible focused app window in QEMU.
Login/session handoff is tracked separately by `TASK-0065B` so lifecycle and session
authority do not blur.

## Goal

Deliver:

0. App registry (`bundlemgrd`): `enumerate`/`list_apps` op returning `AppRecord`s — the static
   "which apps exist" SSOT; launcher + SystemUI query it instead of hardcoding an app list.
1. `abilitymgr` lifecycle broker (the broker formerly drafted as `appmgrd`):
   - resolve the app via `bundlemgrd`, launch via `execd` (only `abilitymgr` may spawn apps)
   - lifecycle callbacks: Create → Start → Foreground/Background → Suspend/Resume → Stop
   - recents list with metadata (thumbnails can be stubbed initially)
   - mediation with `windowd` WM: open window, bind surface, focus transitions
1b. Chat + Search become **real app processes** (`userspace/apps/{chat,search}`) presenting their own
   surfaces; windowd stops constructing the baked `ShellWindow` instances and hosts surface + chrome.
2. Notifications/toasts:
   - minimal `notifd` service (or extend if already present)
   - rate limit per app and priority
   - SystemUI plugin shows toasts and a small tray/shade stub
3. SystemUI navigation:
   - Back/Home/Recents stubs
   - focus switching across windows via WM/appmgrd
   - launcher click opens a demo app window through `appmgrd`, not by selftest-only state mutation
   - the demo app window/toast/nav proof stays on the shared visible proof surface
4. Host tests and OS markers.

## Non-Goals

- Kernel changes.
- Full multi-window apps.
- Real thumbnail capture pipeline (can be a follow-up once screencopy exists).

## Constraints / invariants (hard requirements)

- Deterministic lifecycle ordering and bounded timeouts.
- Policy guardrails:
  - only `appmgrd` may spawn apps
  - notification quotas per app
- Live app launch must preserve authorities: SystemUI requests launch, `appmgrd` owns lifecycle, `windowd` owns focus/window state.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (lifecycle authority)**:
  - This is userspace policy/contract; if an app ignores callbacks it can misbehave.
  - We must document this as “cooperative lifecycle” until stronger confinement exists.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v6b_host/`:

- lifecycle: mocked app receives callbacks in correct order (Create→Start→FG, BG/FG roundtrip)
- notifications: rate limiting drops are counted deterministically
- navigation: recents list and focus selection logic behaves as expected
- live QEMU pointer launcher click starts the demo app and focuses its window

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `bundlemgrd: ready`
- `bundlemgrd: enumerate ok (n=...)`
- `abilitymgr: ready`
- `abilitymgr: launch (app=..., pid=...)`
- `abilitymgr: fg (win=...)` / `bg (win=...)`
- `notifd: ready`
- `systemui: nav ready`
- `systemui: launcher click`
- `abilitymgr: live launch ok`
- `systemui: toast (app=..., id=...)`
- `notes: started` / `notes: paused` / `notes: resumed` (demo app)
- `SELFTEST: ui v6 launch ok`
- `SELFTEST: ui v6 lifecycle ok`
- `SELFTEST: ui v6 toast ok`

### Visual proof — required

- the shared proof surface shows launcher -> app-window launch on-screen,
- the demo app (`notes` or equivalent) appears as a real visible proof window,
- toast/navigation changes are visible on the same desktop/test screen rather than marker-only lifecycle evidence.

## Touched paths (allowlist — reconciled 2026-06-22, see ADR-0036)

> Service-split decision (ADR-0036): the lifecycle broker is **`abilitymgr`** (flesh out the existing
> stub), **not** a new `appmgrd` — a new `appmgrd` would double-structure with `abilitymgr`
> (lifecycle) and `execd` (process spawn). The "which apps exist" registry is **`bundlemgrd`**
> (add an enumerate op), not a new service.

- `source/services/abilitymgr/` (flesh out: ability-lifecycle broker — was drafted as `appmgrd`)
- `source/services/bundlemgrd/` (add `enumerate`/`list_apps` op + `AppRecord` — the registry SSOT)
- `source/services/notifd/` (extend: per-app rate limit + priority)
- `source/services/windowd/` (host client surfaces + WM mediation; remove baked chat/search)
- `source/services/execd/` (spawn wiring — abilitymgr is the only app spawner)
- `source/services/samgrd/` (service discovery as needed)
- `bundles/{chat,search,notes}/manifest.toml` (new: real `.nxb` app bundle manifests — Cap'n Proto via nxb-pack)
- `tools/nxb-pack/tests/repo_bundles.rs` (new: bundle pack+parse proof) + `just pack-bundles` recipe
- `source/services/abilitymgr/src/handoff.rs` (new: launch-handoff orchestrator)
- `source/services/windowd/src/app_surface.rs` (new: per-app surface lifecycle model — ADR-0037) + `windowd::destroy_surface`
- `docs/adr/0037-per-app-surface-lazy-vmo-lifecycle.md` (new: own-VMO-per-app lazy surface decision)
- `userspace/apps/chat/` (new: chat extracted into a real app — P4)
- `userspace/apps/search/` (new: search extracted into a real app — P4)
- `userspace/apps/notes/` (demo app, minimal)
- `userspace/apps/launcher/` (query registry + launch via broker, not hardcoded)
- `source/apps/selftest-client/` (markers)
- `tools/nx/src/chain/contract/` + `tools/nx/tests/chain_app_lifecycle.rs` (integration chain + hop markers)
- `tests/ui_v6b_host/` (new)
- `tools/postflight-ui-v6b.sh` (delegates)
- `docs/dev/ui/patterns/app-structure/lifecycle.md` + `docs/dev/ui/status/notifications.md` (new)

## Plan (phased — each ends at a boot checkpoint; aligns with RFC-0065 §Status at a Glance)

- **P0** — RFC-0065 seed + ADR-0036 (service split) + depends-on/follow-up/allowlist wiring. *(done 2026-06-22)*
- **P1** — `bundlemgr` `enumerate`/`enumerate_apps` + `AppRecord` projection + `bundlemgrd` capnp `OPCODE_ENUMERATE` (marker `bundlemgrd: enumerate ok (n=…)`); host-tested (domain + opcode roundtrip). *(done 2026-06-22; OS-lite binary-frame enumerate deferred to P5/boot — os-lite registry is still a placeholder.)*
- **P2** — `abilitymgr` lifecycle broker, promoted from CLI stub to a **real service like the others** (rngd-shaped `lib`/`os_lite`/`std_impl` + `nexus-service` metadata → auto-discovered into the boot-order SSOT `scripts/discover-services.sh`). Pure host-tested lifecycle state machine + recents + wire dispatch + OS loop emitting `abilitymgr: ready/launch/fg/bg`. *(done 2026-06-22; 18 host tests + riscv os-lite check green. Live resolve-via-bundlemgrd + spawn-via-execd + windowd bind moved to P3 so the authority handoff is wired in one place.)*
- **P3** — launch handoff + **real app bundles**: pure `abilitymgr/handoff.rs` orchestrator (resolve→spawn→bind→focus + rollback, injected `AppResolver`/`Spawner`/`SurfaceBinder` traits, host-tested) + `bundles/{chat,search,notes}/manifest.toml` → `nxb-pack` → `.nxb` (Cap'n Proto `manifest.nxb`, the resolve source) + `tools/nxb-pack/tests/repo_bundles.rs` + `just pack-bundles`. *(done 2026-06-22; 21 abilitymgr + 3 nxb-pack tests green, riscv-checked.)* Live OS clients (execd `Spawner`, windowd `SurfaceBinder`, bundlemgrd `AppResolver`) land with P4 (apps presenting surfaces) + the os-lite bundlemgrd enumerate (P5).
- **P4a** — **per-app surface model** (ADR-0037): each app owns its VMO, lazily allocated when active + freed when closed, composited as its own layer (NOT the shared atlas). `windowd::destroy_surface` (free-on-close) + host-tested `app_surface::AppSurfaces` (instance→own-surface, lazy mount/unmount, z-ordered layers, bounded). *(done 2026-06-22; windowd 115 host tests, riscv-checked.)*
- **P4b** — **full extraction** of chat + search into `userspace/apps/{chat,search}` (search first, then chat):
  - *b1 (done 2026-06-22):* `userspace/apps/search` (`search-app`) — owns its word list + filter + geometry + renders its **own** surface buffer (no windowd dep); 10 host tests. Added to workspace members (the `userspace/apps` exclude requires explicit listing, like `launcher`).
  - *b2 (boot-gated, next):* the compositor composites the search **client** surface (remove the baked `self.search` ShellWindow instance) driven by `AppSurfaces`; the abilitymgr `SurfaceBinder` allocates/frees the surface over windowd IPC on launch/stop. Then chat (b3). windowd = surface-host + chrome only.
- **P5** — `notifd` rate-limit + SystemUI Back/Home/Recents + launcher queries registry → launches via broker + demo `notes` app.
- **P6** — `tools/nx/tests/chain_app_lifecycle.rs` (hop markers, authority-order) + `tests/ui_v6b_host/` + postflight + docs.

## Follow-ups

- Ability/Lifecycle v1.1 (backoff/crash-loop/kill reasons/FG-BG policies): `TASK-0234` (host-first) and `TASK-0235` (OS extension of appmgrd).
