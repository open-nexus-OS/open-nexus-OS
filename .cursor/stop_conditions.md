# Stop Conditions (Task Completion)

<!--
CONTEXT
Hard stop conditions: a task is not "Done" unless all applicable items are satisfied.
-->

## Task completion stop conditions (must satisfy all applicable)
- [ ] All MUST Acceptance Criteria are implemented and proven.
- [ ] All stated Security Invariants are enforced and have negative tests where applicable (`test_reject_*`).
- [ ] No regressions against `.cursor/current_state.md` constraints/invariants.
- [ ] Proof artifacts exist and are referenced in task/handoff docs (tests, markers, logs).
- [ ] For `tools/os2vm.sh` proof paths, typed summary artifacts are present and linked (`os2vm-summary-*.json` / `.txt`).
- [ ] Header blocks and docs are updated where boundaries/contracts/proofs changed.
- [ ] Rust API hygiene is reviewed for touched paths (`newtype`/ownership/`#[must_use]` where sensible).
- [ ] `Send`/`Sync` discipline is reviewed (no unsafe shortcut traits in daemon/session state paths).

## TASK-0020 class stop conditions (DSoftBus mux/flow-control/keepalive v2)
- [ ] Kernel remains untouched; task remains host-first while OS backend is explicitly gated.
- [ ] Mux contract is deterministic and bounded:
  - [ ] explicit caps for stream count/frame payload/buffered bytes/window-credit deltas,
  - [ ] stable fail-closed reject labels for oversize/invalid-state/unknown-stream/credit violations.
- [ ] Mux operation is accepted only on authenticated session context.
- [ ] Backpressure semantics stay explicit (`WouldBlock`/credit exhaustion), with no hidden unbounded queues.
- [ ] Priority policy provides bounded starvation (high-pri favored, low-pri still progresses).
- [ ] Keepalive behavior is bounded and deterministic (timeout leads to explicit teardown).
- [ ] Required negative tests are green:
  - [ ] `test_reject_mux_frame_oversize`
  - [ ] `test_reject_invalid_stream_state_transition`
  - [ ] `test_reject_window_credit_overflow_or_underflow`
  - [ ] `test_reject_unknown_stream_frame`
- [ ] Rust/API hygiene gate is green:
  - [ ] `newtype` wrappers are used where stream/credit/priority confusion is otherwise possible,
  - [ ] mutable mux/session state ownership boundaries are explicit,
  - [ ] `#[must_use]` on critical transition/accounting outcomes,
  - [ ] no `unsafe` `Send`/`Sync` workarounds were introduced.
- [ ] Host proof is green:
  - [ ] deterministic interleaving/fairness behavior,
  - [ ] bounded backpressure behavior,
  - [ ] keepalive timeout behavior,
  - [ ] seeded deterministic state-machine coverage for ordering/credit invariants.
- [ ] OS proof is green and sequential only when backend gate is met:
  - [ ] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` (if required by scope/profile)
- [ ] If phase-D performance closure is claimed, distributed `phase: perf` budgets are green and recorded in 2-VM summary artifacts.
- [ ] If phase-E hardening closure is claimed, distributed `phase: soak` is green with zero fail/panic marker hits in summary artifacts.
- [ ] If phase-E hardening closure is claimed, a deterministic `release-evidence.json` bundle is emitted and reviewed.
- [ ] QEMU marker ladder includes and proves (when OS gate is met):
  - [ ] `dsoftbus:mux session up`
  - [ ] `dsoftbus:mux data ok`
  - [ ] `SELFTEST: mux pri control ok`
  - [ ] `SELFTEST: mux bulk ok`
  - [ ] `SELFTEST: mux backpressure ok`
- [ ] Build hygiene stays green when OS code is touched:
  - [ ] `just dep-gate`
  - [ ] `just diag-os`
- [ ] No unresolved RED decision points remain in `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`.
- [ ] No follow-on scope (`TASK-0021`, `TASK-0022`) was silently absorbed.

## Legacy stop-condition profiles (reference only)
- [ ] TASK-0019-class ABI guardrail stop conditions: use archived closeout evidence in `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (`Done`).
- [ ] TASK-0018-class crashdump v1 stop conditions: use archived closeout evidence in `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (`Done`).
- [ ] TASK-0017-class remote statefs RW ACL/audit stop conditions: use archived closeout evidence in `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (`Done`).

## Never claim success if…
- [ ] Tests were not run where applicable.
- [ ] Markers say `ok/ready` but behavior is stubbed.
- [ ] Scope expanded beyond touched-path allowlist without explicit plan/task update.
- [ ] QEMU proofs were run in parallel and produced contention artifacts.
- [ ] Wire layout, retry budgets, ACL/audit behavior, or marker semantics changed silently.

## Active progress snapshot (TASK-0020 requirement-based progression, 2026-04-11)
- [x] Deterministic mux host surfaces are implemented in `userspace/dsoftbus`.
- [x] Required `test_reject_*` suite is present and green (including unauthenticated session reject).
- [x] Host proof floor is green:
  - [x] `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture`
  - [x] `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture`
  - [x] `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture`
  - [x] `cargo test -p dsoftbus -- --nocapture`
- [x] Per-phase regression commands executed:
  - [x] `just test-e2e`
  - [x] `just test-os-dhcp`
- [x] Canonical OS harnesses executed sequentially:
  - [x] `RUN_UNTIL_MARKER=1 just test-os`
  - [x] `just test-dsoftbus-2vm` (summary artifacts reviewed: `os2vm_1775990226`)
- [x] No follow-on scope (`TASK-0021`, `TASK-0022`) absorbed.
- [x] OS mux-specific marker closure is proven in canonical smoke with `REQUIRE_DSOFTBUS=1`:
  - [x] `dsoftbus:mux session up`
  - [x] `dsoftbus:mux data ok`
  - [x] `SELFTEST: mux pri control ok`
  - [x] `SELFTEST: mux bulk ok`
  - [x] `SELFTEST: mux backpressure ok`
- [x] Distributed mux-ladder closure is proven in canonical 2-VM harness (`tools/os2vm.sh` phase `mux`).
- [x] Distributed deterministic perf-budget closure is proven in canonical 2-VM harness (`tools/os2vm.sh` phase `perf`).
- [x] Distributed bounded soak-hardening closure is proven in canonical 2-VM harness (`tools/os2vm.sh` phase `soak`).
- [x] Distributed release-evidence bundle closure is proven in canonical 2-VM harness (`tools/os2vm.sh` emits `release-evidence.json`).
- [x] Legacy `TASK-0001..0020` closure obligations are fully encoded/proven under `TASK-0020` gate sections.
- [x] Sequencing lock was respected during closure (no proof-execution start for `TASK-0021+` before `TASK-0020` legacy-production closeout).
