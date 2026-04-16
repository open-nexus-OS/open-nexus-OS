# Cursor Current State (SSOT)

## Current architecture state
- **last_decision**: `TASK-0023` / `RFC-0037` are closed as real OS QUIC session baseline; execution focus now shifts to `TASK-0023B` / `RFC-0038` for the selftest-client architecture refactor.
- **active_constraints**:
  - keep `TASK-0021` and `TASK-0022` frozen as done baselines,
  - keep marker honesty strict (`ok/ready` only after real behavior),
  - keep `TASK-0023` closure frozen as the required OS QUIC baseline,
  - keep `TASK-0023B` behavior-preserving (no feature drift under refactor label),
  - keep no_std-safe boundaries explicit (no hidden std/runtime coupling claims),
  - keep `TASK-0024` blocked behind `TASK-0023B`,
  - keep `TASK-0044` as follow-up tuning scope (no silent scope absorption).

## Current focus (execution)
- **active_task**: `TASK-0023B` selftest-client production-grade deterministic test architecture refactor.
- **seed_contract**:
  - `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
  - `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
  - `docs/testing/index.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/IMPLEMENTATION-ORDER.md`

## TASK-0023 closure snapshot (2026-04-16)
- Host proofs green:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS proof green:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - observed markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - fallback markers are forbidden in this profile and absent.
- Service hardening:
  - pure QUIC frame helpers extracted to `source/services/dsoftbusd/src/os/session/quic_frame.rs`,
  - reject-path unit coverage added in `source/services/dsoftbusd/tests/p0_unit.rs`.

## Scope boundaries reaffirmed
- `TASK-0023`: closed as real OS session path (production-floor scope).
- `TASK-0023B`: required refactor/hardening slice before further transport expansion.
- `TASK-0024`: follow-up transport breadth work after `TASK-0023B` (no reopening `TASK-0023` closure semantics).
- `TASK-0044`: explicit tuning/performance breadth follow-up.

## Next handoff target
- Keep proofs reproducible and deterministic while advancing `TASK-0023B` Phase 1 structural extraction.
