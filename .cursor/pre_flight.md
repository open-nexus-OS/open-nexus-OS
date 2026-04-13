# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating a task status to Done.
This is the anti-fake-success gate.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Narrow host/unit tests pass (task canonical command from active task doc; for TASK-0021: QUIC selection/reject/fallback requirement suites)
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

## Task-0021 manual addendum (when applicable)
- [ ] Behavior-first proof selection is explicit in task/RFC:
  - [ ] target behavior is stated in 1-3 lines,
  - [ ] main break point is explicit,
  - [ ] primary proof closes the core risk, secondary proof only closes a real blind spot.
- [ ] Scope stays kernel-unchanged and host-first while OS QUIC remains explicitly disabled by default.
- [ ] Runtime transport selection semantics are explicit and deterministic (`auto|tcp|quic`).
- [ ] `mode=quic` fails closed (no silent downgrade to TCP).
- [ ] `mode=auto` downgrade to TCP is explicit and auditable via deterministic markers.
- [ ] Required negative tests exist and are green:
  - [ ] `test_reject_quic_wrong_alpn`
  - [ ] `test_reject_quic_invalid_or_untrusted_cert`
  - [ ] `test_reject_quic_strict_mode_downgrade`
  - [ ] `test_auto_mode_fallback_marker_emitted`
- [ ] Transport-only success does not bypass DSoftBus authenticated session checks.
- [ ] OS marker ladder is updated/proven only when fallback behavior is genuinely exercised:
  - [ ] `dsoftbus: quic os disabled (fallback tcp)`
  - [ ] `SELFTEST: quic fallback ok`
  - [ ] `dsoftbusd: transport selected tcp` (or equivalent deterministic selector marker)
- [ ] Follow-on boundary (`TASK-0022`) is documented and not absorbed.

## Active progress snapshot (TASK-0021 kickoff, 2026-04-10)
- [x] Queue/order sync complete (`TASK-0020` Done, queue head moved to `TASK-0021`).
- [x] `TASK-0020` handoff archived and `current` switched to TASK-0021 baseline.
- [x] Working context files retargeted to TASK-0021 (`current_state`, `context_bundles`, `next_task_prep`, `pre_flight`, `stop_conditions`).
- [x] TASK-0021 status moved from `Draft` to `In Progress`.
- [x] TASK-0021 phase-A contract lock review completed (`RFC-0035` seed created and linked).
- [ ] TASK-0021 host requirement suites implemented and green.
- [ ] TASK-0021 OS fallback markers proven in canonical QEMU harness.

## Legacy manual profiles (reference only)
- [ ] TASK-0019 closeout checks are archived and tracked in task-local evidence (`Done`).
- [ ] TASK-0018 crashdump closeout checks are tracked in archived handoff and task-local evidence (`Done`).

## Post-implementation (before claiming "Done")
- [ ] Task doc still matches reality (status, proofs, touched paths)
- [ ] Proof commands and evidence are mirrored in handoff/task sections
- [ ] Header blocks updated (CONTEXT, TEST_COVERAGE, ADR links) where code was touched
- [ ] Docs synced only where contract/proof surfaces changed
- [ ] `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/stop_conditions.md` updated in same slice
