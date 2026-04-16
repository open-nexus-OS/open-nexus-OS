# Next Task Preparation (Drift-Free)

## Candidate next execution
- **task**: execute `TASK-0023B` next as the required selftest-client refactor slice.
- **focus**: keep `TASK-0023` closure frozen while refactoring `selftest-client` in phased, no-behavior-change cuts before any `TASK-0024` feature work.

## Current proven baseline (must stay green)
- Host:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - required markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - forbidden fallback markers:
    - `dsoftbusd: transport selected tcp`
    - `dsoftbus: quic os disabled (fallback tcp)`
    - `SELFTEST: quic fallback ok`
- Hygiene:
  - `just dep-gate && just diag-os`

## Boundaries for next slice
- Keep `TASK-0021` and `TASK-0022` closed/done.
- Do not regress `TASK-0023` to fallback-only marker semantics.
- Do not absorb `TASK-0024` transport features into `TASK-0023B`.
- Keep `TASK-0023B` behavior-preserving: same marker order, same proof meanings, same reject behavior.
- Keep `main.rs` shrinking toward orchestration-only structure.
- Keep `TASK-0044` tuning matrix explicit (no hidden scope drift).

## Linked contracts
- `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
- `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
- `tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md`
- `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`
- `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
- `docs/testing/index.md`
- `docs/distributed/dsoftbus-lite.md`
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

## Ready condition
- Start from this frozen green baseline and begin `TASK-0023B` Phase 1 structural extraction:
  - create module skeletons first,
  - move one responsibility slice at a time,
  - rerun proof after each major extraction cut,
  - stop immediately on marker/order/reject-path drift.
