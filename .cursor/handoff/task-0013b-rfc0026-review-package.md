# TASK-0013B / RFC-0026 Review Package

Date: 2026-02-12
Status: Review ready (task is `Done`)

## Scope closed in this package

- RFC-0026 phase-0 baseline + acceptance thresholds
- RFC-0026 phase-1 control-plane reuse/caching slices
- RFC-0026 phase-2 bounded data-plane alignment slices
- Full host + QEMU re-proof with deterministic marker ladder

## Evidence (authoritative run set)

- `cargo test -p nexus-ipc -- --nocapture` -> green
- `cargo test -p timed -- --nocapture` -> green
- `cargo test --workspace` -> green
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` -> green
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` -> green (`24.664s`)
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` -> green (`1:23.77`)
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=180s ./scripts/qemu-test.sh` -> green (`1:24.99`)

## Acceptance result

- `improvement_only` target achieved:
  - baseline strict SMP=2 90s wall-clock: `1:24.93`
  - post-change strict SMP=2 90s wall-clock: `1:23.77`
  - measurable uplift without functional marker regressions

## Known limits / residual risk

- Host load variance still affects absolute QEMU margin.
- `bundlemgrd` fetch path is stable and preserved; deeper optimization is intentionally isolated to follow-up work.
- Control-plane evidence is valid only with sequential QEMU execution.
- Isolated `bundlemgrd` metrics-client cache follow-up was tested and rejected by runtime proof (marker regression); change was reverted.

## Test discipline contract (hard)

- Do not run parallel QEMU smoke proofs.
- Always run QEMU proofs sequentially under the same marker floor.
- Treat any parallel-run result as invalid evidence and rerun sequentially.
