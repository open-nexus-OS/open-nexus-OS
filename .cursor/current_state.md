# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: close `TASK-0016B` (Done), archive handoff, and prepare `TASK-0017` kickoff.
- **rationale**:
  - execute queue order discipline (`TASK-0017` before later networking follow-ons),
  - reuse proven `dsoftbusd` modular seams instead of reopening monolith-style control flow,
  - preserve deterministic marker/audit behavior while adding remote statefs RW capability.
- **active_constraints**:
  - no fake-success markers (`ok/ready` only after real behavior),
  - deny-by-default ACL for remote statefs (`/state/shared/*` only),
  - authenticated identity source from kernel IPC/session identity only (no payload identity trust),
  - bounded input/request sizes and bounded retry loops,
  - no kernel changes in this slice,
  - OS build hygiene must stay green (`just dep-gate`, `just diag-os`),
  - QEMU proofs are sequential only (single-VM then 2-VM).

## Current focus (execution)
- **active_task**: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (Draft, kickoff prep complete)
- **seed_contract**:
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
- **contract_dependencies**:
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **phase_now**: planning/prep only (no feature implementation for TASK-0017 yet)
- **baseline_commit**: `bbfe0f5` (latest committed state before TASK-0017 implementation work)
- **next_task_slice**:
  - protocol/ACL/audit boundary definition for remote statefs RW,
  - host-first negative tests (`test_reject_*`) before QEMU proof tightening,
  - marker contract extension with deterministic evidence.

## Last completed
- `TASK-0016B` is `Done` and archived in handoff.
- CI/workflow lint issue from `make initial-setup` (`clippy::double_must_use`) is fixed and proven green.

## Proof baseline currently green
- `./scripts/fmt-clippy-deny.sh`
- `make initial-setup`
- `make build`
- `make test`
- `make run` (default timeout path now deterministic for local SMP run profile)

## Active invariants (must hold)
- **security**
  - no secrets in logs,
  - authorization fail-closed for remote writes/deletes,
  - remote writes/deletes must have deterministic audit evidence.
- **determinism**
  - stable marker strings and bounded waits,
  - no hidden retry/drain loops,
  - typed summary artifacts for `os2vm` runs when used.
- **build hygiene**
  - `--no-default-features --features os-lite` on OS services,
  - forbidden crates absent in OS graph (`parking_lot`, `parking_lot_core`, `getrandom`).

## Open threads / follow-ups
- Primary: implement `TASK-0017`.
- Explicit follow-ons (do not absorb into TASK-0017 scope):
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## DON'T DO (session-local)
- DON'T widen remote RW access beyond ACL scope.
- DON'T claim task progress without negative tests + deterministic proof evidence.
- DON'T drift marker contracts silently in `scripts/qemu-test.sh` / `tools/os2vm.sh`.
- DON'T pull `TASK-0020/0021/0022` transport redesign work into `TASK-0017`.
