# ADR-0053: settingsd is the single authority for all settings; windowd never writes them

- Status: Proposed
- Date: 2026-07-27
- Links:
  - Tasks: `tasks/TASK-0307-settings-distribution-v2.md` (execution + proof)
  - RFCs: `docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md`
  - Related ADRs: `docs/adr/0035-systemui-declarative-shell-configuration.md`

## Context

Settings state is split across two authorities today. `svc.settings.set` of
`ui.theme.mode`, `ui.theme.accent` and `ui.shell.mode` is routed to WINDOWD as a
surface-control frame — windowd applies live state and then persists back into
settingsd from the compositor thread — while every other key goes to settingsd
directly. `svc.settings.get` always reads settingsd. The same key therefore has
two write paths, two delivery semantics, two failure modes, and a GET that can
disagree with the live state. The measured consequence (boot
`manual--2026-07-27T09-02-33`) is the "changed the color and nothing works"
class: remount storms, input backpressure, persist timeouts, and silent drift
when a lossy push is dropped. This crosses the policy-authority boundary, so the
`architecture-boundaries` gate requires a recorded decision.

## Decision

- **settingsd owns ALL settings state** — live and persisted — including
  `ui.theme.mode`, `ui.theme.accent`, `ui.shell.mode`.
- **Writes flow one way**: every writer (apps, imed, windowd never) →
  settingsd. **Notifications flow the other way**: settingsd → subscribers.
  windowd is a subscriber and presentation-policy engine: it folds settings
  into a versioned presentation snapshot for surfaces and applies its own
  chrome, but it never originates or persists settings state.
- The windowd surface-control channel keeps **window controls only**
  (minimize/close/mode) — those are windowing, not settings.
- Out of scope for this decision: statefsd internals, per-key write ACLs
  (both recorded follow-ups in RFC-0083).

## Consequences

- **Positive**: one ordering, one convergence point, one delivery semantic;
  GET can no longer disagree with SET; the compositor thread stops doing
  synchronous settings persistence; theme writes now require a granted
  settingsd route instead of the windowd request slot every windowed app
  holds (a real, if incidental, security improvement).
- **Negative / accepted churn**: the control center (bundle_type `shell`)
  needs the SETTINGS permission ceiling widened to reach settingsd at all;
  wire ops 16/17/23 and three CONTROL values are retired reserved-forever;
  a migration window with dual authority (idempotent-guarded) is required.
- **Harder**: windowd must treat settings watch events as a first-class wake
  source (they arrive on its own server endpoint), since it no longer sees
  writes on its request channel.
