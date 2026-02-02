# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the *current* system state.
It is intentionally compact and overwritten after each completed task.

Rules:
- Prefer structured bullets over prose.
- Include "why" (decision rationale), not implementation narration.
- Reference tasks/RFCs/ADRs with relative paths.
-->

## Current architecture state
- **last_decision**: (fill) `docs/adr/XXXX-*.md` or `docs/rfcs/RFC-XXXX-*.md`
- **rationale**: (fill) 1-3 bullets explaining *why* the decision was taken
- **active_constraints**:
  - (fill) e.g. "No fake success markers", "OS-lite feature gating", "W^X for MMIO"

## Active invariants (must hold)
- **security**
  - Secrets never logged
  - Identity from kernel IPC (`sender_service_id`), never payload strings
  - Bounded input sizes; validate before parse; no `unwrap/expect` on untrusted data
  - Policy enforcement via `policyd` (deny-by-default)
  - MMIO mappings are USER|RW and NEVER executable (W^X)
- **determinism**
  - Marker strings stable and non-random
  - Tests bounded (no infinite/unbounded waits)
- **build hygiene**
  - OS services use `--no-default-features --features os-lite`
  - Forbidden crates: `parking_lot`, `parking_lot_core`, `getrandom`

## Open threads / follow-ups
- (fill) `tasks/TASK-XXXX-*.md` — short description

## Known risks / hazards
- (fill) "Areas that are fragile" with pointers to code/docs

## DON'T DO (session-local)
- (fill) Temporary prohibitions to prevent drift/regressions
