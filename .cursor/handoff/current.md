# Current Handoff: TASK-0020 streams v2 (in progress)

**Date**: 2026-03-27  
**Status**: `TASK-0019` remains archived/done; `TASK-0020` and `RFC-0033` are now `In Progress`.  
**Contract baseline**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`In Progress`, execution SSOT)

---

## What is stable now

- `TASK-0019` is closed and archived:
  - archive: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - task: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (`Done`)
  - rfc: `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md` (`Complete`)
- `TASK-0020` contract setup is aligned and drift-reduced:
  - task header/links updated to reflect completed prerequisites (`TASK-0015/0016/0016B/0017`),
  - `RFC-0033` created and moved to `In Progress` as the mux v2 seed contract,
  - task keeps execution/proof SSOT ownership per RFC process rules.
- Guardrails for this slice are explicit:
  - host-first execution while OS backend is gated,
  - bounded stream/window/credit semantics,
  - typed ownership + Rust API hygiene (`newtype`, `#[must_use]`, no unsafe `Send`/`Sync` shortcuts).

## Proof snapshot carried forward

- See archived handoff for the full TASK-0019 proof set and marker closure:
  - `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- No `TASK-0020` completion proofs are claimed yet (active in-progress implementation stage).

## Relevant contracts for next slice

- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md` (`In Progress`)
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- follow-on boundaries retained:
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Next actions

1. Execute phase-0 implementation for `TASK-0020` (contract + determinism lock) under host-first gates.
2. Keep implementation bounded to `TASK-0020` touched paths and explicit quality gates.
3. Advance to completion only after host + gated OS proof ladders are green and synchronized in task/RFC/docs.
