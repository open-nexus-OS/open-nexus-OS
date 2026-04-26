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

## TASK-0046 class stop conditions (Config v1: configd + schemas + layering + 2PC + nx config)
- [ ] Execution SSOT + contract seed are synchronized:
  - [ ] `TASK-0046` status/proof/touched-paths reflect real repo state,
  - [ ] `RFC-0044` remains linked and contract-aligned.
- [ ] Canonical format authority is enforced:
  - [ ] canonical runtime/persistence effective snapshot is Cap'n Proto,
  - [ ] JSON remains authoring/validation and derived CLI/debug view only.
- [ ] Deterministic layering and validation behavior is proven:
  - [ ] precedence `defaults < /system < /state < env` is stable,
  - [ ] unknown/type/depth/size rejects are fail-closed with stable non-zero classification.
- [ ] 2PC reload semantics are proven and honest:
  - [ ] prepare reject/timeout triggers abort,
  - [ ] previous effective version remains active after abort,
  - [ ] partial-commit is not observable as success.
- [ ] Marker honesty contract is enforced:
  - [ ] marker-only evidence is not used for closure,
  - [ ] marker outcomes are paired with deterministic state/result assertions and must agree.
- [ ] Required host proof floor is green:
  - [ ] `cargo test -p nexus-config -- --nocapture`
  - [ ] `cargo test -p configd -- --nocapture`
  - [ ] `cargo test -p nx -- --nocapture`
- [ ] `nx` no-drift CLI authority is preserved:
  - [ ] config UX remains under `nx config ...`,
  - [ ] no `nx-config` logic fork or parallel CLI authority exists.
- [ ] Follow-up contract hand-off remains explicit and unbroken:
  - [ ] `TASK-0047`, `TASK-0262`, `TASK-0266`, `TASK-0268`, `TASK-0273`, `TASK-0285`.

## TASK-0047 class stop conditions (Policy as Code v1 unified engine + `nx policy`)
- [ ] Execution SSOT + contract seed are synchronized:
  - [ ] `TASK-0047` status/proof/touched-paths reflect real repo state,
  - [ ] `RFC-0045` remains linked and contract-aligned.
- [ ] Single-authority policy boundary is enforced:
  - [ ] `policyd` remains the decision authority,
  - [ ] no second live policy root remains after migration,
  - [ ] no parallel policy daemon/compiler authority exists.
- [ ] Config-v1 carry-in boundary is enforced:
  - [ ] reload/version transitions flow through `configd`,
  - [ ] previous policy version remains active after reject/timeout/failed apply.
- [ ] Deterministic evaluator behavior is proven:
  - [ ] equivalent policy inputs yield the same canonical version/hash,
  - [ ] explain traces are bounded and deterministic,
  - [ ] invalid/oversize/ambiguous inputs fail closed with stable reject classes.
- [ ] Learn/dry-run lifecycle is honest:
  - [ ] dry-run never grants what enforce would deny,
  - [ ] learn mode is bounded and audited,
  - [ ] unauthenticated/stale mode transitions are rejected.
- [ ] Phase 0 `nx` no-drift refactor is complete and honest:
  - [ ] `tools/nx` structure is split as planned,
  - [ ] current CLI behavior remains unchanged during the refactor,
  - [ ] `nx policy` lives under `tools/nx` with no `nx-*` fork.
- [ ] Behavior-first proof floor is green:
  - [ ] `cargo test -p policy -- --nocapture`
  - [ ] `cargo test -p policyd -- --nocapture`
  - [ ] `cargo test -p nx -- --nocapture`
  - [ ] required `test_reject_*` suites are green,
  - [ ] at least one migrated adapter parity proof is green before cutover claims.
- [ ] Marker honesty contract is enforced:
  - [ ] marker-only evidence is not used for closure,
  - [ ] later OS/QEMU markers, if claimed, are paired with deterministic state/result assertions.

## Active progress snapshot (TASK-0047 in progress, 2026-04-26)
- [x] `TASK-0046` and `RFC-0044` are synchronized to `Done`.
- [x] `TASK-0047` and `RFC-0045` are linked as the new execution+contract pair.
- [x] Gate B framing, security section, red flags, and behavior-first proof expectations are present in `TASK-0047`.
- [x] Phase 0 `tools/nx` structure refactor is explicitly scoped in task + RFC.
- [x] Queue/docs/workfiles are synchronized to the new prep state.
- [ ] Host proof floor for Policy as Code is not yet claimed.
- [ ] OS/QEMU policy closure remains gated and unclaimed.

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

## TASK-0031 class stop conditions (zero-copy VMOs v1 plumbing)
- [ ] Execution SSOT + contract seed are synchronized:
  - [ ] `TASK-0031` status/proof/touched-paths reflect real repo state,
  - [ ] `RFC-0040` remains linked and contract-aligned,
  - [ ] `TASK-0290` is explicitly referenced as production closure route.
- [ ] Behavior-first proof shape is explicit and honest:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and contract-driven,
  - [ ] secondary proof only closes real blind spots.
- [ ] Security and deny-by-default behavior are proven:
  - [ ] unauthorized transfer attempts are rejected,
  - [ ] invalid/oversized map attempts are rejected,
  - [ ] rights mismatches fail closed,
  - [ ] no success marker is emitted for deny/degraded paths.
- [ ] Rust API discipline is reviewed where safety-relevant:
  - [ ] `newtype` usage for VMO/capability IDs is explicit where sensible,
  - [ ] mapping/handle ownership and lifetime semantics are explicit,
  - [ ] `#[must_use]` applied to decision-bearing outcomes where useful,
  - [ ] `Send`/`Sync` assumptions are documented and reviewed without unsafe blanket shortcuts.
- [ ] Deterministic OS marker proof is green (when OS proof path is claimed):
  - [ ] `vmo: producer sent handle`
  - [ ] `vmo: consumer mapped ok`
  - [ ] `vmo: sha256 ok`
  - [ ] `SELFTEST: vmo share ok`
- [ ] Production-grade requirement remains enforceable:
  - [ ] RFC text states completion depends on production-grade closure obligations,
  - [ ] early production-grade claims are blocked until `TASK-0290` closure proofs are green,
  - [ ] track-gate alignment remains explicit (`TRACK-PRODUCTION-GATES-KERNEL-SERVICES`).

## TASK-0032 class stop conditions (PackageFS v2 RO image index fastpath)
- [ ] Execution SSOT + contract seed are synchronized:
  - [ ] `TASK-0032` status/proof/touched-paths reflect real repo state,
  - [ ] `RFC-0041` remains linked and contract-aligned,
  - [ ] follow-up routes stay explicit (`TASK-0033`, `TASK-0286`, `TASK-0287`, `TASK-0290`).
- [ ] Behavior-first proof shape is explicit and honest:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof is minimal and contract-driven,
  - [ ] secondary proof only closes real blind spots.
- [ ] Security and fail-closed behavior are proven:
  - [ ] malformed magic/version reject,
  - [ ] index hash mismatch reject,
  - [ ] out-of-range index entry reject,
  - [ ] path traversal/invalid path reject,
  - [ ] parser bounds-cap reject.
- [ ] Deterministic host proof is green:
  - [ ] deterministic image build + verify proof,
  - [ ] stat/open/read contract proof against fixture image,
  - [ ] required `test_reject_*` suite is green.
- [ ] Deterministic OS marker proof is green (when OS proof path is claimed):
  - [ ] `packagefsd: v2 mounted (pkgimg)`
  - [ ] `SELFTEST: pkgimg mount ok`
  - [ ] `SELFTEST: pkgimg stat/read ok`
- [ ] Production-grade dependency split remains explicit:
  - [ ] zero-copy splice remains `TASK-0033` scope,
  - [ ] kernel closure obligations remain in `TASK-0286/0287/0290`,
  - [ ] no hidden scope absorption into `TASK-0032`.

## TASK-0039 class stop conditions (Sandboxing v1 userspace confinement)
- [ ] Execution SSOT + contract seed are synchronized:
  - [ ] `TASK-0039` status/proof/touched-paths reflect real repo state,
  - [ ] `RFC-0042` remains linked and contract-aligned,
  - [ ] follow-up routes stay explicit (`TASK-0043`, `TASK-0189`).
- [ ] Behavior-first proof shape is explicit and honest:
  - [ ] target behavior is explicit,
  - [ ] main break point is explicit,
  - [ ] primary proof validates Soll behavior (not implementation internals),
  - [ ] secondary proof only closes real blind spots.
- [ ] Security and fail-closed behavior are proven:
  - [ ] traversal escape rejects,
  - [ ] forged/replayed CapFd rejects,
  - [ ] unauthorized namespace path rejects,
  - [ ] capability-distribution boundary enforces no direct fs-service caps to app subjects.
- [ ] Rust API discipline is reviewed where safety-relevant:
  - [ ] newtype usage for subject/namespace/capability identifiers where useful,
  - [ ] `#[must_use]` on decision-bearing outcomes where useful,
  - [ ] ownership + revocation semantics remain explicit,
  - [ ] `Send`/`Sync` expectations are reviewed without unsafe blanket shortcuts.
- [ ] Deterministic host proof is green:
  - [ ] required `test_reject_*` suites are green for traversal/forgery/replay/unauthorized path.
- [ ] Deterministic OS marker proof is green (when OS proof path is claimed):
  - [ ] `vfsd: namespace ready`
  - [ ] `vfsd: capfd grant ok`
  - [ ] `vfsd: access denied`
  - [ ] `SELFTEST: sandbox deny ok`
  - [ ] `SELFTEST: capfd read ok`
- [ ] Production-grade scope honesty remains explicit:
  - [ ] no kernel-enforced sandbox claim in v1 scope,
  - [ ] follow-up hardening remains routed to `TASK-0043` / `TASK-0189`.
- [ ] Gap-proof quality floor is met:
  - [ ] OS marker gate is executed and archived for this cut,
  - [ ] reject proofs include service-path behavior checks (not helper-only only),
  - [ ] no closure claim before TASK/RFC checklist synchronization.

## TASK-0045 class stop conditions (DevX nx CLI v1)
- [ ] Execution SSOT + contract seed are synchronized:
  - [ ] `TASK-0045` status/proof/touched-paths reflect real repo state,
  - [ ] `RFC-0043` remains linked and contract-aligned,
  - [ ] follow-up routes stay explicit (`TASK-0046`, `TASK-0047`, `TASK-0048`, `TASK-0163`, `TASK-0164`, `TASK-0165`, `TASK-0227`, `TASK-0230`, `TASK-0268`).
- [ ] Gate-tier scope honesty is preserved:
  - [ ] Gate J `production-floor` alignment stays explicit,
  - [ ] no production-grade claim for this task.
- [ ] Canonical CLI path is implemented:
  - [ ] `tools/nx` exists as single entrypoint for v1 command families,
  - [ ] no parallel `nx-*` binary drift introduced.
- [ ] Security and fail-closed behavior are proven:
  - [ ] scaffolding rejects traversal and absolute write targets,
  - [ ] `postflight` uses allowlist topic mapping and rejects unknown topics,
  - [ ] no user-string-driven shell command construction,
  - [ ] delegated non-zero exits propagate as non-success.
- [ ] Deterministic contract is proven:
  - [ ] stable exit-code classes (`0/2/3/4/5/6/7`) are covered by tests,
  - [ ] `--json` outputs are stable and asserted in tests,
  - [ ] output tails/diagnostics are bounded.
- [ ] Proof quality floor is met:
  - [ ] required host tests are green (`cargo test -p nx -- --nocapture`),
  - [ ] reject-path tests exist per command family (`new`, `postflight`, `doctor`),
  - [ ] closure evidence is based on exit codes + structured outputs + file effects (not grep-only logs).
- [ ] Scope boundary stays explicit:
  - [ ] no ownership absorption of config/policy/crash/sdk semantics from follow-up tasks.

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

## TASK-0029 class stop conditions (Supply-Chain v1: bundle SBOM + repro + signature allowlist policy)
- [ ] `production-grade` BASELINE scope is preserved — none of the 10 hard gates from TASK-0029 §"Production-grade tier" was crossed:
  - [ ] no sigchain envelope around `(manifest, sbom, repro, signature)` (owned by `TASK-0197`),
  - [ ] no transparency / Merkle translog (owned by `TASK-0197`),
  - [ ] no SLSA-style provenance records (owned by `TASK-0197`),
  - [ ] no anti-downgrade / rollback indices (owned by `TASK-0198 + TASK-0289`),
  - [ ] no `updated/` or `storemgrd/` install-path changes (owned by `TASK-0198`),
  - [ ] no boot-anchor / measured-boot work (owned by `TASK-0289`),
  - [ ] schema-extensibility ratchet preserved on `keystored::IsKeyAllowed*` (`@N` IDs + reserved gap, `reason: Text` open set, `alg: Text` open set),
  - [ ] `meta/` layout-extensibility ratchet preserved (v2 can append `meta/sigchain.nxb` without re-pack),
  - [ ] format-policy compliance (SBOM = CycloneDX JSON; no parallel TOML/YAML/protobuf carrier),
  - [ ] no marker drift (stable-label markers; publisher/key/fingerprint values to logd only).
- [ ] Single allowlist authority is preserved: only `keystored` answers "is this publisher+alg+key allowed". `bundlemgrd` and `policyd` carry zero parallel allowlist logic.
- [ ] Identity is channel-bound: `bundlemgrd` derives caller identity from `sender_service_id`, never from a payload string.
- [ ] Audit-or-fail invariant: every install-time allow/deny decision emits a logd audit event; if logd is unreachable, install fails closed (no silent allow).
- [ ] `bundlemgrd` install path enforces the contract order: signature verify (`keystored::verify`) → policy decision (`policyd` → `keystored::is_key_allowed`) → payload digest match → audit emit. Any failure → reject with stable error label + audit event + deterministic deny marker.
- [ ] Determinism floor: `SOURCE_DATE_EPOCH` only; reuse `nexus-evidence` reproducible-tar primitives; two consecutive packs of the same inputs are byte-identical (proven by host test).
- [ ] `nexus-evidence::scan` deny-by-default secret scanner runs before pack: PEM blocks, `*PRIVATE_KEY*=…` env-style strings, ≥64-char base64 high-entropy blobs refuse to seal.
- [ ] Bounded inputs: explicit size caps on SBOM JSON, repro metadata, bundle entries; reject before parsing if exceeded.
- [ ] Required host reject suite is green:
  - [ ] `test_reject_unknown_publisher` → stable `policy.publisher_unknown`,
  - [ ] `test_reject_unknown_key` → stable `policy.key_unknown`,
  - [ ] `test_reject_unsupported_alg` → stable `policy.alg_unsupported`,
  - [ ] `test_reject_payload_digest_mismatch` → stable `integrity.payload_digest_mismatch`,
  - [ ] `test_reject_sbom_secret_leak` → pack refuses,
  - [ ] `test_reject_repro_schema_invalid` → `repro-verify` rejects,
  - [ ] `test_reject_audit_unreachable` → `bundlemgrd` install fails closed.
- [ ] QEMU markers (gated; only when OS install path is wired) are registered in `source/apps/selftest-client/proof-manifest/markers/` and attached to a profile under `proof-manifest/profiles/`. `verify-uart` deny-by-default enforces the ladder. Stable labels only:
  - [ ] `bundlemgrd: sign policy allow ok`,
  - [ ] `bundlemgrd: sign policy deny ok`,
  - [ ] `SELFTEST: sign policy allow ok`,
  - [ ] `SELFTEST: sign policy deny ok`,
  - [ ] `SELFTEST: sign policy unknown publisher rejected ok`,
  - [ ] `SELFTEST: sign policy unknown key rejected ok`,
  - [ ] `SELFTEST: sign policy payload tamper rejected ok`.
- [ ] Any QEMU evidence bundle produced for this task seals cleanly under both `--policy=bringup` and `--policy=ci` (reuses TASK-0023B Phase-5 pipeline).
- [ ] `tools/nexus-idl/schemas/keystored.capnp` ABI diff is reviewed as part of task review; reserved-field gaps (`@3..@7` request, `@2..@5` response) documented inline in the schema.
- [ ] `nexus-evidence/` remained READ-ONLY (no API change in v1).
- [ ] No kernel changes; `just dep-gate && just diag-os && just diag-host && just fmt-check && just lint && just arch-gate` green; no new forbidden crates in OS graph.
- [ ] No follow-up scope (`TASK-0197 / 0198 / 0289`) silently absorbed; touched-paths allowlist enforced.
- [ ] Architecture contract stays synchronized: `tasks/TASK-0029-...md` (execution SSOT), `docs/rfcs/RFC-0039-...md` (contract seed). Both flipped from `Draft` to `Ready` before code lands; `Done` only after all proofs above are green and `docs/rfcs/README.md` index entry reflects closure.

## Active progress snapshot (TASK-0032 closure alignment, 2026-04-23)
- [x] `TASK-0032` status is `Done` with synchronized proof evidence.
- [x] `RFC-0041` status is `Done` with checklist/proof sync.
- [x] Follow-up routing and Gate-C production dependencies remain explicit (`TASK-0033`, `TASK-0286`, `TASK-0287`, `TASK-0290`).
- [x] `.cursor` workfiles updated from prep posture to post-closure posture.

## Active progress snapshot (TASK-0045 kickoff alignment, 2026-04-24)
- [x] Active SSOT switched to `TASK-0045`.
- [x] `RFC-0043` contract is done and linked while `TASK-0045` remains `In Review`.
- [x] Task header follow-up list and red-flag resolutions are synchronized.
- [x] Security section and proof-quality anti-fake-success requirements are explicit.
- [x] `tools/nx` implementation is present in-tree.
- [x] Host proof suite is green for v1 command and reject contracts.
- [x] TASK-0045 proof-quality deltas are closed before claiming final Done:
  - [x] `--json` error/reject paths are structured and deterministic at process boundary.
  - [x] CLI-level tests verify exit-code classes + structured outputs (not only internal handler return values).
  - [x] scaffolding output contract (including header expectations) is fully aligned with task text.

## Legacy progress snapshot (TASK-0039 execution alignment, 2026-04-24)
- [x] `TASK-0039` status synchronized to `Done`.
- [x] `RFC-0042` status synchronized to `Done`.
- [x] Host reject proof floor is green for traversal/capfd/spawn-boundary checks.
- [x] Stable marker contract strings are wired in selftest/harness surfaces.
- [x] OS marker proof path is green and archived for this cut.
- [x] Kernel bring-up blockers addressed without scope absorption (overlap/heap fixes + typed layout window).
- [x] Status/index synchronization complete (`TASK-0039`, `RFC-0042`, STATUS-BOARD, RFC index).
- [x] Post-closure hardening pass complete without scope drift (`TASK-0043`/`TASK-0189` remain explicit follow-up).

## Legacy progress snapshot (TASK-0029 closure remediation, 2026-04-22)
- [x] Active `.cursor` workfiles and RFC status sections now reflect post-implementation closure state.
- [x] C-01..C-08 implementation slices are landed; host reject suite and QEMU supply-chain profile proofs exist.
- [x] Remaining closure deltas are fixed in code: manifest digest fields + strict authority boundary + sender identity + bounded inputs.
- [x] Final quality gates (`fmt-check`/`lint` + full gate set) are green and referenced from task/RFC closure notes.
- [x] Explicit `cyclonedx-cli` roundtrip proof for RFC-0039 Phase 0 captured and documented.
- [x] Status sync applied: `RFC-0039` is `Done`; `TASK-0029` is `Done`.

## Carry-over (TASK-0023B Phase-6 environmental closure)
- [x] `TASK-0023B` is `In Review`; `RFC-0038` advanced to `Done` modulo Phase-6 checkbox.
- [x] Phase-6 proof-floor evidence stored in `.cursor/replay-{dev-a,ci-like,synthetic-bad}.json` + `.cursor/bisect-good-drift-regress.json`.
- [ ] External CI-runner replay artifact for P6-05 captured + status flip applied per `docs/testing/replay-and-bisect.md` §7-§8. After capture: archive `.cursor/replay-ci.{json,log}`, tick RFC-0038 Phase-6 box, flip TASK-0023B to `Done`, sync STATUS-BOARD / IMPLEMENTATION-ORDER. Independent of TASK-0029 execution.
