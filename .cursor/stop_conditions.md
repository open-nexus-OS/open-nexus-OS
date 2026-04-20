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

## TASK-0023 class stop conditions (DSoftBus QUIC v2 OS-enabled)
- [ ] Real OS QUIC session behavior is implemented and synchronized:
  - [ ] `RFC-0037` and task doc reflect enabled session posture,
  - [ ] follow-up routes remain explicit (`TASK-0024`, `TASK-0044`).
- [ ] Behavior-first proof shape is explicit and maintained:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest.
- [ ] Security reject contract is current and mirrors requirement-named tests:
  - [ ] `test_reject_quic_strict_mode_downgrade`
  - [ ] `test_reject_quic_invalid_or_untrusted_cert`
  - [ ] `test_reject_quic_wrong_alpn`
  - [ ] `test_reject_quic_frame_bad_magic`
  - [ ] `test_reject_quic_frame_truncated_payload`
  - [ ] `test_reject_quic_frame_oversized_payload_encode`
- [ ] Phase-D feasibility guard contract stays green:
  - [ ] `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
  - [ ] `test_reject_quic_feasibility_std_runtime_coupling`
  - [ ] `test_reject_quic_feasibility_non_deterministic_timer_assumptions`
  - [ ] `test_reject_quic_feasibility_entropy_prerequisites_unsatisfied`
  - [ ] `test_reject_quic_feasibility_unbounded_loss_retry_budget`
- [ ] Marker contract is honest in QUIC-required OS profile:
  - [ ] required:
    - [ ] `dsoftbusd: transport selected quic`
    - [ ] `dsoftbusd: auth ok`
    - [ ] `dsoftbusd: os session ok`
    - [ ] `SELFTEST: quic session ok`
  - [ ] forbidden:
    - [ ] `dsoftbusd: transport selected tcp`
    - [ ] `dsoftbus: quic os disabled (fallback tcp)`
    - [ ] `SELFTEST: quic fallback ok`
- [ ] Rust API discipline remains enforced:
  - [ ] `newtype`/ownership/`#[must_use]` expectations are explicit for transport/session boundaries,
  - [ ] `Send`/`Sync` expectations are reviewed without unsafe blanket trait shortcuts.
- [ ] Modern virtio-mmio proof floor is preserved for OS/QEMU closure claims.
- [ ] No follow-up scope is silently absorbed into unrelated tasks.

## TASK-0023B class stop conditions (selftest-client deterministic refactor)
- [x] Phase sequence is completed in order with no skipped closure gate:
  - [x] Phase 1 structural extraction,
  - [x] Phase 2 maintainability/extensibility cleanup,
  - [x] Phase 3 standards/closure review,
  - [x] Phase 4 proof-manifest as marker SSOT + profile-aware harness,
  - [x] Phase 5 manifest schema-v2 split + signed evidence bundles,
  - [x] Phase 6 replay/diff/bisect tooling + cross-host floor (functionally closed 2026-04-20; external CI-runner replay artifact for P6-05 is the single remaining environmental step — see `docs/testing/replay-and-bisect.md` §7-§11).
- [ ] Behavior-preserving refactor contract holds:
  - [ ] marker ordering semantics remain unchanged,
  - [ ] marker meanings remain unchanged,
  - [ ] reject behavior remains fail-closed,
  - [ ] no `TASK-0024` feature scope was absorbed.
- [ ] Proof floor is rerun after each major extraction cut and remains green:
  - [ ] `cargo test -p dsoftbusd -- --nocapture`
  - [ ] `just test-dsoftbus-quic`
  - [ ] `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- [ ] Full ladder authority is preserved:
  - [ ] `scripts/qemu-test.sh` remains the authoritative proof contract,
  - [ ] QUIC markers remain a critical subset, not the whole closure claim.
- [ ] Marker honesty is enforced:
  - [ ] success markers are emitted only after verified behavior/state,
  - [ ] any discovered logic bug or fake-success marker path is corrected before closure,
  - [ ] dishonest markers are converted into honest behavior/proof markers.
- [ ] `main.rs` minimality is materially achieved:
  - [ ] `main.rs` is reduced to entry wiring + top-level orchestration,
  - [ ] no parser/encoder/decoder logic remains in `main.rs`,
  - [ ] no retry/deadline/reply-matching loops remain in `main.rs`,
  - [ ] no service-specific marker branching remains in `main.rs`.
- [ ] Architecture contract stays synchronized:
  - [ ] `TASK-0023B` remains execution SSOT,
  - [ ] `RFC-0038` remains architecture/contract seed,
  - [ ] `TASK-0023` closure baseline remains frozen and green,
  - [ ] `TASK-0024` remains queued after `TASK-0023B`.
- [ ] Rust discipline review is completed where sensible:
  - [ ] `newtype` candidates are reviewed,
  - [ ] ownership boundaries are explicit,
  - [ ] `#[must_use]` is applied to decision-bearing results where useful,
  - [ ] `Send`/`Sync` expectations are reviewed without unsafe shortcut traits.

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

## Active progress snapshot (TASK-0023B closure refresh, 2026-04-20)
- [x] `TASK-0023` is archived as a frozen handoff baseline.
- [x] Active `.cursor` workfiles point to `TASK-0023B` / `RFC-0038`.
- [x] Refactor-specific stop conditions cover phase order, marker honesty, and `main.rs` minimality.
- [x] Queue order is synchronized: `TASK-0023B` before `TASK-0024`.
- [x] All six TASK-0023B phases functionally closed; `RFC-0038` advanced to `Done`; `TASK-0023B` advanced to `In Review`.
- [ ] External CI-runner replay artifact for P6-05 captured + status flip applied per `docs/testing/replay-and-bisect.md` §7-§11. After that, `TASK-0023B` moves to `Done` and queue head advances to `TASK-0024`.
