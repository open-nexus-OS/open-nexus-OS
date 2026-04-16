# Current Handoff: TASK-0023B selftest-client refactor kickoff

**Date**: 2026-04-16  
**Status**: `TASK-0023` is archived as `Done`; `TASK-0023B` is now the active queue head and contract seed for the selftest-client refactor.  
**Execution SSOT**: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`

## What changed
- `TASK-0023` closure is frozen and archived as the proven OS QUIC baseline.
- `TASK-0023B` now owns the next execution slice before `TASK-0024`.
- `RFC-0038` defines the refactor contract: minimal `main.rs`, adaptive `os_lite` structure, and no-behavior-change phased extraction.
- Marker honesty was hardened further: if the refactor reveals logic bugs or fake-success markers, they must be fixed and converted into real behavior/proof signals.

## Current execution posture
- Phase order is fixed:
  - Phase 1: structural extraction without behavior change,
  - Phase 2: maintainability/extensibility cleanup without feature drift,
  - Phase 3: standards + closure review with full proof floor.
- The full ladder in `scripts/qemu-test.sh` remains authoritative, not only the QUIC subset.
- `main.rs` must shrink toward entry wiring + top-level orchestration only.

## Frozen baseline that must stay green
- Host:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - required QUIC subset markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`

## Next handoff target
- Execute `TASK-0023B` Phase 1 by extracting the first `os_lite` seams from `source/apps/selftest-client/src/main.rs`.
- Do not absorb `TASK-0024` feature work into the refactor.
