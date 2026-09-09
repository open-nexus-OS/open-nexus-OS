---
title: TASK-0074 UI v10b: modal semantics in the DSL runtime (bounded overlay stack, dismissal contract, focus trap, one windowd routing verb, toast unification)
status: Draft (end-state rewrite 2026-09-09 — residual of the 2026-08-14 rebase: modal-manager semantics only)
owner: @ui
created: 2025-12-23
updated: 2026-07-05
depends-on:
  - TASK-0073 (design-system SSOT: token convergence + glass primitive + core/controls/inputs/nav/window primitives)
follow-up-tasks: []
links:
  - Architecture spine: docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md
  - Component inventory (IST + promote verdict): docs/dev/ui/components/inventory.md
  - Token reconciliation: docs/dev/ui/foundations/visual/token-reconciliation.md
  - Design contract (overlays + window templates + 5-surface notifications): docs/dev/design_handoff_open_nexus_os/
  - windowd boundary this task realizes (W6): docs/rfcs/RFC-0067
  - DSL emit target: tasks/TRACK-DSL-V1-DEVX.md
  - WM baseline: tasks/TASK-0064; Notifications baseline: tasks/TASK-0069; Search/Settings: TASK-0071/0072
  - Testing contract: scripts/qemu-test.sh
---

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09 (verified in code):** delivered elsewhere — App Shell = `userspace/
apps/window-kit` (`WinAppWindow.nx`, RFC-0084 slots, TASK-0308; size classes 640/1024 in
`app-host/src/probe/env.rs`); overlays are app-owned `.overlay()` by design (`userspace/dsl/core/
src/registry.rs:115`, rule :398-400; `settings/ui/components/chrome/{PickerSheet,MoreMenu}.nx`),
painted/hit-ordered by app-host (`probe/paint/collect.rs:76-123`); `Toast` widget exists
(`userspace/ui/widgets/toast`). Missing — no bounded overlay depth, no ESC/backdrop dismissal
contract (Escape only drops widget focus, `app-host/src/probe/interaction.rs:430`,
`dsl/runtime/src/focus.rs:136`), no focus trap, no windowd hook, no overlay/modal goldens, no
notifd→Toast routing (notifd is a placeholder; notifications are TASK-0123-0125 — out of scope),
`docs/dev/ui/components/inventory.md:97-103` still lists Modal/ActionSheet/Alert as widget
promotions "new — 0074" (stale).

### Goal (end system)

Modal semantics as a runtime contract on the existing app-owned `.overlay()` primitive: a
bounded overlay stack, ESC/backdrop dismissal as dispatched events, focus and hit-testing
confined to the topmost modal, one windowd routing verb for app-modal windows, toasts as
transient overlays. No new widget crates.

### Non-goals

Overlay widget crates (Modal/Popover/Menu/Tooltip/ActionSheet/Alert/FAB — dead by design);
notifications/notifd feed (0123-0125); system-modal (cross-app) blocking; windowd rendering of
anything.

### Invariants

- ONE mutation path: dismissal is `onDismiss` → reducer → state; the runtime never hides an
  overlay by itself.
- `MODAL_DEPTH_MAX = 4`; while a modal is open, hit-testing and text focus are confined to the
  topmost modal subtree; background input never leaks, in-process or across the owner's
  windows.
- Deterministic: same inputs ⇒ same stack transitions; goldens light/dark.

### Decisions

- **D1 Overlay kind.** `.overlay(modal|transient)` (registry `ModifierSpec` arg
  `ModArg::OptToken`), IR v1.3 `ViewNode.overlayKind` (append-only; shared bump with 0077B),
  runtime `userspace/dsl/runtime/src/overlay.rs::OverlayStack`.
- **D2 Lint NX0412 `ModalWithoutDismiss` (Error):** a `modal` overlay must declare
  `onDismiss`.
- **D3 ESC path replaced, not duplicated.** The "Escape drops widget focus" branch in
  `app-host/src/probe/interaction.rs:430` is DELETED; ESC → `OverlayStack::dismiss_top(Escape)`
  → `onDismiss`; focus clearing (`focus.rs`) happens as the consequence, one path. Backdrop tap
  = reason `Backdrop`. Gate: fixture "ESC with no modal open changes nothing; with a modal open
  dispatches `onDismiss(Escape)` and does not reach the widget below".
- **D4 windowd hook = one verb.** `OP_SURFACE_CONTROL` gains `CONTROL_WIN_MODAL{on}` (own-window
  gate): while set, windowd's input routing refuses presses on OTHER windows of the SAME owner
  sid (app-modal). No modal rendering, no toast drawing in windowd (boundary SSOT
  `docs/dev/ui/windowd-cleanup-map.md`).
- **D5 Toast unification.** `Toast` is a `transient` overlay; auto-dismiss via the existing
  `svc.time.after(Int) → Bool` route (slot 17) implemented by app-host's timed client — no
  timers in the DSL; the 5-surface notification routing waits for notifd (0123-0125) and is
  NOT stubbed here.
- **D6 Inventory drift fixed.** `docs/dev/ui/components/inventory.md:97-103` → overlays are
  app-owned `.nx`, verdict "converged (0074 semantics)". ADR "modal semantics live in the DSL
  runtime; windowd keeps one routing verb" (docs/adr, next free number).

### Packages

- **P0** ADR + IR v1.3 changelog (`docs/dev/dsl/ir.md`). Blast: paper.
- **P1** core + runtime (`overlay.rs`, NX0412, conformance fixtures: depth cap, trap,
  ESC/backdrop, nested). Blast: `dsl_conformance`, `dsl_goldens`, `dsl_apps_conformance`.
- **P2** app-host integration (ESC path replaced, hit-test confinement, `svc.time.after`).
  Blast: visible + smp1 lanes, `apphost:` markers, settings/stash pickers still work.
- **P3** windowd verb + `source/services/windowd/tests/modal_routing.rs`. Blast: input lanes
  (`input-live`), RFC-0086 consumers.
- **P4** goldens (modal/sheet/toast light+dark in `tests/ui_v10_goldens`) + inventory + docs.

### Definition of Done

Host: goldens; conformance — depth 5 rejected, background tap ignored, ESC dispatches
`onDismiss`, focus confined, NX0412 fixture. QEMU (registered in `proof-manifest/markers/
ui.toml` + `end.toml`, `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` via the windowd
contract): `apphost: modal open (depth=1)`, `apphost: modal dismiss (reason=escape)`,
`windowd: win modal on (id=…)`, `SELFTEST: ui v10 dialog ok`, `SELFTEST: ui v10 live modal ok`
(live pointer + keyboard open/dismiss on the visible surface, background-leak check against a
visible target). Docs: `docs/dev/ui/patterns/app-shell.md` modal section, `docs/dev/dsl/
syntax.md`, `inventory.md`.

### Touched paths

`userspace/dsl/core/src/{registry.rs,diag.rs,check/lints.rs,lower/views.rs}`,
`userspace/dsl/runtime/src/{overlay.rs (new),focus.rs,view.rs}`, `tools/nexus-idl/schemas/
ui_ir.capnp`, `source/services/app-host/src/probe/{interaction.rs,paint/collect.rs}`,
`source/services/windowd/src/compositor/runtime/input.rs`, `source/libs/nexus-display-proto/
src/control.rs` (approval zone), `docs/dev/ui/components/inventory.md`, `docs/adr/`.

### Dependencies

TASK-0077B (keyed state, IR v1.3). TASK-0324 P4a (windowd on the declarative arm) before P3.

## Rebase (2026-08-14) — heavily reduced residual — historical, superseded by the end-state rewrite above

### Delivered elsewhere — do NOT re-implement

1. **App Shell shipped as DSL, not widget crates**: `userspace/apps/window-kit/`
   (`bundle_type = "library"`) with `WinAppWindow.nx` (three-zone body,
   RFC-0084 slots) + `WinTopBar`/`WinMenuItem`/`WinSideItem`/`WinPropRow`/
   `WinActionItem`/`WinActionFace`. Responsive collapse happens at **640/1024
   via `device.sizeClass`** (`source/services/app-host/src/probe/env.rs:72-88`),
   not this ledger's 820/560 breakpoints. Owner: **TASK-0308** (In progress);
   consumers: TASK-0311/0312/0313. Do not rebuild an `AppWindow` scaffold here.
2. **W6 windowd convergence executed differently**: the per-surface
   migrate-then-delete list (chat → search → settings → desktop_layer →
   greeter) is dead. `docs/dev/ui/windowd-cleanup-map.md` DELETE column,
   "Status 2026-07-10: AUSGEFÜHRT" — chat/search/settings_window/greeter/
   desktop_layer/app_menu were deleted outright; chrome comes from the widget.
   The remaining windowd shrink is the map's **MOVE column — not this task**.
3. **Overlay primitives: the repo voted app-owned `.nx` overlays**, not widget
   crates. Evidence: the `.overlay()` modifier
   (`userspace/dsl/core/src/registry.rs:115`), the "app-owned overlay" rule
   (`registry.rs:398-400` — e.g. `Select`'s open panel is an app-owned
   `.overlay()`), and real examples
   `userspace/apps/settings/ui/components/chrome/{PickerSheet,MoreMenu}.nx`.
   No Modal/Popover/Menu/Tooltip/ActionSheet/Alert/FAB crates exist among the
   37 widgets — **by design, not as a gap**. The W4 "overlays wave" of widget
   primitives is dead scope.

**Doc drift noted (do not edit now):**
`docs/dev/ui/components/inventory.md:97-103` still claims those overlays are
"new — 0074" widget promotions. That is stale against the app-owned-overlay
decision — correct inventory.md during this task's build phase, not in this
rebase.

### Honest residual scope (Size M)

**Modal-manager SEMANTICS**, implemented in the widget/DSL layer plus one
minimal windowd hook:

- bounded modal stack depth,
- **focus trap via windowd focus routing** (the one windowd hook — routing
  only, no UI),
- ESC/backdrop dismissal contract,
- toast unification + routing (5-surface notification routing).

Boundary SSOT (`docs/dev/ui/windowd-cleanup-map.md:4-9`): windowd = Single
Present Authority (compositor SERVICE); widgets/chrome → `ui/widgets/*`;
shell UI → the DSL shell app. windowd gets only the focus-routing hook — no
modal rendering, no toast drawing, and never build into a MOVE/DELETE file.

### Corrected adoption targets

`userspace/apps/{launcher,notes}` are not valid targets: `notes` does not
exist, and `launcher` is a legacy Rust stub (launcher UI lives in
`userspace/apps/desktop-shell`). Real DSL apps today: calculator, chat,
desktop-shell, greeter, ime-ui, settings, stash.

### Corrected touched paths

`source/services/windowd/src/compositor/runtime/*` is removed from the
allowlist except the focus-routing hook; see the corrected allowlist below.
The STATUS ledger at the bottom (2026-07-06) is superseded by this section.

## Context (updated 2026-07-05) — historical, superseded by the end-state rewrite above

With the primitive SSOT in place (TASK-0073: W1–W3 + W5-nav/window), this task delivers the
**overlays wave (W4)**, the **modal manager**, **toast unification**, the **App Shell**, and the
**staged windowd convergence (W6)** — collapsing windowd's ~15k LOC of bespoke row-renderers
(`compositor/runtime/*`) onto the single reactive path `LayoutNode → LayoutEngine → SceneGraph →
nexus-gfx`, one surface at a time, each boot-verified identical.

OS-gated: it touches running services + QEMU markers and must prove the adopted shell stays
genuinely interactive through live QEMU input.

**User intent (2026-07-05):** production-grade Apple quality, no double structures (promote the
best impl, then delete the bespoke loser — this is where the triple structure finally becomes one),
`docs/dev/` kept at Human-Interface-Guidelines quality throughout.

## Goal — historical, superseded by the end-state rewrite above

> **Rebased 2026-08-14:** only item 2 (modal manager) survives as residual
> scope, in the widget/DSL layer + a minimal windowd focus-routing hook.
> Item 1 (W4 overlay primitives) is dead by design (app-owned `.nx`
> overlays), item 3 (App Shell) is owned by TASK-0308 (window-kit), items
> 4-5 (W6 convergence, per-surface migration/adoption list) were executed
> differently or target apps that don't exist — see the Rebase section.

1. **W4 — overlays wave (primitives):** Modal, ActionSheet, Alert, Popover/PopoverItem, Menu/
   ContextMenu, Tooltip, FAB — full handoff contract, on the reactive path, on the dense overlay
   material (D4 glass primitive, overlay level + scrim).
2. **Modal manager:**
   - userspace-only modal stack (Dialog/Sheet) with backdrop, focus trap, ESC handling, bounded depth,
   - unified toasts via the kit `Toast` (feeds the 5-surface notification routing),
   - focus traps use `windowd` focus/input routing — no leaked events to background surfaces,
   - live pointer inside/outside + keyboard escape/focus behavior visible in QEMU on the shared surface.
3. **App Shell:** `AppWindow` scaffold (title bar/toolbar/content/sidebar/properties slots,
   responsive collapse ≥820/≥560/<560), hooks into WM title/icon state, delegates global shortcuts
   to SystemUI. Composed from TASK-0073 window/nav primitives — not a new structure.
4. **W6 — windowd convergence (the double-structure kill):** migrate `compositor/runtime/*`
   surface-by-surface (chat → search → settings → desktop_layer → greeter) onto the promoted
   declarative components + scene graph; **delete the bespoke renderer** once each surface is
   boot-verified identical. Realizes the RFC-0067 windowd-slimming.
5. **Adoption/migration:** SystemUI overlays (quick settings, notifications, palette, settings
   overlay) + apps (`launcher`, `notes`, `settings`) adopt the App Shell + kit primitives.
6. **Markers + OS selftests + postflight.**

## Non-Goals

- Kernel changes.
- Perfect "final UI" — v1 design-system adoption with stable visuals/behavior.
- New primitives beyond the handoff contract (blessed native surfaces are the DSL track's remit).

## Constraints / invariants (hard requirements)

- **Promote the best, then delete the loser** (RFC-0070 D5): each W6 surface migration ends with the
  bespoke renderer removed — no lingering parallel path. Boot-verify identical before deletion.
- Migration must not break existing markers; new markers are additive + deterministic.
- Modal manager bounded (cap stack depth); focus traps route via `windowd`; no background input leak.
- One reactive path (D1) — no new bespoke renderers introduced during adoption.
- No `unwrap/expect`; no blanket `allow(dead_code)`; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Goldens for the overlays wave + App Shell chrome in light/dark (may live in `ui_v10_goldens`).
- Modal-manager unit proofs: bounded depth, focus-trap containment, ESC/backdrop dismissal.

### Proof (OS/QEMU) — gated (order tolerant)

- `design: kit adopted (systemui)`
- `design: kit adopted (launcher)`
- `design: kit adopted (notes)`
- `windowd: surface converged (chat|search|settings|desktop|greeter)` — one per collapsed surface (W6)
- `SELFTEST: ui v10 button ok`
- `SELFTEST: ui v10 dialog ok`
- `SELFTEST: ui v10 live modal ok`
- `SELFTEST: ui v10 theme recolor ok`

### Visual proof — required

- shared proof surface shows an adopted app shell + a modal/sheet target;
- live pointer/keyboard visibly open and dismiss the modal on that same screen;
- background-input-leak checks performed against visible targets, not only event logs;
- each W6-converged surface looks identical before/after the bespoke renderer is deleted.

### Docs — required (HIG-grade)

- `docs/dev/ui/patterns/app-shell.md` + overlay/modal pattern docs current;
- `docs/dev/ui/status/notifications.md` reflects the 5-surface routing wired to `Toast`;
- inventory verdicts flipped to "converged" as each surface lands.

## Touched paths (allowlist) — corrected 2026-08-14

- `userspace/ui/widgets/*` (modal/toast semantics where widget-shaped)
- app-owned overlay `.nx` surfaces in `userspace/apps/*` (modal/toast
  adoption; coordinate `window-kit` changes with TASK-0308, its owner)
- `source/services/windowd/` — ONLY the focus-routing hook for focus traps
  (check the cleanup map first; no rendering, no overlays)
- `source/apps/selftest-client/` (markers)
- `docs/dev/ui/patterns/app-shell.md`, `docs/dev/ui/foundations/quality/testing.md`,
  `docs/dev/ui/status/notifications.md`,
  `docs/dev/ui/components/inventory.md` (fix the "new — 0074" drift during build)

## Plan (small PRs)

1. overlays wave primitives + host goldens.
2. modal manager + unified toasts (+ 5-surface routing hookup).
3. App Shell (`AppWindow`) + host snapshots.
4. W6 windowd convergence — one surface per PR, boot-verified then bespoke deleted.
5. SystemUI + app (launcher/notes/settings) adoption + markers.
6. OS selftests + docs + postflight.

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06) — historical, superseded by the end-state rewrite above

> **SUPERSEDED by the "Rebase (2026-08-14)" section above** — kept for
> history only. The W4 overlays wave, the App Shell build, and the W6
> per-surface convergence recorded below are dead scope (delivered elsewhere
> or executed differently); only the modal-manager semantics remain.

> Durable done/open record. **Nothing in this task has started yet** — it is unblocked now that
> TASK-0073's primitive kit + token SSOT + Icon system + goldens/a11y harness are in place (host-safe,
> mostly committed). Overlay primitives are host-safe; the modal manager, App Shell adoption, and the
> whole W6 convergence + palette/bake/glass work are **[BOOT-GATED]** (touch running services + QEMU
> markers → need a user boot-verify per phase).

### Ready to build on (from TASK-0073, DONE)
- The kit crates `userspace/ui/widgets/*` (32 components), `Icon`/`Icon::lucide`, `Text`, `InteractionState`.
- Token SSOT (`resolve_material`/glass materials incl. `MaterialToken::Overlay` + `scrim` tokens for overlay surfaces), `ShapeKind::Vector` (icons).
- Golden + a11y harness `tests/ui_v10_goldens/` (extend with overlay/app-shell goldens; painter is shape-aware).

### ⬜ OPEN — W4 overlays wave (host-safe, this task) — 0 of 9
- **Modal**, **ActionSheet**, **Alert**, Popover/PopoverItem, Menu/ContextMenu, Tooltip, FAB.
- Build on the dense **overlay** glass material (`MaterialToken::Overlay`) + `scrim` token; each a pure `LayoutNode` builder like the kit; add host goldens.

### ⬜ OPEN — Modal manager (host-safe logic + [BOOT-GATED] input routing)
- Userspace modal stack (bounded depth), backdrop, focus trap, ESC. Focus traps must route via
  `windowd` focus/input (no background leak) — **[BOOT-GATED]** live QEMU pointer/keyboard proof.
- Unified `ToastView` + the **5-surface notification routing** (Activity Runner / Mitteilungen /
  Control Center / System-Toast / Background Jobs) — behavioural (see design_handoff README).

### ⬜ OPEN — App Shell — 0 of 2 window-compose
- **AppWindow** (sidebar·content·properties, responsive collapse ≥820/≥560/<560), **WindowActionBar**.
  Compose from TASK-0073 `Window`/`Sidebar`/`WindowPane`/`WindowControls`. Host snapshots first.

### ⬜ OPEN — [BOOT-GATED] the live-path bundle (needs user boot-verify)
1. **Palette shift**: retune core neutrals (`surface/fg/bg/accent`) in `.nxtheme.toml` to the handoff
   pure-grey palette (updates value-pinning tests + changes windowd's baked look).
2. **windowd `theme.rs`/`assets::THEME_*`** → point at the shared generation (one bake path).
3. **Glass primitive D4**: extend `nexus-gfx` `LayerBackdrop` with tint/shine/border from material
   tokens; route windowd's baked-tint path through it (the TASK-0073 "one glass draw" golden).
4. **windowd renders `ShapeKind`** (Path/Vector/Triangle) so kit icons/chevrons appear in the live OS.
5. **W6 convergence**: collapse `windowd/src/compositor/runtime/*` (~15k LOC bespoke row-renderers)
   onto `LayoutNode → LayoutEngine → SceneGraph → nexus-gfx`, one surface per PR
   (chat→search→settings→desktop_layer→greeter), each boot-verified identical, then delete the
   bespoke renderer. This is the RFC-0067 windowd-slimming and the "one path" endgame.

### ⬜ OPEN — Adoption + proof (per this task's DoD)
- SystemUI overlays + apps (launcher/notes/settings) adopt the kit; markers `design: kit adopted (...)`,
  `SELFTEST: ui v10 ...`, `windowd: surface converged (...)`; postflight.

### Notes for whoever continues
- Overlay primitives + App Shell + modal-manager LOGIC are host-safe (build like TASK-0073, add
  goldens). Everything that touches windowd rendering or running services is boot-gated → stage per
  phase with a user boot-verify. Architecture SSOT = **RFC-0070** (one declarative path, promote the
  best impl not the incumbent). windowd convergence detail = **RFC-0067**.
