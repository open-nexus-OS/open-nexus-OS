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

## TASK-0022 class stop conditions (DSoftBus core no_std transport abstraction)
- [ ] Behavior-first proof shape is documented and enforced:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest,
  - [ ] secondary proof exists only for a real blind spot.
- [ ] Kernel remains untouched; task remains core/no_std extraction only.
- [ ] Plane boundaries are explicit and preserved:
  - [ ] discovery/auth-session/transmission responsibilities remain separated,
  - [ ] policy authorization is not absorbed into transport core.
- [ ] Security invariants are enforced:
  - [ ] identity remains channel-authoritative (`sender_service_id`), not payload-derived,
  - [ ] correlation/replay checks remain bounded and deterministic,
  - [ ] unauthenticated paths fail closed.
- [ ] Required negative tests are green:
  - [ ] `test_reject_invalid_state_transition`
  - [ ] `test_reject_nonce_mismatch_or_stale_reply`
  - [ ] `test_reject_oversize_frame_or_record`
  - [ ] `test_reject_unauthenticated_message_path`
  - [ ] `test_reject_payload_identity_spoof_vs_sender_service_id`
- [ ] Host baseline regression proof is green:
  - [ ] `just test-dsoftbus-quic` stays green (no TASK-0021 regression).
- [ ] Rust API discipline is proven in touched boundaries:
  - [ ] `newtype`/ownership/`#[must_use]` expectations are enforced where safety-relevant,
  - [ ] `Send`/`Sync` behavior is reviewed without unsafe blanket trait shortcuts.
- [ ] Zero-copy discipline is explicit:
  - [ ] bulk-path changes prefer borrow/VMO/filebuffer style where possible,
  - [ ] unavoidable copies are bounded and documented.
- [ ] OS proof is green and sequential when OS integration hooks are touched:
  - [ ] `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - [ ] expected markers are present:
    - [ ] `dsoftbusd: ready`
    - [ ] `dsoftbusd: auth ok`
- [ ] If distributed behavior is asserted, 2-VM proofs are green and summaries are reviewed:
  - [ ] `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - [ ] `summary.{json,txt}` + `release-evidence.json` reviewed for run.
- [ ] Build hygiene stays green when OS code is touched:
  - [ ] `just dep-gate`
  - [ ] `just diag-os`
- [ ] No unresolved RED decision points remain in `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`.
- [ ] No follow-on scope (`TASK-0023` / `TASK-0044`) was silently absorbed.

## TASK-0023 class stop conditions (DSoftBus QUIC v2 OS-enabled gated)
- [ ] Blocked-state gate outcome is explicit and synchronized:
  - [ ] task remains `Blocked` until no_std QUIC feasibility evidence exists,
  - [ ] `RFC-0037` is linked as the active contract seed,
  - [ ] follow-up implementation route is explicit (`TASK-0024`),
  - [ ] tuning follow-up remains explicit (`TASK-0044`).
- [ ] Behavior-first proof shape is explicit and maintained:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest.
- [ ] Security reject contract is current and mirrors real requirement-named tests:
  - [ ] `test_reject_quic_strict_mode_downgrade`
  - [ ] `test_reject_quic_invalid_or_untrusted_cert`
  - [ ] `test_reject_quic_wrong_alpn`
- [ ] Marker contract remains honest while blocked:
  - [ ] `dsoftbus: quic os disabled (fallback tcp)`
  - [ ] `SELFTEST: quic fallback ok`
- [ ] No QUIC success markers are emitted in OS/QEMU while task is blocked.
- [ ] Rust API discipline for follow-up implementation is documented and enforced:
  - [ ] `newtype`/ownership/`#[must_use]` expectations are explicit for transport/session boundaries,
  - [ ] `Send`/`Sync` expectations are reviewed without unsafe blanket trait shortcuts.
- [ ] Modern virtio-mmio proof floor is preserved for OS/QEMU closure claims.
- [ ] No OS QUIC implementation scope was silently absorbed into unrelated tasks.

## Legacy stop-condition profiles (reference only)
- [ ] TASK-0021-class QUIC scaffold stop conditions: use archived closure evidence in `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md` (`Done`).
- [ ] TASK-0019-class ABI guardrail stop conditions: use archived closeout evidence in `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (`Done`).
- [ ] TASK-0018-class crashdump v1 stop conditions: use archived closeout evidence in `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (`Done`).
- [ ] TASK-0017-class remote statefs RW ACL/audit stop conditions: use archived closeout evidence in `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md` (`Done`).

## Never claim success if…
- [ ] Tests were not run where applicable.
- [ ] Markers say `ok/ready` but behavior is stubbed.
- [ ] Scope expanded beyond touched-path allowlist without explicit plan/task update.
- [ ] QEMU proofs were run in parallel and produced contention artifacts.
- [ ] Wire layout, retry budgets, ACL/audit behavior, or marker semantics changed silently.

## Active progress snapshot (TASK-0023 in-progress gate sync, 2026-04-15)
- [x] Queue/order metadata remains synchronized (`TASK-0023` queue head, explicitly blocked).
- [x] Handoff archived for `TASK-0022`; `current` switched to `TASK-0023` prep state.
- [x] Core `.cursor` working files retargeted to `TASK-0023` gate-prep context.
- [x] `RFC-0037` contract seed added and linked to `TASK-0023`.
- [x] `TASK-0023` follow-up routing synchronized (`TASK-0024`, `TASK-0044`).
- [x] `TASK-0023` RED feasibility risk resolved as explicit blocked gate outcome.
- [x] `TASK-0023` security proof test names synchronized to real host reject suites.
