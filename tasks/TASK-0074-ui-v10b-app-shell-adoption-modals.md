---
title: TASK-0074 UI v10b: modal semantics in the DSL runtime (bounded overlay stack, dismissal contract, focus trap, one windowd routing verb, toast unification)
status: Done (2026-10-06 — P0–P4 in one go as Block 3's first task; proof chain + board smoke in "Delivered" below; end-state rewrite 2026-09-09)
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

## Delivered 2026-10-06 (what was built, where the decisions above were corrected by measurement)

- **D1 — no IR bump.** `.overlay(modal|transient)` rides modId 48's existing token arg
  (`ModArg::OptToken`, vocabulary `modal|transient`), `.dismissAfter(ms)` = modId 56 (append-only);
  `ViewNode.overlayKind` was unnecessary. Runtime: `userspace/dsl/runtime/src/overlay.rs`
  (`OverlayStack`, `MODAL_DEPTH_MAX = 4`, `DismissReason`), built by every emit, box ids resolved
  like handlers; confinement = the `confine` argument of the ONE hit-test
  (`interact::hit_scrolled`) — press, hover, focus and the backdrop rule cannot disagree.
- **D2 — lint is NX0413 `OverlayDismiss`** (NX0412 was already `RetiredTimeout`): a kinded overlay
  without `on Dismiss`, or `.dismissAfter` off a transient. New trigger `Dismiss`.
- **D3 — ESC route.** The "Escape drops widget focus" branch did not exist at HEAD (only a doc
  comment); the real gap was that imed drops every key without a focused FIELD. Built: windowd
  relays WINDOW focus to imed as `FIELD_KIND_NONE` (`sync_surface_focus_to_imed`, after input /
  text-focus / close), imed delivers Escape to that surface and nothing else (`test_reject_*`).
  app-host: `ACTION_ESCAPE` → `View::dismiss_top(Escape)`; no modal → nothing.
- **D4 — as decided.** `CONTROL_WIN_MODAL = 8` (`value = sid << 4 | on`), own-window gated,
  `AppWindowSlot.app_modal` (reset on close + fresh launch), `modal_gate.rs` (pure verdict + the
  `ModalProof` edge counter), applied in the press, hover and wheel loops; a refused press dies
  (`windowd: press refused (modal id=…)`). Markers `windowd: win modal on/off (id=…)`,
  `SELFTEST: ui v10 dialog ok` (first on→off round trip), `SELFTEST: ui v10 live modal ok`
  (presses routed to the modal window while on, live route up).
- **D5 — `svc.time.after` does not exist and would block the app's loop for the toast's
  lifetime.** Built instead: the declared `.dismissAfter(ms)`; app-host merges it with the clock's
  one-shot (`timer_deadline_ns`, `timer_fired`, `probe/overlay.rs`) and fires `dismiss_at(Timeout)`.
- **D6 — inventory fixed; ADR-0068.** The elements live in the shared DSL library `window-kit`:
  `WinAlert` (absorbs backdrop, ESC/cancel → `AlertCancel(id)`, confirm → `AlertConfirm(id)`),
  `WinModal` (slots body/footer, backdrop + ✕ + ESC → `ModalClose(id)`), `WinToast` (left edge,
  `.dismissAfter(4000)`, tap or timeout → `ToastDismiss(id)`). **The compiler now pulls only the
  library components an app references** (transitive closure, `project.rs`): compiling the whole
  shelf into every consumer would have forced settings to declare the alert events and the shell
  to declare every window event — the kit could never have grown.
- **Shell demo (operator rule: every visual task ships a shell entry + ack rung):** Control Center
  power button → `PowerAsk` → `WinAlert` ("Ausschalten?") → ESC/Cancel close; Confirm → `WinToast`
  "Noch kein Energiedienst angebunden." (4 s). `ShellOverlays.nx` (plain full-bleed layer mounted
  last on both `ShellPage`s), `power.store.nx`, i18n ×5. Board rung `board-visual: modal`.
- **Proof:** host — core/runtime unit tests, `dsl_apps_conformance` (`overlays.rs`: depth 5 refused,
  background tap → backdrop dismiss, focus/hover confined, absorbing layer leaks nothing, nested
  ESC innermost-first, transient timeout; `shell_power_alert.rs`: the shell flow + the injector
  targets' SSOT), `dsl_goldens` `kit_{alert,modal,toast}_{light,dark}`, windowd
  `tests/modal_routing.rs`, imed `test_reject_*`, nx `contract_covers_markers`; QEMU —
  `usb-visible` lane requires the modal rungs (injector: pill → power → background press → ESC);
  board — `[PASS] board-visible` with the `modal` ack. See the proof table at the end.

- **ONE project loader.** `nexus_dsl_core::load_project` (ui/**.nx + referenced library
  components + platform merge) now feeds the build, the CLI's directory mode (`nx-dsl
  build/run <app>`) and the `systemui_bootstrap_shell_host` tests, which had their own walker
  (no libraries → `WinAlert` unknown) and asserted i18n KEY names; they assert the baked
  default texts now.

### Live proof on the `usb-visible` lane (2026-10-06, green after seven iterations)

`apphost: modal open (depth=1)` → `windowd: win modal on (id=2)` → background press absorbed
(`apphost: tap (400,400) hit=…` on the alert's layer) → ESC → `apphost: modal dismiss
(reason=escape)` → `windowd: win modal off (id=2)` → `SELFTEST: ui v10 dialog ok` + `… live modal
ok`; chain-marker contract 20/20 incl. the new `ui-modal` group. What the lane taught (fixed at
the root, no workarounds):

- **windowd dropped imed's push without a text focus** (`handle_imed_push` routed only through
  `text_focus`): now an ACTION push to the window-focus holder (`imed_surface_focus`) is routed;
  commits/preedits without a field stay dropped. **The desktop surface takes the modal verb**
  (`desktop_modal`, `taskbar_gate` = its own-window law) — it gates no sibling, it records the edge.
- **Relative-mouse positioning is not a sum of steps on the one-hart TCG guest**: QEMU's USB
  boot mouse merges queued reports with int8 clamping while the guest polls slowly, and inputd's
  acceleration caps a delta at 256. The injector now homes into the corner (the clamp makes
  (0,0) exact), travels in ≤40-px steps, and corrects from app-host's own tap trace (`apphost:
  tap (x,y) …`, now also for quiet taps); a press that never surfaced is pressed again.
- **The harness's greeter login was dead** (`windowd: greeter visible` no longer exists): the
  injector logs in from the greeter's handler-box dump (its Submit circle), waits for `windowd:
  session shell visible`. The session shell is the DESKTOP product (`product=default`) — the
  targets' SSOT is `shell_power_alert.rs` with `FixtureEnv::desktop()`.
- **The run ended before the choreography**: the launcher's early stop now also waits for the
  marker a lane names in `QEMU_LADDER_ALSO_WAIT` (harness.toml: the live-modal rung) inside a
  320 s grace; `ci-os-usb-visible` RUN_TIMEOUT 240 → 360 s.

### Board cycles 2026-10-06 (pre-commit smoke, operator rule)

| Cycle | Result |
|---|---|
| 1 (SD-boot without the user's wait) | alert opens, ESC closes (operator); log not pulled |
| 2 | alert → Cancel; alert → Confirm → toast → `dismiss (reason=timeout)`; `dialog ok` + `live modal ok`; no keyboard rung (no key typed) |
| 3 | alert → Confirm → toast → timeout → second alert → **USER-PF in app-host** (`AnimationDriver::tick_emit`, timeline.rs:71, from `anim_tick`) → shell dead, input chain backed up; cursor sprite breaks up over the Control Center (F7 mechanism: sprite re-upload on shape change during scanout) |
| 4 (identical sequence) | **all functional rungs green**: open/on → ESC `dismiss (reason=escape)` → off → `dialog ok`; open → Confirm → toast → `timeout`; `live modal ok`; `live keyboard route on`; acks desktop/typed/pointer/modal. Ladder verdict red ONLY on `KSELFTEST: ipc call budget FAIL (rt=85us budget=64)` |

The cycle-3 fault did not reproduce in cycle 4 nor on QEMU with the identical choreography
(`usb-visible`, confirm → toast → timeout → alert → background → ESC, green); the Vec header the
driver iterates was corrupt, the allocator's arena handling is correct (deallocs of arena blocks
are ignored), no fixed-size array neighbours it. **Decision (user, 2026-10-06): leave it until
the GPU optimisation lane; recorded here, not allow-listed.**

The IPC call budget on the board was 33–55 µs in every Block-2 cycle and 85–98 µs in all three
logged cycles today, also when the benchmark ran BEFORE any input (cycle 4); QEMU shows 42 µs
before and after this task — not attributable to this change from the evidence; goes into the
M/G/S measurement package. Not allow-listed.

### Open findings (recorded, not built)

- `KSELFTEST: ipc call budget` measures IPC round trips while live input may already be
  flowing (board cycle 2026-10-06: 98 µs vs 64 µs with the mouse moving during the benchmark;
  QEMU: 4972 µs with the injector's homing burst) — the benchmark does not serialize against
  input, so its verdict depends on the operator's hands. Not allow-listed; the injector now
  starts after the ladder, the board operator waits ~90 s before touching the input.
- **Board: one app-host fault in `AnimationDriver::tick_emit`** after toast → alert (cycle 3 of
  4). Reproduced on the `usb-visible` lane with the identical registers (2026-10-06 evening,
  while proving TASK-0066) and **fixed at the root**: `relayout_retained` reconciled the
  animation driver INSIDE its layout generation, so the driver's Vec pushes landed in arena
  memory that the layout two frames later reset under the running fade (ADR-0065 amendment;
  `anim_sync` now runs after the generation closes). Latent since the arena landed; the toast →
  alert sequence was the first to span two layouts with a live keyframe.
- **Board: `KSELFTEST: ipc call budget` 85–98 µs (budget 64) in every cycle today**, 33–55 µs in
  Block 2, QEMU unchanged at 42 µs — M/G/S measurement package.
- **F7 detail**: the pointer sprite tears over hover fields — the dc cursor sprite is re-uploaded
  in place on a shape change while the controller scans it; fix = double-buffered sprite swapped
  at cfg-ready (gpud dc, GPU lane).
- QEMU HID report merging under slow guest polling (one hart + TCG) loses pointer distance
  for ANY fast injector; the harness's own first-move/close steps land exactly only when the
  guest keeps up (seen both ways across runs) — a lane-level nondeterminism the proof markers
  tolerate; the modal phase is closed-loop and immune.

- `emitProp` handlers (`emit($props.onX)`) are still unwired in the runtime (`emit.rs` "wired with
  the instance/params work") — the kit therefore uses the store-event contract (`AlertCancel(id)`
  …), like `WinAppWindow`. A follow-up when component callbacks land.
- `ActionSheet` (bottom, grouped) is not written as a kit element; the contract covers it
  (`.overlay(modal)` composition) — write it with its first consumer.
- The toast is the SURFACE only; the 5-surface routing / notifd feed is TASK-0123..0125.
- windowd relays window focus to imed on input/text-focus/close; a focus change by a
  programmatic raise with no input frame after it is picked up at the next input frame.

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

- **P0** ADR + IR changelog (`docs/dev/dsl/ir.md`; **the current IR version — TASK-0077B took it to v1.7, "v1.3" above is stale**, append-only). Blast: paper.
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
