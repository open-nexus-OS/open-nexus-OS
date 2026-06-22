# RFC-0065: UI v6b — App Lifecycle + App Registry + Notifications + Navigation Contract

- Status: Draft (seed contract — execution truth lives in TASK-0065)
- Owners: @ui @runtime
- Created: 2026-06-22
- Last Updated: 2026-06-22 (seed)
- Links:
  - Tasks: `tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md` (execution + proof — **SSOT** for stop conditions)
  - Depends on: `docs/rfcs/RFC-0064-ui-v6a-window-management-chat-window-contract.md` (WM baseline — ShellWindow N-window model + focus)
  - Depends on: `docs/rfcs/RFC-0002-process-per-service-architecture.md` (process-per-service; execd is the spawner)
  - Depends on: `source/services/bundlemgrd/` (installed-app registry — the static "which apps exist" SSOT)
  - Depends on: `source/services/execd/` (process spawn — our appspawn/launchd)
  - Related: `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` (notification limits via configd)
  - Related: `tasks/TASK-0047-policy-as-code-v1-unified-engine.md` (launch/notify policy guards)
  - Decision record: `docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md` (why `abilitymgr`, not a new `appmgrd`)
  - Follow-up: `tasks/TASK-0065B-session-login-greeter-v0.md` (session/login authority — kept separate from lifecycle)

## Status at a Glance

- **Phase 0 (Registry enumeration)**: 🟢 (host/std path) — `bundlemgr` domain gained `enumerate()`/`enumerate_apps()` + the `AppRecord` projection; `bundlemgrd` exposes capnp `OPCODE_ENUMERATE` (marker `bundlemgrd: enumerate ok (n=…)`). Host-tested (domain + opcode roundtrip). The OS-lite binary-frame `enumerate` (real boot app source for the launcher) is deferred to P5/boot — the os-lite registry is still a placeholder.
- **Phase 1 (Lifecycle broker)**: 🟢 (broker core + real service) — `abilitymgr` was promoted from a CLI stub to a **real service like the others** (rngd-shaped: `lib`/`os_lite`/`std_impl` + `[package.metadata.nexus-service]`, auto-discovered into the boot-order SSOT). Ships the pure host-tested lifecycle state machine (Create→Start→FG/BG→Suspend/Resume→Stop) + recents + a wire dispatch + the OS service loop emitting `abilitymgr: ready` / `launch` / `fg` / `bg` markers. **Live resolve-via-bundlemgrd + spawn-via-execd + windowd surface bind is P2 (WM mediation) — deferred there to keep authorities' wiring in one place.**
- **Phase 2 (WM mediation + launch handoff)**: 🟡 (orchestrator + app bundles done; live OS clients pending) — the pure **launch-handoff orchestrator** (resolve→spawn→bind→focus, with rollback) lives in `abilitymgr/handoff.rs`, host-tested with injected `AppResolver`/`Spawner`/`SurfaceBinder` traits proving the authority order. The **real app bundles** ship: `bundles/{chat,search,notes}/manifest.toml` → `nxb-pack` → `.nxb` with Cap'n Proto `manifest.nxb` (the resolve source — `bundlemgrd` enumerates them, `abilitymgr` resolves the launch ability). The live OS clients are sequenced behind their deps: the execd `Spawner` + windowd `SurfaceBinder` land with Phase 3 (apps presenting surfaces); the bundlemgrd `AppResolver` needs the os-lite enumerate (Phase 4/boot).
- **Phase 3 (Chat + Search as real apps)**: 🟡 (per-app-surface foundation done; extraction pending) — the **per-app surface model** landed (ADR-0037): `windowd::destroy_surface` (free-on-close) + the host-tested `app_surface::AppSurfaces` registry (each app its **own VMO**, lazily allocated when active, freed on stop, composited as its **own layer** with z-order — never the shared atlas plane). Next (P4b): the compositor stops baking chat/search into the atlas and composites per-app client surfaces, and chat/search become real `userspace/apps/{chat,search}` processes presenting into their own surface.
- **Phase 4 (Notifications + Navigation)**: ⬜ — `notifd` rate-limited toasts; SystemUI Back/Home/Recents; launcher queries the registry (Phase 0) and launches via the broker (Phase 1); demo `notes` app.

Definition: a phase is 🟢 when its **contract** is defined here and its **proof gates** (TASK-0065 host tests, OS markers, the `tools/nx` chain test) are green. "Done" does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Stop conditions, proof commands, plan ordering, and touched paths live exclusively in **TASK-0065** (the SSOT).

- **This RFC owns**:
  - The **service split contract**: registry vs. lifecycle vs. process-spawn vs. windows vs. notifications — one authority per service, no service wearing two hats.
  - The `AppRecord` shape returned by the registry enumeration.
  - The ability **lifecycle state machine** + ordering invariants.
  - The **launch handoff contract**: SystemUI → abilitymgr → (bundlemgrd resolve) → execd spawn → windowd surface-bind → abilitymgr focus.
  - The chat/search **app-extraction contract**: windowd hosts client surfaces, apps own content.
  - The **per-app surface lifecycle** (ADR-0037): each app owns its surface VMO, lazily allocated when active and freed when closed, composited as its own layer — never a shared plane.
  - The notification rate-limit contract (per-app quota + priority).
  - The deterministic UART **marker ladder** + the `tools/nx` chain ordering.

- **This RFC does NOT own**:
  - Kernel changes (lifecycle is cooperative userspace policy, not kernel-enforced).
  - Session/login authority — that is **TASK-0065B** (deliberately separate so lifecycle ≠ session).
  - Real thumbnail/screencopy capture (recents thumbnails are stubbed until screencopy exists).
  - Full multi-window-per-app, resize, minimize/maximize.
  - Backoff/crash-loop/kill-reason policy (Ability v1.1 — TASK-0234/0235).
  - Install/sign/supply-chain of bundles (already owned by bundlemgrd/policyd).

### Relationship to tasks (single execution truth)

- **TASK-0065** is the SSOT for stop conditions, proof commands, plan ordering, and touched paths.
- This RFC owns the stable contracts and invariants that TASK-0065 implements.

## Context

UI v6a (RFC-0064) gave us an N-window WM: chat and search are `ShellWindow` instances **baked
inside windowd's compositor** — windowd constructs them and renders their content itself. That was
the right move to land the WM, but it is not how a real OS runs apps: in a production system the
window server hosts *client* surfaces, and a separate authority owns *which apps exist* and *what
state each running app is in*.

Two production OSes already solved this exact split, and our service tree already mirrors them —
the v6b job is to **fill the existing slots**, not invent a new service:

| Concern | OpenHarmony | Apple | **Open Nexus (this tree)** |
|---|---|---|---|
| Static registry — *which apps exist*, abilities, icons, install/query/**enumerate** | BMS (`bundlemgr`) | LaunchServices / `lsd` | **`bundlemgrd`** (functional; needs enumerate) |
| Ability lifecycle — running abilities, FG/BG, recents/mission, focus mediation | AMS (`AbilityManagerService`) | FrontBoard / SpringBoard | **`abilitymgr`** (stub → broker) |
| Process lifecycle — spawn/kill the OS process | AppMgrService + `appspawn` | `launchd` + `xpcproxy` | **`execd`** (existing spawner) |
| Windows / surfaces | WMS | BackBoard / WindowServer | **`windowd`** |
| Notifications | ANS | `usernotificationsd` | **`notifd`** (skeleton) |
| System-service registry (not apps) | samgr | bootstrap/launchd | **`samgrd`** |

The TASK-0065 draft called the lifecycle broker "`appmgrd` (new)". That name collides with two
things we already have: OpenHarmony's *AppMgr* is the **process** layer (our `execd`/appspawn), and
the *ability lifecycle* layer is **AMS = our `abilitymgr`**. Creating a new `appmgrd` would overlap
both → double structure. The decision (ADR-0036) is: **lifecycle broker lives in `abilitymgr`; the
registry stays in `bundlemgrd`; process spawn stays in `execd`.**

## Goals

- A queryable **app registry** (`bundlemgrd enumerate`) so the launcher/SystemUI learn the app set at
  runtime — never hardcoded.
- A cooperative **ability-lifecycle broker** (`abilitymgr`) with deterministic ordering + recents.
- **Chat and search become real app processes** that present surfaces; windowd hosts + chrome only.
- **Rate-limited notifications** (`notifd`) + SystemUI toast host.
- **SystemUI navigation** (Back/Home/Recents) + a launcher-click that launches a real app via the
  broker (not selftest-only state mutation).
- A `tools/nx/tests/chain_app_lifecycle.rs` integration chain that proves the **authority-handoff
  order** via hop markers.

## Non-Goals

- Kernel changes (cooperative lifecycle).
- Session/login (TASK-0065B).
- Multi-window-per-app, resize, min/max.
- Real thumbnail capture.
- Crash-loop/backoff policy (Ability v1.1).

## Constraints / invariants (hard requirements)

- **One authority per service** — registry (`bundlemgrd`), lifecycle (`abilitymgr`), process-spawn
  (`execd`), windows (`windowd`), notifications (`notifd`). No service wears two hats; no new
  `appmgrd`.
- **Launch preserves authorities** — SystemUI *requests* launch; `abilitymgr` *owns* lifecycle and is
  the **only** caller allowed to spawn apps (via execd); `windowd` *owns* window/focus state.
- **Deterministic lifecycle ordering** with bounded timeouts; no timing-dependent state.
- **No fake success** — a marker is emitted only when the behavior actually happened.
- **Notification quotas per app** are enforced and drops are counted deterministically.
- **No `unwrap`/`expect`** in production paths; no blanket `allow(dead_code)`; no kernel debug logs.

## Proposed design

### App registry — `AppRecord` (normative)

`bundlemgrd` gains an enumerate op. Each installed app/ability projects to:

```rust
struct AppRecord {
    id: BundleId,             // stable bundle/ability id, e.g. "org.nexus.chat"
    display_name: String,     // human label for launcher/recents
    launch_ability: String,   // ability entrypoint abilitymgr launches
    icon_ref: Option<IconRef>,// asset handle (registry-owned; renderer resolves later)
    required_caps: Vec<String>,// capabilities (already tracked by bundlemgrd)
}
```

- `enumerate()` returns `Vec<AppRecord>` for all installed bundles — the single source of "which
  apps exist". The launcher and SystemUI **query** this; they never embed an app list.
- Existing per-name `query` stays; `enumerate` is the list projection.

### Lifecycle state machine (normative)

```
        ┌─────────┐  launch   ┌────────┐  start  ┌────────────┐
        │ (none)  │ ────────▶ │ Create │ ──────▶ │ Foreground │
        └─────────┘           └────────┘         └─────┬──────┘
                                                  bg ▲ │ fg
                                                     │ ▼
                                              ┌────────────┐ suspend ┌─────────┐
                                              │ Background │ ──────▶ │ Suspend │
                                              └─────┬──────┘ resume  └────┬────┘
                                                stop│ ◀───────────────────┘
                                                    ▼
                                                ┌──────┐
                                                │ Stop │
                                                └──────┘
```

- Ordering is deterministic and host-testable: a mocked app receives `Create → Start → Foreground`,
  then `Background/Foreground` round-trips, then `Stop`.
- `abilitymgr` keeps a **recents** list (id + metadata; thumbnails stubbed).

### Launch handoff contract (normative)

```
SystemUI: launcher click (app_id)
  → abilitymgr.launch(app_id)
      → bundlemgrd.query(app_id)         # resolve AppRecord (must be installed)
      → execd.spawn(launch_ability)      # process-per-service; only abilitymgr may spawn apps
      → windowd: create_surface + bind   # app process presents its own surface
      → abilitymgr: fg(win)              # focus transition owned by windowd, requested by abilitymgr
```

Each arrow is a separate authority. The `tools/nx` chain encodes them as ordered hops so a regression
that, e.g., lets SystemUI spawn directly (skipping abilitymgr) fails the order check.

### Chat + Search as real apps (normative)

- `userspace/apps/chat` and `userspace/apps/search` become real app processes. They own their
  content/state and present surfaces to windowd via the existing surface protocol
  (`create_surface`/`queue_buffer`/present — already used by launcher + systemui).
- windowd **stops constructing** `self.chat`/`self.search` `ShellWindow` instances. It hosts the
  client surfaces and wraps them in WM chrome (`window_frame::Frame` + `ShellWindow` chrome: title
  bar / X / drag / z-order) — the chrome stays, the content leaves.
- Extraction order: **search first** (more self-contained), then **chat** (carries the VirtualList
  scroll/momentum, higher risk). Each step is a boot checkpoint.

### Notifications (normative)

- `notifd` enforces a **per-app quota** (rate limit) + priority; over-quota posts are dropped and
  **counted deterministically** (host-testable).
- SystemUI hosts a toast surface + a small tray/shade stub; a toast is shown on the shared proof
  surface, not marker-only.

## Security considerations

- **Threat model**: a misbehaving app that ignores lifecycle callbacks, or floods notifications.
- **Mitigations**: only `abilitymgr` may spawn apps (capability-gated via execd); notification quotas
  per app; launch/notify guarded by policyd (TASK-0047). Lifecycle is documented as **cooperative**
  (userspace policy) until stronger confinement exists — see YELLOW below.
- **Open risks (YELLOW — lifecycle authority)**: a non-cooperative app can stay foreground or ignore
  suspend. Acceptable for v6b; hardened confinement is Ability v1.1 (TASK-0234/0235).

## Failure model (normative)

- `launch` of a **non-installed** id → `Err` + marker, no spawn.
- `execd.spawn` failure → lifecycle stays in `Create`, surfaces never bound, marker emitted.
- Over-quota notification → dropped + counter incremented (no silent loss).
- App exits → `Stop` transition + window unbound; recents keeps the entry.
- No silent fallback: every failure path emits a marker or returns `Err`.

## Proof / validation strategy (required)

Authoritative commands + the full marker list live in **TASK-0065**. This RFC fixes the shape:

### Proof (Host)

- `tests/ui_v6b_host/`: lifecycle ordering, notification rate-limit drop counting, recents/focus
  selection.
- `cargo test -p bundlemgrd -- enumerate`, `cargo test -p abilitymgr`.

### Proof (integration chain — `tools/nx`)

- `tools/nx/tests/chain_app_lifecycle.rs` — `Contract` impls for bundlemgrd/abilitymgr/execd/windowd/
  notifd/systemui emitting **string-identical** markers to the real services, wired as ordered `Hop`s
  with `depends_on`, so the **authority-handoff order** is proven (registry → lifecycle → spawn →
  window → focus → toast), not just marker presence.

### Proof (OS/QEMU) — marker ladder (order-tolerant where noted)

```
bundlemgrd: ready
bundlemgrd: enumerate ok (n=...)
abilitymgr: ready
notifd: ready
systemui: nav ready
systemui: launcher click
abilitymgr: launch (app=..., pid=...)
abilitymgr: fg (win=...) / bg (win=...)
abilitymgr: live launch ok
systemui: toast (app=..., id=...)
notes: started / paused / resumed         # demo app
SELFTEST: ui v6 launch ok
SELFTEST: ui v6 lifecycle ok
SELFTEST: ui v6 toast ok
```

### Visual proof (required)

Launcher → app-window launch is visible on the shared proof surface; the demo app (`notes`) appears
as a real window; toast + nav changes are visible — not marker-only.

## Alternatives considered

- **New `appmgrd` service (per the draft)** — rejected. Overlaps `abilitymgr` (lifecycle) and `execd`
  (spawn) → double structure. OpenHarmony/Apple both keep lifecycle (AMS/FrontBoard) separate from
  process-spawn (appspawn/launchd) and from the registry (BMS/LaunchServices); we already have those
  three services. See ADR-0036.
- **Keep chat/search baked in windowd** — rejected. The whole point of v6b is real app processes;
  baked windows can't prove the launch/lifecycle/surface-host contract.
- **Registry inside abilitymgr** — rejected. "Which apps exist" (static, install-time) is a different
  authority from "what's running" (dynamic). bundlemgrd already owns install/query; enumerate belongs
  there.
- **Kernel-enforced lifecycle** — rejected for v6b. Cooperative userspace policy first; confinement is
  a tracked follow-up (Ability v1.1).

## Open questions

- **Recents thumbnails** — stubbed until a screencopy/screencap path exists (follow-up).
- **Notification persistence across reboot** — out of scope for v6b (configd-backed quota config is in;
  stored notifications are not).
- **Chat/search surface size negotiation** — windowd chrome owns the frame; does the app or the WM own
  the content size? Default: WM proposes, app accepts (resolve during Phase 3).

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0** (Registry enumerate): `bundlemgr enumerate`/`enumerate_apps` + `AppRecord` projection + `bundlemgrd` capnp `OPCODE_ENUMERATE` + host tests — proof: `cargo test -p bundlemgr -- enumerate` + `cargo test -p bundlemgrd -- enumerate` (green 2026-06-22). OS-lite enumerate deferred to P5/boot.
- [x] **Phase 1** (Lifecycle broker, core): `abilitymgr` promoted to a real service; lifecycle state machine + recents + wire dispatch + OS loop + `abilitymgr: ready/launch/fg/bg` markers — proof: `cargo test -p abilitymgr` (18 host tests green 2026-06-22) + riscv os-lite check. Live resolve/spawn/bind → P2 (WM mediation).
- [~] **Phase 2** (WM mediation + launch handoff): `abilitymgr/handoff.rs` orchestrator (resolve→spawn→bind→focus + rollback, host-tested) + real `.nxb` app bundles (`bundles/{chat,search,notes}`, `nxb-pack` + parse tests, `just pack-bundles`). Proof: `cargo test -p abilitymgr` (21) + `cargo test -p nxb-pack --test repo_bundles` (3), green 2026-06-22. Live execd/windowd/bundlemgrd clients → Phase 3/4.
- [~] **Phase 3** (Chat + Search real apps): per-app-surface foundation + first app crate done — `windowd::destroy_surface` + host-tested `app_surface::AppSurfaces` (own VMO, lazy load/free, own layer; ADR-0037) + `userspace/apps/search` (`search-app`: owns its word list + filter + renders its own surface buffer; 10 host tests). Proof: `cargo test -p windowd` (115) + `cargo test -p search-app` (10), riscv-checked. Boot-gated next: compositor composites the search client surface (remove the baked instance) + the live `SurfaceBinder`→windowd IPC; then chat.
- [ ] **Phase 4** (Notifications + Nav + notes): notifd quota + SystemUI nav + launcher-via-registry + demo `notes` — proof: `tests/ui_v6b_host`
- [ ] `tools/nx/tests/chain_app_lifecycle.rs` hop chain green (authority-order proven)
- [ ] Task TASK-0065 linked and its stop conditions cover all phases above.
- [ ] QEMU markers from §marker ladder appear in `scripts/qemu-test.sh` / postflight and pass.
- [ ] Anti-marker: `abilitymgr: launch` must NOT appear before `systemui: launcher click`.
