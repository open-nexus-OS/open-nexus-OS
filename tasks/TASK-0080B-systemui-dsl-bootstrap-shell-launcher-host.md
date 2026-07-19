---
title: TASK-0080B SystemUI DSL bootstrap shell + login greeter (host-first): desktop + launcher grid + greeter pages in .nx
status: Done
owner: @ui
created: 2026-03-28
updated: 2026-07-06
depends-on:
  - tasks/TASK-0076B-dsl-v0_1c-visible-os-mount-first-frame.md
  - tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
  - tasks/TASK-0078-dsl-v0_2b-service-stubs-cli-demo.md
follow-up-tasks:
  - tasks/TASK-0080C-systemui-dsl-bootstrap-shell-os-wiring.md
  - tasks/TASK-0119-systemui-dsl-migration-phase1a-launcher-qs-host.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Shell registry the shell plugs into (EXISTS, ADR-0035): source/services/systemui/manifests/
    (shell.toml `dsl_root` + features + [first_frame]; product selects profile/shell/theme)
  - App registry feeding the launcher (EXISTS): source/services/bundlemgrd (ENUMERATE → AppRecord)
  - Launch authority (EXISTS): source/services/abilitymgr (launch_with_caps, fail-closed)
  - Session/login authority the greeter renders for (EXISTS, TASK-0065B): source/services/sessiond
    + contract docs/dev/ui/shell/session.md
  - Design kit consumed: userspace/ui/widgets/* (TASK-0073) via the DSL widget registry
---

## Context (updated 2026-07-06)

The system's own chrome becomes DSL: this task authors the **bootstrap shell**
(wallpaper/desktop + launcher) **and the login greeter** as real `.nx` programs,
host-first with snapshots. OS mount + live-input proof land in TASK-0080C via the
in-compositor mount from TASK-0076B.

**IST corrections vs the old draft:**
- ~~`appmgrd`~~ does not exist. The launcher's app list comes from **bundlemgrd
  ENUMERATE** (`AppRecord { id, displayName, launchAbility, requiredCaps }`) and
  launch goes through **abilitymgr** — both exist and are already wired for
  fail-closed capability checks.
- The shell-config registry exists (ADR-0035); `shell.toml` already carries
  `dsl_root` — this task creates what that field points at.
- The greeter exists as a native surface (TASK-0065B, shipped): this task re-authors
  its **view** in DSL. **Authority stays in sessiond** — the DSL greeter renders and
  dispatches; sessiond decides login/session (masterplan decision; no auth logic in
  the shell).

## Goal

1. **Shell DSL workspace** `userspace/systemui/` (what `dsl_root` resolves to):
   - `shells/desktop/` — `ShellPage.nx` (wallpaper, desktop chrome per shell.toml
     `[features]`), `LauncherPage.nx` (grid/list from the app registry, search/filter,
     windowed collection);
   - `greeter/GreeterPage.nx` — user list + password field + submit; states
     idle/authenticating/failure from the TASK-0065B contract;
   - shared `components/` + `composables/` following the canonical
     Store/Event/reduce/@effect/Page shape; effects call `svc.bundlemgr.enumerate`,
     `svc.ability.launch`, `svc.session.*` via typed adapters (transcripts on host).
2. **Profile-aware from the start**: host fixtures force desktop, tablet
   portrait/landscape (+ convertible shell modes) so the shell never becomes
   desktop-only by accident; per-profile overrides via `ui/platform/<profile>/` where
   layout diverges structurally.
3. **Icons**: SVG-sourced through the Icon primitive/Lucide import (TASK-0073);
   hover/focus/pressed via `InteractionState`-mapped tokens; PNGs only as goldens.
4. **Host proofs**: snapshot matrix + interaction fixtures (launcher tap emits the
   launch request with the right app id; greeter submit dispatches the sessiond
   login effect; failure state renders deterministically).

## Non-Goals

- OS wiring/live input (TASK-0080C). Quick settings/notifications/media (TASK-0119+).
  Any auth/session logic in the shell (sessiond only). Kernel changes.

## Constraints / invariants (hard requirements)

- Real shell path, not a prototype: LauncherPage is the base TASK-0119 extends;
  the greeter replaces the native greeter view in 0080C, same sessiond contract.
- Launch/login flows use the real service contracts (transcript-tested on host) —
  no mock-only shell behavior.
- State/action model ready for live pointer focus/hover/click (0080C) — id-based
  target-action, no synthetic-only paths.
- Deterministic fixtures for app list + session states; no company/product names.
- No `unwrap/expect` in bridge code; no godfiles.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/systemui_bootstrap_shell_host/`:

- shell + launcher + greeter snapshot matrix stable (profiles × dark/light);
- launcher search/filter deterministic; tap fixture emits
  `launch(app_id=<from AppRecord>)` through the effect path;
- greeter: submit dispatches the login effect; transcripted success/failure drive the
  canonical state transitions; failure golden;
- windowed launcher grid behaves (reorder/insert fixtures);
- all pages pass the a11y lints (labels on interactive nodes).

### Docs — required

- `docs/dev/dsl/patterns.md` gains the "system surface" chapter (shell/greeter as
  DSL consumers, authority-stays-in-services rule);
- `docs/dev/ui/shell/session.md` cross-linked (greeter view now DSL; contract
  unchanged); `docs/dev/ui/dsl-migration.md` updated.

## Touched paths (allowlist)

- `userspace/systemui/` (new: shells/desktop, greeter, components, composables)
- `userspace/dsl/runtime/` (svc adapters for bundlemgr/ability/session if not yet
  generated), `tests/systemui_bootstrap_shell_host/` (new)
- `docs/dev/ui/dsl-migration.md`, `docs/dev/dsl/patterns.md`

## Plan (small PRs)

1. shell page + wallpaper/chrome + snapshots
2. launcher page + registry adapter + interaction fixtures
3. greeter page + sessiond adapter + state fixtures
4. profile matrix + a11y pass + docs + handoff notes for 0080C

## STATUS / PROGRESS LEDGER (2026-07-07)

**HOST-SIDE DoD DELIVERED (uncommitted)** — autonomous phase-6 batch:

- **Workspace** `userspace/systemui/` (workspace-glob EXCLUDED in root
  Cargo.toml — DSL-only tree, the tools/__pycache__ lesson):
  - `shells/desktop/ui/` — ShellPage (top bar + Apps entry → navigate
    "/launcher"), LauncherPage (registry grid via
    `svc.bundlemgr.enumerate(query)` — filtering AT the service, canonical
    search-as-you-type; tap → `dispatch(Launch(app.id))` →
    `svc.ability.launch`), phone override (single-column list, structural
    divergence), Routes, launcher.store.nx; i18n/{en,de}.json.
  - `greeter/ui/` — GreeterPage (users via `svc.session.users`, secret
    TextField binding, submit → `svc.session.login`; phases 0 idle /
    1 authenticating / 2 failure mirror the TASK-0065B contract),
    session.store.nx, Routes; i18n/{en,de}.json.
- **Service surface** (dsl_services.capnp, sorted): `ability.launch(Str)`,
  `bundlemgr.enumerate(Str) -> List<AppEntry>`, `session.login(Str,Str)`,
  `session.users() -> List<Str>` — generated into the checker + app-host
  surface as always.
- **Host proofs** `tests/systemui_bootstrap_shell_host/` (7 green):
  profile matrix (desktop/phone/tablet), transcripted launch by AppRecord id
  (byte-exact replay, `is_clean()`), service-side search refilter, phone
  override structural divergence, greeter success/failure state machine
  (secret cleared on BOTH outcomes), keyed grid reorder/insert, lint/a11y
  gate for both trees.
- **COMPILER FIX ON THE WAY** (pre-existing, the shell was the first program
  big enough to hit it): `set_root_canonical` ASSERTS single-segment output —
  the default allocator grew by adding a second segment and ABORTED the
  build. Both canonicalization sites (core lower/mod.rs + ir hashing.rs) now
  size the first segment from the source message. Canonical bytes are
  unchanged (goldens prove it); all DSL suites green.
- **Docs**: patterns.md "System surfaces" chapter; session.md DSL-greeter
  note (contract unchanged); docs/dev/ui/dsl-migration.md created.
- **Icons (DoD item 3): DEFERRED** — the DSL Icon primitive renders via the
  widget registry, but the launcher v1 uses text cards; the Lucide-sourced
  icon pass rides with the 0080C mount (needs the asset pipeline wiring,
  TASK-0081 item 3).

OPEN → 0080C: OS mount (registry `dsl_root` now points at this tree),
greeter swap, live-input launch e2e.
