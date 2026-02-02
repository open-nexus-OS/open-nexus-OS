# Next Task Preparation (Drift-Free)

<!--
CONTEXT
This file drives your "prep" ritual: validate the *next* task is drift-free
relative to current_state.md and the system vision/contracts before starting
a new chat/session.

It should be updated during the previous task's wrap-up, before handing off.
-->

## Candidate next task
- **task**: (fill) `tasks/TASK-XXXX-*.md`
- **handoff_target**: `.cursor/handoff/current.md` (always updated as the live entry-point)
- **handoff_archive**: (optional but recommended) `.cursor/handoff/archive/TASK-XXXX*.md` (snapshot after completion)
- **linked_contracts**:
  - (fill) `docs/rfcs/RFC-XXXX-*.md`
  - (fill) `docs/adr/XXXX-*.md`

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: (YES/NO) — if NO, specify conflict + fix location
- **best_system_solution**: (YES/NO) — if NO, propose alternative + where to record (ADR/RFC/task)
- **scope_clear**: (YES/NO) — acceptance criteria testable + non-goals explicit
- **touched_paths_allowlist_present**: (YES/NO) — task declares "Touched paths"

## Header / follow-up hygiene
- **follow_ups_in_task_header**: (YES/NO) — list missing follow-ups if any
- **security_considerations_complete**: (YES/NO) — list gaps + required `test_reject_*`

## Dependencies & blockers
- **blocked_by**:
  - (fill) e.g. missing RFC section, missing policy decision, missing test harness
- **prereqs_ready**:
  - (fill) e.g. "marker helper exists", "service API stabilized"

## Decision
- **status**: (GO / NEEDS_TASK_EDIT / NEEDS_RFC_OR_ADR / BLOCKED)
- **notes**:
  - (fill) 1-5 bullets
