# Pre-Flight (End-of-Task Quality Gate)

<!--
CONTEXT
Run this checklist before updating a task status to Done.
This is the anti-fake-success gate.
-->

## Automatic (must be green when applicable)
- [ ] Host diagnostics compile (when host code touched): `just diag-host`
- [ ] Narrow host/unit tests pass (task canonical commands from active task doc; for TASK-0023: `just test-dsoftbus-quic` + requirement-named QUIC reject suites)
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

## Task-0022 manual addendum (when applicable)
- [ ] Behavior-first proof selection is explicit in task/RFC:
  - [ ] target behavior is stated in 1-3 lines,
  - [ ] main break point is explicit,
  - [ ] primary proof closes the core risk, secondary proof only closes a real blind spot.
- [ ] Scope stays kernel-unchanged and does not pre-enable `TASK-0023` OS QUIC behavior.
- [ ] Plane separation remains explicit in core contracts:
  - [ ] discovery / auth-session / transmission boundaries are distinct,
  - [ ] auth success is not treated as policy authorization.
- [ ] Identity authority remains channel-bound (`sender_service_id`), never payload-derived.
- [ ] Zero-copy-first discipline is preserved:
  - [ ] bulk path prefers borrow/VMO/filebuffer style where possible,
  - [ ] unavoidable copies are bounded and documented.
- [ ] Rust API discipline is applied where safety-relevant:
  - [ ] `newtype` used for domain IDs/handles,
  - [ ] `#[must_use]` on decision-bearing return values,
  - [ ] ownership transfer semantics are explicit,
  - [ ] `Send`/`Sync` behavior reviewed without unsafe blanket shortcuts.
- [ ] `TASK-0021` baseline remains green during refactor (`just test-dsoftbus-quic`).
- [ ] Modern virtio-mmio proof floor is preserved for OS/QEMU closure claims.

## Task-0023 manual addendum (when applicable)
- [ ] Enabled-session contract is explicit and honest:
  - [ ] OS QUIC session path is real (not marker-only),
  - [ ] required QUIC markers are emitted only after real auth/session behavior,
  - [ ] fallback markers are forbidden in QUIC-required profile.
- [ ] Contract seed alignment is explicit:
  - [ ] `RFC-0037` exists and is linked from `TASK-0023`,
  - [ ] follow-up ownership remains explicit (`TASK-0024`, `TASK-0044`).
- [ ] Behavior-first proof shape is explicit in task/RFC:
  - [ ] target behavior is stated in 1-3 lines,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest.
- [ ] Security reject discipline is current and requirement-named:
  - [ ] strict mode downgrade reject stays fail-closed,
  - [ ] cert trust rejects stay fail-closed,
  - [ ] ALPN mismatch rejects stay fail-closed.
- [ ] Phase-D feasibility guard suite remains green:
  - [ ] `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture` is green,
  - [ ] `test_reject_quic_feasibility_std_runtime_coupling` is green,
  - [ ] `test_reject_quic_feasibility_non_deterministic_timer_assumptions` is green,
  - [ ] `test_reject_quic_feasibility_entropy_prerequisites_unsatisfied` is green,
  - [ ] `test_reject_quic_feasibility_unbounded_loss_retry_budget` is green.
- [ ] Service-side frame reject suite is green:
  - [ ] `cargo test -p dsoftbusd --test p0_unit -- --nocapture`,
  - [ ] `test_reject_quic_frame_bad_magic`,
  - [ ] `test_reject_quic_frame_truncated_payload`,
  - [ ] `test_reject_quic_frame_oversized_payload_encode`.
- [ ] Rust discipline is captured for follow-up implementation:
  - [ ] newtype candidates and ownership boundaries are documented,
  - [ ] `#[must_use]` expectations are explicit for decision-bearing APIs,
  - [ ] `Send`/`Sync` review expectations are explicit without unsafe blanket traits.
- [ ] Follow-up routing is explicit and synchronized:
  - [ ] implementation route is `TASK-0024`,
  - [ ] tuning follow-up remains `TASK-0044`,
  - [ ] no scope absorption into unrelated active tasks.
- [ ] QEMU marker contract for QUIC-required profile is proven:
  - [ ] required markers:
    - [ ] `dsoftbusd: transport selected quic`
    - [ ] `dsoftbusd: auth ok`
    - [ ] `dsoftbusd: os session ok`
    - [ ] `SELFTEST: quic session ok`
  - [ ] forbidden markers absent:
    - [ ] `dsoftbusd: transport selected tcp`
    - [ ] `dsoftbus: quic os disabled (fallback tcp)`
    - [ ] `SELFTEST: quic fallback ok`

## Task-0023B manual addendum (when applicable)
- [ ] Refactor phase order is respected:
  - [ ] Phase 1 structural extraction first,
  - [ ] Phase 2 maintainability/extensibility cleanup second,
  - [ ] Phase 3 standards/closure review last.
- [ ] Refactor remains behavior-preserving:
  - [ ] marker order is unchanged,
  - [ ] marker meanings are unchanged,
  - [ ] reject behavior remains fail-closed,
  - [ ] no `TASK-0024` feature work leaked into the refactor.
- [ ] Proof is rerun after each major extraction cut, not only at final closeout:
  - [ ] `cargo test -p dsoftbusd -- --nocapture`,
  - [ ] `just test-dsoftbus-quic`,
  - [ ] `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`.
- [ ] `main.rs` minimality is enforced in the resulting shape:
  - [ ] no subsystem-specific helper logic remains in `main.rs`,
  - [ ] no parser/encoder/decoder remains in `main.rs`,
  - [ ] no retry/deadline/reply-matching loops remain in `main.rs`,
  - [ ] no service-specific marker branching remains in `main.rs`.
- [ ] Marker honesty remains explicit:
  - [ ] success markers are emitted only after verified behavior/state,
  - [ ] if logic bugs or fake-success markers are found, they are fixed rather than preserved,
  - [ ] dishonest markers are converted into honest behavior/proof markers before closure.
- [ ] Architecture contract stays synchronized:
  - [ ] `TASK-0023B` remains execution SSOT,
  - [ ] `RFC-0038` remains architecture/contract seed,
  - [ ] queue order still keeps `TASK-0024` after `TASK-0023B`.

## Task-0029 manual addendum (when applicable)
- [ ] Production-grade BASELINE scope respected — no scope creep into `TASK-0197 / 0198 / 0289`. The 10 hard gates from TASK-0029 §"Production-grade tier" are mechanically reviewed at every cut:
  - [ ] no sigchain envelope around `(manifest, sbom, repro, signature)`,
  - [ ] no transparency / Merkle translog,
  - [ ] no SLSA-style provenance records,
  - [ ] no anti-downgrade / rollback indices,
  - [ ] no `updated/` or `storemgrd/` install-path changes,
  - [ ] no boot anchor / measured-boot work,
  - [ ] schema-extensibility ratchet preserved on the `keystored` capnp bump (`@N` IDs + reserved gap; `reason: Text`; `alg: Text`),
  - [ ] `meta/` layout-extensibility ratchet preserved (v2 can append `meta/sigchain.nxb` without re-pack),
  - [ ] format-policy compliance (SBOM = CycloneDX JSON; no parallel TOML/YAML/protobuf carrier),
  - [ ] no marker drift (stable-label markers; publisher/key/fingerprint values to logd only).
- [ ] Single-allowlist-authority is preserved — only `keystored` answers "is this publisher+alg+key allowed". `bundlemgrd` and `policyd` carry zero parallel allowlist logic.
- [ ] Identity is channel-bound: `bundlemgrd` derives caller identity from `sender_service_id`, never from a payload string.
- [ ] Audit-or-fail invariant: every install-time allow/deny decision emits a logd audit event. If logd is unreachable, install fails closed.
- [ ] `bundlemgrd` install path enforces the contract order: verify (`keystored::verify`) → policy (`policyd` → `keystored::is_key_allowed`) → payload digest match → audit emit. Any failure → reject with stable error label + audit event + deterministic deny marker.
- [ ] Determinism floor: `SOURCE_DATE_EPOCH` everywhere; reuse `nexus-evidence` reproducible-tar primitives; two consecutive packs of the same inputs are byte-identical (proven by host test).
- [ ] `nexus-evidence::scan` deny-by-default secret scanner runs before pack — PEM blocks, `*PRIVATE_KEY*=…` env-style strings, ≥64-char base64 high-entropy blobs refuse to seal.
- [ ] Bounded inputs — explicit size caps on SBOM JSON, repro metadata, bundle entries; reject before parsing if exceeded.
- [ ] Required host reject suite is green (TASK-0029 §"Reject-path proof (host)"):
  - [ ] `test_reject_unknown_publisher` → `policy.publisher_unknown`,
  - [ ] `test_reject_unknown_key` → `policy.key_unknown`,
  - [ ] `test_reject_unsupported_alg` → `policy.alg_unsupported`,
  - [ ] `test_reject_payload_digest_mismatch` → `integrity.payload_digest_mismatch`,
  - [ ] `test_reject_sbom_secret_leak` → pack refuses,
  - [ ] `test_reject_repro_schema_invalid` → `repro-verify` rejects,
  - [ ] `test_reject_audit_unreachable` → `bundlemgrd` install fails closed.
- [ ] QEMU markers (gated, only when OS install path is wired) registered in `source/apps/selftest-client/proof-manifest/markers/` and attached to a profile under `proof-manifest/profiles/`; `verify-uart` deny-by-default enforces the ladder. Stable labels only; no variable data in the marker string:
  - [ ] `bundlemgrd: sign policy allow ok`,
  - [ ] `bundlemgrd: sign policy deny ok`,
  - [ ] `SELFTEST: sign policy allow ok`,
  - [ ] `SELFTEST: sign policy deny ok`,
  - [ ] `SELFTEST: sign policy unknown publisher rejected ok`,
  - [ ] `SELFTEST: sign policy unknown key rejected ok`,
  - [ ] `SELFTEST: sign policy payload tamper rejected ok`.
- [ ] Any QEMU evidence bundle produced for the task seals cleanly under both `--policy=bringup` and `--policy=ci` (reuses TASK-0023B Phase-5 pipeline).
- [ ] `tools/nexus-idl/schemas/keystored.capnp` ABI diff is reviewed as part of task review (CAUTION zone per `.cursorrules`); reserved-field gaps (`@3..@7` request, `@2..@5` response) documented inline in the schema.
- [ ] `nexus-evidence/` stayed READ-ONLY (no API change in v1).
- [ ] No kernel changes.
- [ ] `just dep-gate && just diag-os && just diag-host && just fmt-check && just lint && just arch-gate` green.

## Task-0031 manual addendum (when applicable)
- [ ] Scope honesty is preserved:
  - [ ] `TASK-0031` remains plumbing/honesty floor (no kernel redesign scope creep),
  - [ ] production-grade closeout remains explicit in `TASK-0290`,
  - [ ] no early "production-grade" claim before closure proofs.
- [ ] Contract seed alignment is current:
  - [ ] `RFC-0040` exists and is linked from `TASK-0031`,
  - [ ] RFC includes normative production-grade requirement aligned with `TRACK-PRODUCTION-GATES-KERNEL-SERVICES`,
  - [ ] Gate-A/Gate-C relevant zero-copy obligations are explicitly mapped.
- [ ] Behavior-first proof shape is explicit:
  - [ ] target behavior is stated in 1-3 lines,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest,
  - [ ] secondary proof only closes a real blind spot.
- [ ] Security and reject discipline are explicit and tested:
  - [ ] unauthorized transfer reject,
  - [ ] invalid/oversized mapping reject,
  - [ ] rights mismatch reject,
  - [ ] no fake success markers for deny/degraded paths.
- [ ] Rust discipline is reviewed where relevant:
  - [ ] `newtype` candidates for VMO/cap IDs are explicit,
  - [ ] ownership/lifetime semantics for mappings are explicit,
  - [ ] `#[must_use]` is used for decision-bearing results where sensible,
  - [ ] `Send`/`Sync` assumptions are justified without unsafe blanket shortcuts.
- [ ] Deterministic marker contract is proven in OS/QEMU path:
  - [ ] `vmo: producer sent handle`,
  - [ ] `vmo: consumer mapped ok`,
  - [ ] `vmo: sha256 ok`,
  - [ ] `SELFTEST: vmo share ok`.
- [ ] Production closure route remains explicit:
  - [ ] `TASK-0290` stays linked as mandatory closure step,
  - [ ] RFC/task text states "Complete only after production-grade closure obligations are green".

## Task-0032 manual addendum (when applicable)
- [ ] Scope honesty is preserved:
  - [ ] `TASK-0032` stays image/index fastpath scope (no kernel scope creep),
  - [ ] VMO splice scope remains in `TASK-0033`,
  - [ ] production dependencies (`TASK-0286/0287/0290`) stay explicit and unabsorbed.
- [ ] Contract seed alignment is current:
  - [ ] `RFC-0041` exists and is linked from `TASK-0032`,
  - [ ] Gate-C production mapping is explicit and consistent with `TRACK-PRODUCTION-GATES-KERNEL-SERVICES`.
- [ ] Behavior-first proof shape is explicit:
  - [ ] target behavior is stated in 1-3 lines,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and honest,
  - [ ] secondary proof only closes a real blind spot.
- [ ] Security and reject discipline are explicit and tested:
  - [ ] bad magic/version reject,
  - [ ] index-hash mismatch reject,
  - [ ] out-of-range entry reject,
  - [ ] path traversal reject,
  - [ ] index-cap reject.
- [ ] Deterministic marker contract is proven when OS path is claimed:
  - [ ] `packagefsd: v2 mounted (pkgimg)`,
  - [ ] `SELFTEST: pkgimg mount ok`,
  - [ ] `SELFTEST: pkgimg stat/read ok`.
- [ ] No fake-success fallback:
  - [ ] mount success marker only after real validation + index load,
  - [ ] no silent fallback to unvalidated data path.

## Task-0039 manual addendum (when applicable)
- [ ] Scope honesty is preserved:
  - [ ] kernel remains untouched in v1 sandbox scope,
  - [ ] userspace confinement boundary is explicitly documented (no kernel-enforced claims),
  - [ ] follow-up hardening scope stays explicit (`TASK-0043`, `TASK-0189`).
- [ ] Contract seed alignment is current:
  - [ ] `RFC-0042` exists and is linked from `TASK-0039`,
  - [ ] Gate-B production mapping is explicit and consistent with `TRACK-PRODUCTION-GATES-KERNEL-SERVICES`.
- [ ] Behavior-first proof shape is explicit:
  - [ ] target behavior is stated in 1-3 lines,
  - [ ] main break point is explicit,
  - [ ] primary proof validates Soll behavior (not implementation coupling),
  - [ ] secondary proof only closes a real blind spot.
- [ ] Security and reject discipline are explicit and tested:
  - [ ] traversal escape reject,
  - [ ] forged/replayed CapFd reject,
  - [ ] unauthorized path reject,
  - [ ] direct-cap bypass prevention at spawn-time authority boundary.
- [ ] Rust API discipline is reviewed where safety-relevant:
  - [ ] newtypes for identity-bearing handles where boundaries cross components,
  - [ ] `#[must_use]` on decision-bearing results where useful,
  - [ ] ownership and revocation semantics are explicit,
  - [ ] `Send`/`Sync` assumptions reviewed without unsafe blanket shortcuts.
- [ ] Deterministic marker contract is stable-label only when OS path is claimed:
  - [ ] `vfsd: namespace ready`
  - [ ] `vfsd: capfd grant ok`
  - [ ] `vfsd: access denied`
  - [ ] `SELFTEST: sandbox deny ok`
  - [ ] `SELFTEST: capfd read ok`
- [ ] Gap closure quality floor (anti fake-green):
  - [ ] OS gate run recorded with marker evidence (`RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`),
  - [ ] CapFd reject proof includes at least one service-path assertion (not helper-only),
  - [ ] TASK/RFC checklists are synced only after evidence exists.

## Task-0045 manual addendum (when applicable)
- [ ] Scope and gate tier are explicit:
  - [ ] `TASK-0045` is host-first tooling scope (no OS runtime closure claim),
  - [ ] Gate-J `production-floor` is explicit (not production-grade).
- [ ] Contract seed alignment is current:
  - [ ] `RFC-0043` exists and is linked from `TASK-0045`,
  - [ ] v1 command surface + exit-code contract stay synchronized with implementation.
- [ ] Security fail-closed behavior is proven with reject tests:
  - [ ] scaffolding rejects traversal and absolute path writes,
  - [ ] `postflight` rejects unknown topics,
  - [ ] delegated failures are propagated (no fake success),
  - [ ] no shell-command construction from user-controlled strings.
- [ ] Proof quality is Soll-behavior focused:
  - [ ] tests assert exit-code classes and structured output fields,
  - [ ] tests assert concrete filesystem effects for scaffolding,
  - [ ] no closure based on marker/log grep only.
- [ ] Extension/no-drift contract is preserved:
  - [ ] subcommand architecture is canonical under `tools/nx`,
  - [ ] no new standalone `nx-*` binaries are introduced for follow-ups.
- [ ] `nx dsl fmt|lint|build` floor is honest:
  - [ ] delegates deterministically when backend exists,
  - [ ] fails closed as unsupported when backend is absent,
  - [ ] never prints success on delegate non-zero.

## Active progress snapshot (TASK-0032 closure, 2026-04-23)
- [x] `TASK-0032` execution SSOT is synchronized to landed implementation and proof evidence (`Done`).
- [x] `RFC-0041` contract seed is synchronized to closure state (`Done`).
- [x] Required TASK-0032 reject suite is green (`test_reject_pkgimg_*`).
- [x] Required TASK-0032 marker ladder is proven in single-VM QEMU path.
- [x] Build hygiene gates are green for touched OS/host code (`diag-host`, `dep-gate`, `diag-os`).

## Active progress snapshot (TASK-0045 kickoff, 2026-04-24)
- [x] Active SSOT switched to `TASK-0045`.
- [x] Contract seed `RFC-0043` is done and linked from task/index while `TASK-0045` remains `In Review`.
- [x] Follow-up tasks are populated in `TASK-0045` header.
- [x] Security section and anti-fake-success proof criteria are explicit in task text.
- [x] Gate-tier alignment clarified as Gate J `production-floor`.
- [x] `tools/nx` implementation is in-tree with canonical subcommand dispatch.
- [x] Host proof suite for `nx` command families is green (`cargo test -p nx -- --nocapture`).
- [x] TASK-0045 closure deltas resolved:
  - [x] `--json` reject/error paths return structured output instead of plain text.
  - [x] process-level CLI tests assert exit code classes + structured output fields.
  - [x] scaffolding header expectation (CONTEXT) is explicitly satisfied.

## Legacy progress snapshot (TASK-0039 execution, 2026-04-24)
- [x] Host reject floor implemented and green:
  - [x] traversal / unauthorized namespace rejects,
  - [x] forged / replayed / rights-mismatch CapFd rejects,
  - [x] spawn direct-fs-cap bypass reject.
- [x] OS-gated stable markers are wired in manifest/harness surfaces.
- [x] Security/testing docs synced to current proof shape.
- [x] Full OS marker proof run recorded for this cut (`RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`).
- [x] Kernel blocker fixes validated in the same gate path (POOL overlap + heap OOM resolved).
- [x] Final TASK/RFC closure status flip completed on task/rfc/status board index surfaces.
- [x] Post-closure hardening deltas re-proven:
  - [x] runtime spawn fs-cap boundary check is active in `execd` os-lite path,
  - [x] vfsd os-lite handle ownership uses `sender_service_id` for read/close.

## Legacy progress snapshot (TASK-0031 prep alignment, 2026-04-21)
- [x] Active SSOT switched to `TASK-0031`.
- [x] New contract seed `RFC-0040` created and linked.
- [x] RFC now includes normative production-grade requirement and explicit closure routing via `TASK-0290`.
- [x] `.cursor` workfiles moved from legacy `TASK-0029` posture to `TASK-0031` prep posture.
- [x] Context bundle entries for `@task_0031_context` and `@task_0031_touched` added.

## Legacy progress snapshot (TASK-0029 closure remediation, 2026-04-22)
- [x] Open questions pinned in task + RFC; execution plan authored and implemented through C-08.
- [x] Host proofs + gated QEMU `supply-chain` profile run are green.
- [x] `.cursor` workfiles switched from kickoff posture to closure-remediation posture.
- [x] Contract drift deltas closed: manifest `sbomDigest`/`reproDigest`, strict `policyd` authority boundary, `sender_service_id` enforcement, bounded size guards.
- [x] Final quality gate line fully green: `dep-gate`, `diag-os`, `diag-host`, `fmt-check`, `lint`, `arch-gate`.
- [x] Explicit `cyclonedx-cli` roundtrip proof captured for RFC-0039 Phase 0.
- [x] Status sync applied: `RFC-0039` is `Done`; `TASK-0029` is `Done`.

## Carry-over (TASK-0023B Phase-6 environmental closure)
- [x] All six `TASK-0023B` phases functionally closed (P1-P6); `RFC-0038` advanced from `Draft` to `Done`; `TASK-0023B` advanced from `Draft` to `In Review`.
- [ ] External CI-runner replay artifact for P6-05 captured (recipe in `docs/testing/replay-and-bisect.md` §7-§8). After capture: flip `TASK-0023B` to `Done`, tick RFC-0038 Phase-6 checkbox, sync STATUS-BOARD / IMPLEMENTATION-ORDER. Independent of TASK-0029 execution.

## Legacy manual profiles (reference only)
- [ ] TASK-0019 closeout checks are archived and tracked in task-local evidence (`Done`).
- [ ] TASK-0018 crashdump closeout checks are tracked in archived handoff and task-local evidence (`Done`).

## Post-implementation (before claiming "Done")
- [ ] Task doc still matches reality (status, proofs, touched paths)
- [ ] Proof commands and evidence are mirrored in handoff/task sections
- [ ] Header blocks updated (CONTEXT, TEST_COVERAGE, ADR links) where code was touched
- [ ] Docs synced only where contract/proof surfaces changed
- [ ] `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/context_bundles.md`, `.cursor/next_task_prep.md`, `.cursor/stop_conditions.md` updated in same slice
