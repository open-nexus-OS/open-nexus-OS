# Current Handoff: TASK-0020 streams v2 kickoff prep

**Date**: 2026-03-27  
**Status**: `TASK-0019` is archived and closed as done; current focus moves to sequential kickoff for `TASK-0020`.  
**Contract baseline**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Draft`)

---

## What is stable now

- `TASK-0019` is closed and archived:
  - archive: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - task: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (`Done`)
  - rfc: `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md` (`Complete`)
- ABI guardrail boundaries remain fixed:
  - kernel untouched,
  - deterministic bounded matching,
  - authenticated profile source + subject binding,
  - static startup lifecycle only.

## Proof snapshot carried forward

- See archived handoff for the full TASK-0019 proof set and marker closure:
  - `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`

## Relevant contracts for next slice

- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- ABI follow-on boundaries retained:
  - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
  - `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`

## Next actions

1. Keep TASK-0019 artifacts stable as completed baseline (no scope reopen).
2. Start TASK-0020 plan/implementation in strict sequential order.
3. Preserve ABI lifecycle/kernel enforcement follow-ons in TASK-0028/TASK-0188 only.
