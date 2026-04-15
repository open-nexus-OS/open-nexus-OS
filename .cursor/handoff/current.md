# Current Handoff: TASK-0022 preparation kickoff

**Date**: 2026-04-14  
**Status**: `TASK-0021` handoff is archived; `TASK-0022` prep is now active.  
**Execution SSOT**: `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Transition performed
- Archived prior handoff snapshot:
  - `.cursor/handoff/archive/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- `TASK-0022` was refreshed to match current baseline:
  - RFC seed created: `docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md`,
  - header/follow-up task links,
  - production-gate alignment with `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`,
  - security invariants and negative-test requirements,
  - red-flag state re-evaluation.

## Baseline that must stay frozen during TASK-0022
- `TASK-0021` stays `Done` and must not regress:
  - host QUIC transport proof,
  - strict fail-closed transport selection semantics,
  - deterministic OS fallback markers.
- Mandatory regression guard during refactor:
  - `just test-dsoftbus-quic`
  - `just deny-check`

## Current task-0022 focus
- Extract reusable `no_std + alloc` DSoftBus core seams from the proven host path.
- Preserve existing auth/session invariants and sender-bound identity semantics.
- Unblock `TASK-0023` without pulling QUIC-enablement scope into this task.
