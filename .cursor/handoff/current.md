# Current Handoff: TASK-0056 kickoff (v2a present scheduler + double-buffer + input routing)

**Date**: 2026-04-30  
**Completed task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `Done`  
**Completed contract**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `Done`  
**Active task (execution SSOT)**: `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md` — `In Progress`  
**Active contract seed**: `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md` — `In Progress`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Carry-in baseline (must stay true)

- `TASK-0055`/`RFC-0047` headless present authority is `Done`.
- `TASK-0055B`/`RFC-0048` visible scanout bootstrap is `Done`.
- `TASK-0055C`/`RFC-0049` visible first SystemUI frame is `Done`.
- 56 must extend the same `windowd` authority path; no sidecar present/input authority.

## TASK-0056 start posture

- RFC seed for 56 exists and is linked in both task and RFC index.
- 56 task header is synchronized (`depends-on`, `follow-up-tasks`, Gate E mapping, security invariants, red flags).
- Scope remains baseline-functional:
  - double-buffer surface present contract,
  - minimal scheduler/fence semantics,
  - deterministic input hit-test/focus/keyboard routing.
- Out-of-scope remains explicit:
  - visible cursor polish (`TASK-0056B`),
  - latency/perf tuning (`TASK-0056C`),
  - WM/compositor-v2 breadth (`TASK-0199`/`TASK-0200`),
  - kernel production-grade closure claims.
