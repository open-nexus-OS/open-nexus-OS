# ADR-0068: Modal semantics live in the DSL runtime on the app-owned `.overlay()`; windowd keeps ONE routing verb

- Status: Accepted
- Date: 2026-10-06
- Links:
  - Tasks: `tasks/TASK-0074-ui-v10b-app-shell-adoption-modals.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0067` (windowd = compositor service, no UI), `docs/rfcs/RFC-0070`
    (design-system SSOT), `docs/rfcs/RFC-0075` (IME text focus + delivery), `docs/rfcs/RFC-0086`
    (own-window gate for `CONTROL_WIN_*`)
  - Related ADRs: `docs/adr/0065-*` (emit-generation arena), `docs/adr/0034-*` (reactive cursor)

## Context

Overlays were already app-owned: `.overlay()` lifts a container out of flow and the app's page
decides what is in it (settings' PickerSheet/MoreMenu). What was missing was the MODAL contract —
a bounded stack, dismissal by ESC / backdrop / timeout, a focus trap, and an answer to "the app
has a second window" — plus the design handoff's Alert / Modal / System toast as elements apps
can use. Three forks had to be closed: widget crates vs. DSL compositions; where the modal state
lives (runtime vs. windowd vs. app store); how ESC reaches an app that has no text field (imed
drops every key without a focused field).

## Decision

- **The modal stack is a DERIVED view of the scene, owned by the DSL runtime.** Every
  `.overlay(modal|transient)` container the emit produces joins `View::overlays()` in emit order
  (the last modal is on top, `MODAL_DEPTH_MAX = 4`, the fifth is a runtime error). The runtime
  never hides an overlay: the ONE mutation path is the layer's own `on Dismiss` handler, which
  the runtime fires for ESC (`dismiss_top`), a backdrop tap (a tap inside the layer that no
  handler claims) and a transient's timeout (`dismiss_at`). Hit-testing, hover and text focus are
  confined to the topmost modal's subtree in the ONE hit-test (`interact::hit_scrolled`).
- **The kind rides the existing modifier (`.overlay(modal)` / `.overlay(transient)`,
  `.dismissAfter(ms)`); no IR schema change.** The compiler enforces the contract: NX0413 — a
  kinded overlay without `on Dismiss`, or `.dismissAfter` off a transient, is an error.
- **The host owns the clock.** A transient layer declares its lifetime; app-host merges it with
  the clock's one-shot (one timer per app-host) and fires the handler with reason `timeout`.
  There is no `svc.time.after` call: a blocking service call inside an effect would stall the
  app's loop for the toast's lifetime.
- **windowd keeps ONE verb and draws nothing.** `CONTROL_WIN_MODAL{sid, on}` (own-window gated)
  flags the sender's window app-modal; while set, windowd's press/hover/wheel routing refuses the
  owner's OTHER windows (`modal_gate`, pure, host-tested) and the press dies there. No modal
  state, no scrim, no toast in the compositor (RFC-0067).
- **ESC reaches a window without a text field through imed, not around it.** windowd relays
  window focus to imed as `FIELD_KIND_NONE`; imed then delivers Escape to that surface and
  nothing else (no commit, no composition, no strip, no learning, no OSK). One key path:
  inputd → imed → windowd → app-host. The former "Escape drops widget focus" idea is gone.
- **The elements are DSL components of the shared library (`window-kit`):** `WinAlert`,
  `WinModal`, `WinToast` — compositions of system primitives with a declared store contract
  (`AlertConfirm/AlertCancel/ModalClose/ToastDismiss/WinNoop`). No overlay widget crates. A
  consumer compiles in only the library components it references (transitive closure), so a kit
  can grow without every app declaring every kit event.
- **Out of scope:** system-modal (cross-app) blocking, the notification feed (notifd,
  TASK-0123..0125), ActionSheet/Popover/Menu/Tooltip/FAB elements, a second clock in the DSL.

## Consequences

- **Positive**: one hit-test decides reachability (no second "modal" path in app-host); a modal
  cannot leak input in-process or across the owner's windows; toasts cannot linger (the host
  fires the handler; the reducer decides); every visual element has one home (window-kit) and
  every app the same vocabulary; the compiler refuses an undismissable modal.
- **Negative / accepted**: `WinNoop`-style absorbers remain an app's duty (an alert that must
  not close on a backdrop tap absorbs its own taps); a nested modal costs a scene emit like any
  state change; the one-per-app-host timer means a toast and the clock share a deadline (the
  earlier wins, both run when due).
- **Churn**: `interact::hit_scrolled` gained a `confine` argument; `Mods.overlay` is an
  `Option<OverlayKind>`; app-host's event loop arms `timer_deadline_ns()` instead of the clock
  alone; imed's `plan()`/`key()` gained the no-field gate; `compile_project_bundle` pulls only
  referenced library components.

## Alternatives considered

- Modal/Alert/Toast as Rust widget crates — rejected (the repo voted app-owned `.nx` overlays;
  a crate cannot own the page's z-order).
- Modal state in windowd (a compositor-drawn scrim and dialog) — rejected (RFC-0067: windowd
  composites, it has no UI; the shell/app owns pixels).
- `svc.time.after(ms) → Bool` as a service call — rejected (blocks the effect for the lifetime;
  the declared `.dismissAfter` with the host's timer is the reactive form).
- A second ESC route inputd → windowd — rejected (two paths for one key; imed is the one key
  authority, so it learned a focus kind instead).
