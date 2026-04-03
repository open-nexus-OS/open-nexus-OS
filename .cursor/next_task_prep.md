# Next Task Preparation (Drift-Free)

<!--
CONTEXT
Preparation file for the next execution slice.
Update during wrap-up so a new session can start without drift.
-->

## Candidate next task
- **task**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`In Progress`)
- **handoff_target**: `.cursor/handoff/current.md`
- **handoff_archive**: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- **linked_contracts**:
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (execution SSOT)
  - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md` (contract seed, In Progress)
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md` (follow-on boundary)
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md` (follow-on boundary)
  - `docs/testing/index.md`
  - `scripts/qemu-test.sh`
- **first_action**: request/execute plan-first kickoff for TASK-0020 phase 0 (contract + determinism lock) with host-first proof strategy.

## Start slice (now)
- **slice_name**: TASK-0020 phase-0 kickoff (contract-seeded, host-first)
- **target_file**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- **must_cover**:
  - keep TASK-0019 closed as done baseline,
  - lock deterministic limits/reject labels before feature growth,
  - preserve host-first execution and explicit OS gating,
  - enforce Rust/API hygiene requirements (`newtype`, ownership, `#[must_use]`, safe `Send`/`Sync` discipline),
  - keep TASK-0021/TASK-0022 scope boundaries explicit.

## Execution order
1. **TASK-0017**: remote statefs RW (Done)
2. **TASK-0018**: crashdumps v1 (Done, archived handoff)
3. **TASK-0019**: ABI syscall guardrails v2 (Done)
4. **TASK-0020**: streams v2 mux/flow-control/keepalive (current slice)
5. **TASK-0021+**: transport/core follow-ons (out of current slice)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - SSOT/handoff now point to TASK-0020 with RFC seed in place.
- **best_system_solution**: YES
  - host-first mux contract lock reduces drift before backend expansion.
- **scope_clear**: YES
  - scope is TASK-0020 only; no QUIC (`TASK-0021`) or core split (`TASK-0022`) absorption.
- **touched_paths_allowlist_present**: YES
  - TASK-0020 includes explicit allowlist for dsoftbus/mux/test/docs/harness paths.

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0021 and TASK-0022 boundaries are explicit in TASK-0020 header.
- **security_considerations_complete**: YES
  - threat model/invariants/DON'T DO and required `test_reject_*` set are explicit for mux state/credit abuse paths.

## Dependencies & blockers
- **blocked_by**:
  - OS mux proof closure remains gated while `userspace/dsoftbus/src/os.rs` backend stays placeholder.
- **prereqs_ready**: YES
  - `TASK-0005`, `TASK-0015`, `TASK-0016`, `TASK-0016B`, and `TASK-0017` are complete and referenced.
  - canonical harness contracts remain stable for staged host-first -> OS-gated execution.

## Decision
- **status**: IN PROGRESS (TASK-0020 phase 0 execution started)
- **notes**:
  - `RFC-0033` seed exists and `TASK-0020` is explicit SSOT for execution/proofs.
  - task header/phase model now reflects completed prerequisites and drift-reduced boundaries.
  - next checkpoint: phase 0 contract lock + host reject-test surface before implementation breadth.
