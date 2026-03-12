# Current Handoff: TASK-0015 dsoftbusd refactor v1

**Date**: 2026-03-12  
**Status**: `TASK-0015` is now `In Progress`; preparation and contract seeding are complete.  
**Contract seed**: `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`

---

## What is stable now

- `TASK-0014` is closed and archived at `.cursor/handoff/archive/TASK-0014-observability-v2-metrics-tracing.md`.
- Task tracking is aligned through `TASK-0014`; `TASK-0015` is now the next execution slice in:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
- `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` exists as the execution truth for the next slice.

## Current focus

- Refactor `source/services/dsoftbusd/src/main.rs` into a modular internal layout without changing:
  - wire formats,
  - existing marker names/semantics,
  - single-VM behavior,
  - cross-VM behavior.
- Keep the task intentionally scoped to preparatory structure work for:
  - `TASK-0016`
  - `TASK-0020`
  - `TASK-0021`
  - `TASK-0022`

## Relevant contracts

- Task SSOT:
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- Architecture:
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`
- Proof contracts:
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`

## First execution slice

- Read `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`.
- Inspect `source/services/dsoftbusd/src/main.rs` and confirm extraction seams:
  - entry/runtime wiring
  - netstack IPC adapter
  - discovery state
  - session FSM / handshake
  - local IPC / remote gateway
  - observability helpers
- Land the first non-behavioral split so `main.rs` becomes thinner without changing proofs.

## Guardrails

- No fake success markers.
- No protocol or ABI changes.
- No `netstackd` behavior changes.
- No shared-core extraction into `userspace/dsoftbus` in this task.
- Keep retry budgets / nonce-correlation / marker timing semantics intact.
- Run QEMU proofs sequentially only.
