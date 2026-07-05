---
title: TASK-0074 UI v10b (OS-gated): overlays wave + modal manager + toast unification + App Shell + SystemUI/app adoption + windowd convergence (W6) + markers
status: Draft
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

## Context (updated 2026-07-05)

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

## Goal

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

## Touched paths (allowlist)

- `userspace/ui/widgets/overlays/*`, `userspace/ui/widgets/window/*` (App Shell), `userspace/ui/shells/*`
- `source/services/windowd/src/compositor/runtime/*` (W6 collapse — net deletion), `windowd/src/scene_graph.rs`
- SystemUI plugins (adoption); `userspace/apps/{launcher,notes,settings}/` (adoption)
- `source/apps/selftest-client/` (markers); `tools/postflight-ui-v10.sh`
- `docs/dev/ui/patterns/app-shell.md`, `docs/dev/ui/foundations/quality/testing.md`, `docs/dev/ui/status/notifications.md`

## Plan (small PRs)

1. overlays wave primitives + host goldens.
2. modal manager + unified toasts (+ 5-surface routing hookup).
3. App Shell (`AppWindow`) + host snapshots.
4. W6 windowd convergence — one surface per PR, boot-verified then bespoke deleted.
5. SystemUI + app (launcher/notes/settings) adoption + markers.
6. OS selftests + docs + postflight.
