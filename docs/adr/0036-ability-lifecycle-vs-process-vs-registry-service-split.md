# ADR-0036: App lifecycle, process spawn, and app registry are three separate services — `abilitymgr`, `execd`, `bundlemgrd` (no new `appmgrd`)

- Status: Accepted. Establishes the service split for UI v6b (RFC-0065 / TASK-0065). No code yet —
  this records the decision so the rejected `appmgrd` name is never reintroduced.
- Created: 2026-06-22
- Builds on: ADR-0017 (service architecture), RFC-0002 (process-per-service), RFC-0064 (UI v6a WM).
- Contract: `docs/rfcs/RFC-0065-ui-v6b-app-lifecycle-registry-notifications-navigation-contract.md`
- Execution (SSOT): `tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md`

## Context

TASK-0065's draft (Dec 2025) specified an `appmgrd` (new) service for the "ability-lite lifecycle".
At implementation time the tree already contains services that cover the surrounding concerns:

- `bundlemgrd` — functional bundle/app registry (install/query/get_payload; manifests carry
  `abilities` + `capabilities`).
- `abilitymgr` — present but a **stub** (a CLI `--help` handler; "abilitymgr manages ability
  lifecycle").
- `execd` — process spawner (our appspawn/launchd).
- `windowd` — window/surface server.
- `notifd` — notification service (skeleton).
- `samgrd` — system-**service** registry (not apps).

Introducing a new `appmgrd` would overlap `abilitymgr` (lifecycle) **and** `execd` (process spawn),
producing a double structure with unclear ownership of "who spawns" and "who owns lifecycle state".

## How the production OSes split it

Both reference architectures keep three distinct authorities, which our tree already mirrors:

| Concern | Reference system A | Reference system B | **Open Nexus** |
|---|---|---|---|
| Static registry — which apps/abilities exist; enumerate | BMS (`bundlemgr`) | LaunchServices / `lsd` | **`bundlemgrd`** |
| Ability lifecycle — running abilities, FG/BG, recents, focus mediation | AMS (`AbilityManagerService`) | FrontBoard / SpringBoard | **`abilitymgr`** |
| Process lifecycle — spawn/kill the OS process | AppMgrService + `appspawn` | `launchd` + `xpcproxy` | **`execd`** |

Crucially, a reference system's **app-manager** is the *process* layer (≈ our `execd`/spawn), **not** a third
app-management daemon. The thing the draft called "appmgrd ability-lite lifecycle" is, line for line,
the **scene-lifecycle** role in reference systems — i.e. **our `abilitymgr`**.

## Decision

1. The **ability-lifecycle broker lives in `abilitymgr`** (flesh out the existing stub). It owns the
   lifecycle state machine (Create→Start→Foreground/Background→Suspend/Resume→Stop), the recents
   list, and focus mediation with windowd.
2. The **app registry stays in `bundlemgrd`**. It gains an `enumerate`/`list_apps` op returning
   `AppRecord`s — the single source of "which apps exist". Launcher/SystemUI **query** it; no app list
   is ever hardcoded.
3. **Process spawn stays in `execd`**. `abilitymgr` is the only caller permitted to spawn apps, and it
   does so via execd — it does not reimplement spawn.
4. **No new `appmgrd` service is created.** The name is retired to avoid the AMS/AppMgr conflation.
5. One authority per service: registry (`bundlemgrd`) vs. lifecycle (`abilitymgr`) vs. process-spawn
   (`execd`) vs. windows (`windowd`) vs. notifications (`notifd`) vs. system-service registry
   (`samgrd`). No service wears two hats.

## Consequences

- **Positive**: matches the reference-system production split; no duplicated spawn logic; clear handoff
  chain (SystemUI request → abilitymgr lifecycle → bundlemgrd resolve → execd spawn → windowd surface
  → abilitymgr focus) that the `tools/nx` chain test can assert by hop order.
- **Positive**: the existing `abilitymgr` and `notifd` slots are filled rather than duplicated; the
  TASK-0065 allowlist is corrected (`appmgrd/ (new)` → `abilitymgr/ (flesh out)`).
- **Cost**: `abilitymgr` graduates from stub to a real service with its own tests/markers; the draft's
  `appmgrd` markers are renamed to `abilitymgr:` in the marker ladder.
- **Risk (cooperative lifecycle)**: lifecycle is userspace policy, not kernel-enforced; a
  non-cooperative app can ignore callbacks. Documented as cooperative until Ability v1.1
  (TASK-0234/0235) adds confinement.
