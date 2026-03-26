# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: close `TASK-0017` with real `statefsd` parity and fake-green-resistant marker gates.
- **rationale**:
  - keep remote statefs scope inside `dsoftbusd` gateway/session seams from `TASK-0015`,
  - enforce fail-closed ACL/auth/bounds behavior before marker/proof claims,
  - preserve deterministic single-VM + 2-VM proof discipline.
- **active_constraints**:
  - no fake-success markers (`ok/ready` only after real behavior),
  - deny-by-default ACL for remote statefs (`/state/shared/*` only),
  - authenticated identity source from kernel IPC/session identity only (no payload identity trust),
  - bounded input/request sizes and bounded retry loops,
  - no kernel changes in this slice,
  - OS build hygiene must stay green (`just dep-gate`, `just diag-os`),
  - QEMU proofs are sequential only (single-VM then 2-VM).

## Current focus (execution)
- **active_task**: `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (Complete)
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
- **phase_now**: TASK-0017 closeout complete; proofs and SSOT synchronized
- **baseline_commit**: `86a670b` (TASK-0017 prep reference from handoff)
- **next_task_slice**:
  - keep TASK-0017 closed and drift-free,
  - start only explicitly scoped follow-on work (`TASK-0020`/`TASK-0021`/`TASK-0022`) when requested.

## Last completed
- `TASK-0017` is fully closed:
  - remote statefs v1 gateway contract in `dsoftbusd` (`GET/PUT/DEL/LIST/SYNC` framing path),
  - remote path is wired to `statefsd` (no authoritative shadow-backend path),
  - deterministic v2 nonce-correlation is enforced in the gateway bridge for shared reply matching,
  - required negative tests added and green:
    - `test_reject_statefs_write_outside_acl`
    - `test_reject_statefs_prefix_escape`
    - `test_reject_oversize_statefs_write`
    - `test_reject_unauthenticated_statefs_request`
  - deterministic markers proven:
    - `dsoftbusd: remote statefs served`
    - `SELFTEST: remote statefs rw ok`

## Proof baseline currently green
- `cargo test -p dsoftbusd --tests -- --nocapture`
- `just dep-gate`
- `just diag-os`
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- `tools/os2vm.sh` summary artifacts:
  - `artifacts/os2vm/runs/os2vm_1774454076/summary.json`
  - `artifacts/os2vm/runs/os2vm_1774454076/summary.txt`

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
- TASK-0017 has no remaining stop-condition gaps.
- Explicit follow-ons (do not absorb into TASK-0017 scope):
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## DON'T DO (session-local)
- DON'T widen remote RW access beyond ACL scope.
- DON'T claim task progress without negative tests + deterministic proof evidence.
- DON'T drift marker contracts silently in `scripts/qemu-test.sh` / `tools/os2vm.sh`.
- DON'T pull `TASK-0020/0021/0022` transport redesign work into `TASK-0017`.
