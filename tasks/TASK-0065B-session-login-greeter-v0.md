---
title: TASK-0065B Session v1: sessiond session authority + login greeter + SystemUI shell selection
status: Done
owner: @ui @platform @runtime
created: 2026-04-30
updated: 2026-07-02
depends-on:
  - tasks/TASK-0065-ui-v6b-app-lifecycle-notifications-navigation.md (Done — abilitymgr/bundlemgrd/execd split)
follow-up-tasks:
  - credential auth behind OP_LOGIN (keystored-backed) — the greeter click becomes "click → prompt → verify"
  - lock/unlock UI (SessionState::Locked + OP_LOCK are reserved seams)
  - session switching (docs/dev/ui/patterns/identity-and-trust/session-switching.md)
  - multi-user avatar grid on the greeter (registry already carries N users)
  - per-app session scoping via the app runtime (TASK-0080D)
links:
  - Shipped contract: docs/dev/ui/shell/session.md
  - Shell config model: docs/dev/ui/foundations/layout/profiles.md + docs/adr/0035-systemui-declarative-shell-configuration.md
  - Service split: docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md
  - Init manifest/boot stages: docs/rfcs/RFC-0069-* (§4 session-start stage)
  - Design track (auth/lock, still design-stage): docs/dev/ui/patterns/identity-and-trust/oobe-greeter-lock.md
---

## Context (rewritten 2026-07-02 — the 2026-04 draft was stale)

The original draft predated the ADR-0036 service split (it still said "appmgrd",
a retired name — the lifecycle broker is `abilitymgr`), the sessiond skeleton
(RFC-0069 Batch S), and the SystemUI library resolver (ADR-0035). This rewrite
records what was actually built: production-grade session management with
`sessiond` as the single session authority (the role a session/login manager
daemon plays on leading commercial OSes — own name, own contract), a real login
window, and session-driven shell selection through SystemUI.

## Goal (all delivered)

1. **sessiond = session authority**: host-tested state machine
   `Greeter → Active` (+`Locked` designed-in, reserved), manifest user registry
   (`manifests/users.toml`: id, display_name, product; optional `auto_login`),
   versioned wire protocol `nexus_abi::sessiond` (GET_STATE / LOGIN / LOCK
   reserved; golden-frame tests). Honest markers: `sessiond: greeter (n=…)` and
   `sessiond: session start (user=… product=…[ auto])` — auto-login runs the
   SAME `login()` transition.
2. **Login greeter in windowd**: full-screen blurred+dimmed wallpaper (one-time
   separable box blur baked into Plane 1 — no atlas, no per-frame cost), a
   centered round avatar (SDF circle + ring + Lucide `circle-user` + name),
   hover feedback (card-only redraw from a saved backdrop), click →
   `OP_LOGIN` → pristine base restored → session shell applied.
3. **SystemUI owns both UI decisions** (profiles.md contract): greeter
   appearance from `manifests/greeter/default/greeter.toml`
   (`systemui::greeter_config()`, bounded validation, hardcoded fallback);
   user→shell via the existing `resolve_product(product_id)` — sessiond stores
   only the opaque product string.
4. **Pre-session launch gating at the authority** (production choice — not just
   UI): `abilitymgr` refuses `OP_LAUNCH` with `STATUS_DENIED` unless sessiond
   reports an ACTIVE session. Injected `SessionGate` trait in `handoff.rs`
   (host-tested: rejected BEFORE resolve, nothing leaks/spawns); live os-lite
   gate = bounded sessiond GET_STATE per launch, fail-closed. windowd
   additionally suppresses ALL shell affordances while the greeter owns the
   display (`interaction::resolve_click_session`, host-tested).
5. **Never bricks**: windowd probes sessiond (250 ms cadence, ~6 s bound) only
   AFTER the framebuffer handoff; unreachable → `windowd: session unavailable
   (auto shell)` = today's default shell. Proven by an OS_SKIP=sessiond boot.

## Non-Goals

- Password/PAM/biometric/keyboard auth (docks behind OP_LOGIN later).
- Multi-user switching UI, lock-screen UI (states/ops reserved only).
- Kernel-enforced sessions.
- Per-app session scoping for spawned apps (TASK-0080D).

## Constraints / invariants (held)

- Session state is sessiond's alone; SystemUI/windowd render it, never forge it.
- `windowd` remains the input/hit-test/focus authority.
- No shell affordance reachable from the greeter (windowd gate) AND no launch
  without a session (abilitymgr gate) — defense at both layers.
- Boot-reveal timing unchanged (greeter renders over the already-present base).
- windowd heap: greeter buffers are one-time allocations; the per-hover redraw
  reuses a stored buffer (the service bump allocator never frees). windowd moved
  to `nexus-service-entry/heap-2m`.
- No unwrap/expect; manifests strictly validated with deterministic errors.

## Definition of Done — status

### Host proof (all green)
- `sessiond`: state machine (login ok / wrong-state / lock reserved) + users
  manifest (parse, duplicate id, missing field, unknown auto_login, greeter mode).
- `nexus-abi`: sessiond golden frames + malformed rejection.
- `systemui`: greeter manifest parses, fallback sane, out-of-bounds rejected.
- `windowd` `interaction`: greeter click hits only the avatar; sidebar/chat/
  hotspot unreachable while active; hover tracking; gate-off defers to shell.
- `abilitymgr` `handoff`: pre-session launch rejected before resolve; active
  session launches.

### OS/QEMU proof (headless, green)
- `init: windowd route->sessiond ok`, `init: abilitymgr route->sessiond ok`
- `sessiond: ready` → `sessiond: greeter (n=1)` → `windowd: greeter visible`
- Fallback lane (sessiond removed via OS_SKIP): boot reaches the shell,
  `windowd: session unavailable (auto shell)`, 0 fatals.
- Required proof ladder extended with `sessiond: ready` + `windowd: greeter
  visible`; the interactive injector (`tools/qmp_visible_input_inject.py`)
  logs in like a user (center click) before its choreography.

### Visual proof (user gtk boot, verified 2026-07-02)
- Boot → login window (blurred wallpaper, avatar circle, name) → hover →
  click → desktop shell.

## Touched paths

- `source/services/sessiond/` (state.rs, users.rs, manifests/users.toml, os_lite.rs)
- `source/libs/nexus-abi/src/lib.rs` (`pub mod sessiond`)
- `source/services/systemui/` (greeter.rs, manifests/greeter/)
- `source/services/windowd/` (session_client.rs, compositor/runtime/{session,greeter}.rs,
  interaction.rs, scene/input gates, build.rs avatar icon, Cargo heap-2m)
- `source/services/abilitymgr/` (handoff.rs SessionGate, os_lite.rs live gate)
- `source/init/nexus-init/` (endpoints/wiring/service_topology sessiond routes)
- `source/libs/nexus-service-entry/` (heap-2m tier)
- `policies/base.toml` (`session.query`), `scripts/qemu-test.sh`,
  `tools/qmp_visible_input_inject.py`, `docs/dev/ui/shell/session.md`
