# Next Task Preparation (Drift-Free)

<!--
CONTEXT
Preparation file for the next execution slice.
Update during wrap-up so a new session can start without drift.
-->

## Candidate next task
- **task**: `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md` (`In Progress`)
- **handoff_target**: `.cursor/handoff/current.md`
- **handoff_archive**: `.cursor/handoff/archive/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- **linked_contracts**:
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Done`, legacy closure SSOT complete)
  - `docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md` (`In Progress`, TASK-0021 seed contract)
  - `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md` (`Done`)
  - `docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md` (`Done`, legacy `TASK-0001..0020` scope only)
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
- **first_action**: execute behavior-first phase-B host proofs under `TASK-0021` + `RFC-0035` (strict order, no `TASK-0022` absorption).

## Start slice (now)
- **slice_name**: TASK-0021 phase-B behavior-first host proof lock (post RFC-0035 seed)
- **target_file**: `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
- **must_cover**:
  - keep TASK-0019 closed as done baseline,
  - keep strict order (no preemption of later tasks),
  - preserve host-first execution and explicit OS gating for QUIC scaffolding,
  - preserve separation from `TASK-0022` core/no_std extraction scope,
  - reuse closure evidence discipline from `TASK-0020` (no fake success markers).

## Execution order
1. **TASK-0017**: remote statefs RW (Done)
2. **TASK-0018**: crashdumps v1 (Done, archived handoff)
3. **TASK-0019**: ABI syscall guardrails v2 (Done)
4. **TASK-0020**: streams v2 mux/flow-control/keepalive (Done)
5. **TASK-0021**: QUIC v1 host-first scaffold (next slice)

## Drift-free check (must be YES to proceed)
- **aligns_with_current_state**: YES
  - SSOT/handoff show TASK-0020 closed and next queue head is TASK-0021.
- **best_system_solution**: YES
  - strict numerical progression preserves drift-free execution.
- **scope_clear**: YES
  - scope is TASK-0021 only; no absorption from TASK-0022 or later tracks.
- **touched_paths_allowlist_present**: YES
  - TASK-0021 touched paths are explicit in task header.

## Header / follow-up hygiene
- **follow_ups_in_task_header**: YES
  - TASK-0022 boundary is explicit in TASK-0021 header.
- **security_considerations_complete**: YES
  - TASK-0021 security section exists and stays fail-closed.

## Dependencies & blockers
- **blocked_by**:
  - none for starting TASK-0021 in host-first mode.
- **prereqs_ready**: YES
  - `TASK-0005`, `TASK-0015`, `TASK-0016`, `TASK-0016B`, and `TASK-0017` are complete and referenced.
  - canonical harness contracts remain stable for staged host-first -> OS-gated execution.

## Decision
- **status**: ACTIVE (TASK-0021 started in strict order after TASK-0020 closeout)
- **notes**:
  - `TASK-0020` is `Done`, `RFC-0033` is `Done`, and `RFC-0034` legacy-scope closure is `Done`.
  - next execution starts with `TASK-0021` only, keeping strict numerical order.
