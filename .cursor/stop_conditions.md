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

## TASK-0021 class stop conditions (DSoftBus QUIC v1 host-first scaffold)
- [ ] Behavior-first proof shape is documented and enforced:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest,
  - [ ] secondary proof exists only for a real blind spot.
- [ ] Kernel remains untouched; task remains host-first while OS QUIC is explicitly disabled-by-default.
- [ ] Transport selection semantics are deterministic and explicit (`auto|tcp|quic`).
- [ ] Security invariants are enforced:
  - [ ] `mode=quic` fails closed when QUIC requirements are unmet,
  - [ ] `mode=auto` fallback to TCP is explicit and marker-audited,
  - [ ] ALPN/cert mismatch paths are deterministic rejects.
- [ ] Required negative tests are green:
  - [ ] `test_reject_quic_wrong_alpn`
  - [ ] `test_reject_quic_invalid_or_untrusted_cert`
  - [ ] `test_reject_quic_strict_mode_downgrade`
  - [ ] `test_auto_mode_fallback_marker_emitted`
- [ ] Host proof is green:
  - [ ] QUIC connect + stream path is proven in host suites,
  - [ ] strict/downgrade behavior is fail-closed,
  - [ ] fallback behavior is deterministic and observable.
- [ ] OS proof is green and sequential when fallback behavior is asserted:
  - [ ] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] expected fallback markers are present:
    - [ ] `dsoftbus: quic os disabled (fallback tcp)`
    - [ ] `SELFTEST: quic fallback ok`
    - [ ] `dsoftbusd: transport selected tcp` (or equivalent deterministic marker)
- [ ] If distributed behavior is asserted, 2-VM proofs are green and summaries are reviewed:
  - [ ] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - [ ] `summary.{json,txt}` + `release-evidence.json` reviewed for run.
- [ ] Build hygiene stays green when OS code is touched:
  - [ ] `just dep-gate`
  - [ ] `just diag-os`
- [ ] No unresolved RED decision points remain in `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`.
- [ ] No follow-on scope (`TASK-0022`) was silently absorbed.

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

## Active progress snapshot (TASK-0021 kickoff, 2026-04-10)
- [x] Queue/order metadata synchronized (`TASK-0020` done; queue head is `TASK-0021`).
- [x] Handoff archived for `TASK-0020` and current handoff switched to `TASK-0021`.
- [x] Core `.cursor` working files retargeted for `TASK-0021`.
- [x] TASK-0021 task status promoted to `In Progress`.
- [x] TASK-0021 phase-A contract lock finalized (`RFC-0035` seed created and linked).
- [ ] TASK-0021 host requirement-named tests implemented and green.
- [ ] TASK-0021 OS fallback marker proofs green in canonical QEMU harness.
