# Handoff Archive: TASK-0020 streams v2 (done)

**Date**: 2026-04-11  
**Status**: `TASK-0019` remains archived/done; `TASK-0020` is `Done`, `RFC-0033` is `Done`, and `RFC-0034` legacy closure scope (`TASK-0001..0020`) is `Done`.  
**Contract baseline**: `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md` (`Done`, execution SSOT closeout)

---

## What is stable now

- `TASK-0019` is closed and archived:
  - archive: `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
  - task: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (`Done`)
  - rfc: `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md` (`Done`)
- `TASK-0020` phase-0 contract + determinism lock is implemented:
  - deterministic fairness default + reject-label taxonomy are locked in task/RFC,
  - phase-0 typed mux boundary landed in `userspace/dsoftbus/src/mux_v2.rs`,
  - host proof surface landed in `userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs`,
  - task remains execution/proof SSOT per RFC process rules.
- `TASK-0020` host phase-1/2 test-first slices are implemented:
  - frame/state/keepalive contract tests: `userspace/dsoftbus/tests/mux_frame_state_keepalive_contract.rs`,
  - open/accept/data/rst integration tests: `userspace/dsoftbus/tests/mux_open_accept_data_rst_integration.rs`,
  - integration surface: `userspace/dsoftbus/src/mux_v2.rs` (`MuxHostEndpoint` + wire event pump).
- Guardrails for this slice are explicit:
  - host-first execution while OS backend is gated,
  - bounded stream/window/credit semantics,
  - typed ownership + Rust API hygiene (`newtype`, `#[must_use]`, no unsafe `Send`/`Sync` shortcuts).

## Proof snapshot carried forward

- See archived handoff for the full TASK-0019 proof set and marker closure:
  - `.cursor/handoff/archive/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- `TASK-0020` requirement-based host proofs are green:
  - `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture`
  - `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture`
  - `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture`
  - `cargo test -p dsoftbus -- --nocapture`
- `TASK-0020` completion is now claimed with proven host/OS/distributed/perf/soak/release-evidence gates.
- Phase regression commands have been executed after each host slice:
  - `cargo test -p dsoftbus -- --nocapture`
  - `just test-e2e`
  - `just test-os-dhcp`
- Additional OS-gated harness runs completed and reviewed:
  - `RUN_UNTIL_MARKER=1 just test-os`
  - `just test-dsoftbus-2vm` with summaries:
    - `artifacts/os2vm/runs/os2vm_1775990226/summary.json`
    - `artifacts/os2vm/runs/os2vm_1775990226/summary.txt`
    - `artifacts/os2vm/runs/os2vm_1775990226/release-evidence.json`
- Mux-specific OS marker ladder is now claimed and proven (`dsoftbus:mux ...` / `SELFTEST: mux ...`) via:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
- Distributed mux marker ladder is now proven on both nodes via:
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - summary: `artifacts/os2vm/runs/os2vm_1775990226/summary.json`
- Distributed deterministic perf-budget gate is now proven via `tools/os2vm.sh` phase `perf` (observed timings persisted in summary JSON).
- Distributed bounded soak hardening gate is now proven via `tools/os2vm.sh` phase `soak` (node liveness + fail/panic marker guards across two rounds).
- Release-ready machine-readable bundle is now emitted via `tools/os2vm.sh` (`release-evidence.json`).
- Legacy production closure obligations from `RFC-0034` are extracted and proven under `TASK-0020` without preempting later tasks.

## Relevant contracts for next slice

- `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
- `docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md` (`Done`)
- `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- `docs/adr/0005-dsoftbus-architecture.md`
- `tasks/IMPLEMENTATION-ORDER.md`
- `tasks/STATUS-BOARD.md`
- follow-on boundaries retained:
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Next actions

1. Start `TASK-0021` in strict numeric order (no preemption).
2. Keep `TASK-0021` host-first and avoid absorbing `TASK-0022` scope.
3. Preserve the no-fake-success marker discipline from TASK-0020 closure.
