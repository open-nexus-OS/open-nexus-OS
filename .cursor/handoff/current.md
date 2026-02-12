# Current Handoff: TASK-0013 QoS ABI + timed coalescing — CLOSED

**Date**: 2026-02-11  
**Status**: TASK-0013 implementation and proof closure complete. QoS authority/audit deltas are closed and full proof ladder reruns are green.

---

## Baseline fixed (carry-in)

- `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`: Done.
- `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md`: Done.
- `docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md`: Done.
- SMP proof ladder contract remains unchanged.

## Active focus (TASK-0013)

- Active task: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`.
- Active contract: `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md`.
- Implemented in this slice:
  - QoS syscall typed decode hard-rejects overflow (`-EINVAL`) instead of truncating.
  - Kernel spawn path default task QoS set to `Normal` (bootstrap/task-selftest helper unchanged).
  - New `source/services/timed/` service with:
    - deterministic windows (`Idle=8ms`, `Normal=4ms`, `Interactive=1ms`, `PerfBurst=0ns`),
    - bounded registration cap (`MAX_TIMERS_PER_OWNER=64`),
    - deterministic reject semantics (`STATUS_OVER_LIMIT`, invalid/malformed rejects).
  - Init wiring + routing for `timed` implemented (including self-route allowance needed for deterministic bring-up).
  - Selftest probe + markers added:
    - `timed: ready`
    - `SELFTEST: timed coalesce ok`
  - Harness/build integration updated (`justfile`, `Makefile`, `scripts/run-qemu-rv64.sh`, `scripts/qemu-test.sh`).
  - Reject test coverage added in `timed` unit tests (`test_reject_timer_registration_over_limit`).

## Closure notes

- Exec-path blocker status: **unblocked (runtime-verified)**.
  - fixed: deterministic `KPGF` in `execd` path caused by VMO arena map-base mismatch (`VMO_POOL` base vs AS identity map end).
  - fixed: subsequent deterministic `ALLOC-FAIL` in second exec path by reclaiming address spaces/pages on reap + enabling heap-page reclamation in `PageTable::drop` under `bringup_identity`.
  - latest proof snapshot (`RUN_PHASE=mmio RUN_TIMEOUT=140s ./scripts/qemu-test.sh`): `exit_code=0`, no `KPGF`, no `ALLOC ERROR`.
- QoS authority path now uses kernel-derived service identity (`execd`/`policyd`) instead of capability-slot shortcut.
- Explicit audit trail present:
  - kernel QoS decisions emit `QOS-AUDIT ...`,
  - `timed` registration path emits deterministic audit decisions.
- Full proof ladder rerun green after final patchset:
  - `cargo test --workspace`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Guardrails

- No alternate SMP/scheduler authority path.
- No drift in TASK-0012/TASK-0012B marker semantics.
- Keep deterministic, bounded proofs and modern virtio-mmio defaults.
