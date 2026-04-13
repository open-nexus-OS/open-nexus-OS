# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating a task status to Done.
This is the anti-fake-success gate.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Narrow host/unit tests pass (task canonical command from active task doc; for TASK-0020: mux host/reject/fairness/backpressure/keepalive tests)
- [ ] OS dependency gate (when OS code touched): `just dep-gate`
- [ ] OS diagnostics compile (when OS code touched): `just diag-os`
- [ ] Single-VM QEMU marker proof is green (only when OS backend gate is met): `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Cross-VM QEMU proof is green when the active task explicitly requires it: `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- [ ] If `tools/os2vm.sh` path was used, summary artifacts were reviewed (`os2vm-summary-<runId>.json` and `.txt`)
- [ ] If distributed performance closure is claimed, `tools/os2vm.sh` phase `perf` budgets are green in summary evidence.
- [ ] If final hardening closure is claimed, `tools/os2vm.sh` phase `soak` is green with zero fail/panic marker hits.
- [ ] If final hardening closure is claimed, `release-evidence.json` is present and reviewed for the run.
- [ ] QEMU runs were executed sequentially (no parallel smoke/harness runs contending on shared artifacts)
- [ ] Determinism floor respected: marker order and bounded retry semantics preserved
- [ ] No new lints in touched files (run task/workspace lint policy)

## Manual (agent verifies, then documents proof)
- [ ] Acceptance Criteria satisfied (task + linked RFC/ADR)
- [ ] Tests validate the desired behavior (Soll-Zustand), not implementation quirks
- [ ] No fake-success logs/markers introduced (`ready/ok` only after real behavior)
- [ ] Ownership/authority boundaries stayed aligned with linked contracts
- [ ] No follow-on feature scope leaked into this task
- [ ] Header discipline checked in touched code/docs (CONTEXT/OWNERS/TEST_COVERAGE where applicable)
- [ ] Rust construct hygiene reviewed where relevant (`newtype` candidates, ownership boundaries, `#[must_use]` for critical return values)
- [ ] `Send`/`Sync` discipline reviewed (no blanket/unsafe trait shortcuts in daemon/session state)

## Task-0020 manual addendum (when applicable)
- [ ] Scope stays kernel-unchanged and host-first while OS backend remains explicitly gated.
- [ ] Bounded limits are explicit for stream count, payload size, buffered bytes, and window/credit deltas.
- [ ] Mux operations are accepted only on authenticated session context.
- [ ] Backpressure semantics are explicit (`WouldBlock`/credit exhaustion), not hidden by unbounded queues.
- [ ] Keepalive cadence/timeout behavior is deterministic and bounded.
- [ ] Required negative tests exist and are green:
  - [ ] `test_reject_mux_frame_oversize`
  - [ ] `test_reject_invalid_stream_state_transition`
  - [ ] `test_reject_window_credit_overflow_or_underflow`
  - [ ] `test_reject_unknown_stream_frame`
- [ ] Rust/API hygiene is enforced:
  - [ ] `newtype` wrappers for stream/window/credit/priority domains where appropriate,
  - [ ] explicit ownership model for mutable mux session state,
  - [ ] `#[must_use]` on critical transition/accounting outcomes,
  - [ ] no `unsafe` `Send`/`Sync` shortcuts.
- [ ] OS proof environment uses canonical harness defaults (modern virtio-mmio behavior; no legacy-only assumptions).
- [ ] QEMU marker ladder is updated/proven only when OS backend gate is genuinely met:
  - [ ] `dsoftbus:mux session up`
  - [ ] `dsoftbus:mux data ok`
  - [ ] `SELFTEST: mux pri control ok`
  - [ ] `SELFTEST: mux bulk ok`
  - [ ] `SELFTEST: mux backpressure ok`
- [ ] Follow-on boundaries (`TASK-0021`, `TASK-0022`) are documented and not absorbed.

## Active progress snapshot (TASK-0020 requirement-based slices, 2026-04-11)
- [x] Narrow host/unit tests pass (requirement-based host proof commands):
  - [x] `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds -- --nocapture`
  - [x] `cargo test -p dsoftbus --test mux_frame_state_keepalive_contract -- --nocapture`
  - [x] `cargo test -p dsoftbus --test mux_open_accept_data_rst_integration -- --nocapture`
  - [x] `cargo test -p dsoftbus -- --nocapture`
- [x] Determinism floor respected in host tests (stable reject labels + bounded tick/credit semantics + deterministic lifecycle).
- [x] Tests validate desired behavior (reject taxonomy, seeded sequence invariants, mixed-priority fairness pressure, naming rejects, integration propagation).
- [x] Rust construct hygiene reviewed (`newtype` domains + `#[must_use]` outcomes in mux phase surfaces).
- [x] `Send`/`Sync` discipline reviewed (no blanket/unsafe shortcuts introduced).
- [x] Per-phase regression set executed:
  - [x] `just test-e2e`
  - [x] `just test-os-dhcp`
- [x] Canonical OS harnesses executed and reviewed:
  - [x] `RUN_UNTIL_MARKER=1 just test-os`
  - [x] `just test-dsoftbus-2vm` (+ summary artifacts reviewed: `os2vm_1775990226`)
- [x] Follow-on scope not absorbed (`TASK-0021`/`TASK-0022` unchanged).
- [x] Mux-specific OS marker ladder proven with canonical gate:
  - [x] `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
  - [x] markers present: `dsoftbus:mux session up`, `dsoftbus:mux data ok`, `SELFTEST: mux pri control ok`, `SELFTEST: mux bulk ok`, `SELFTEST: mux backpressure ok`
- [x] Distributed mux ladder proven with canonical 2-VM gate:
  - [x] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - [x] phase `mux` marker ladder present on both nodes (evidence: `os2vm_1775990226`)
- [x] Distributed perf-budget gate proven with canonical 2-VM gate:
  - [x] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - [x] phase `perf` budget checks passed (evidence: `os2vm_1775990226`)
- [x] Distributed soak hardening gate proven with canonical 2-VM gate:
  - [x] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - [x] phase `soak` checks passed (evidence: `os2vm_1775990226`, rounds: 2, fail/panic marker hits: 0)
- [x] Release evidence bundle emitted and reviewed:
  - [x] `artifacts/os2vm/runs/os2vm_1775990226/release-evidence.json`
- [x] Legacy closure obligations from `RFC-0034` are fully proven under `TASK-0020` scope.
- [x] Sequencing discipline was enforced during TASK-0020 closure (no proof-execution start for `TASK-0021+` before closeout).
- [x] TASK-0020 closeout status aligned (`tasks/TASK-0020...` is now `Done`; linked RFC statuses synced).

## Legacy manual profiles (reference only)
- [ ] TASK-0019 closeout checks are archived and tracked in task-local evidence (`Done`).
- [ ] TASK-0018 crashdump closeout checks are tracked in archived handoff and task-local evidence (`Done`).

## Post-implementation (before claiming "Done")
- [ ] Task doc still matches reality (status, proofs, touched paths)
- [ ] Proof commands and evidence are mirrored in handoff/task sections
- [ ] Header blocks updated (CONTEXT, TEST_COVERAGE, ADR links) where code was touched
- [ ] Docs synced only where contract/proof surfaces changed
- [ ] `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/stop_conditions.md` updated in same slice
