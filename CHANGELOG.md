# Changelog

All notable changes to Open Nexus OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Changed - 2026-04-27

#### TASK-0054 / RFC-0046 host renderer closure (`TASK-0054`, `RFC-0046`)

- Closed the narrow host-first UI renderer proof floor and RFC contract:
  - `userspace/ui/renderer` provides a safe Rust BGRA8888 `Frame`, checked dimensions/stride/damage newtypes,
    deterministic clear/rect/rounded-rect/blit/text primitives, and bounded full-frame damage overflow behavior
  - `userspace/ui/fonts` provides the repo-owned deterministic fixture font; no host font discovery or locale fallback
  - `tests/ui_host_snap` proves expected pixels, full rounded-rect/text masks, damage behavior, snapshot/golden
    comparison, PNG metadata independence, golden update gating, artifact path confinement, anti-fake-marker source
    scanning, and required reject classes
- Added host proof coverage:
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture`
  - `cargo test -p ui_host_snap reject -- --nocapture`
  - `just diag-host`
  - `just test-all`
  - `just ci-network`
  - `scripts/fmt-clippy-deny.sh`
  - `make clean`, `make build`, `make test`, `make run`
- Synchronized `TASK-0054` to `Done`, `RFC-0046` to `Done`, RFC index, status board, implementation order, and UI testing docs.
- OS/QEMU present markers, compositor/windowd wiring, GPU/device paths, and Gate A kernel/core production-grade claims remain out of scope.

### Changed - 2026-04-26

#### TASK-0047 / RFC-0045 host-first closure (`TASK-0047`, `RFC-0045`)

- Closed the Policy as Code v1 host-first contract floor:
  - active policy root is now `policies/nexus.policy.toml`
  - `recipes/policy/` is legacy documentation only, not a live TOML authority
  - `userspace/policy` provides deterministic `PolicyVersion`, bounded evaluator traces, and stable reject classes
  - Config v1 carries policy candidate roots as `policy.root`
  - `policies/manifest.json` records the deterministic tree hash and validates fail-closed when missing or stale
  - `policyd` stages configd-fed `PolicyTree` candidates through `configd::ConfigConsumer` and rejects stale/unauthorized lifecycle changes
  - external `policyd` host frame operations for `Version`, `Eval`, `ModeGet`, and `ModeSet` are backed by `PolicyAuthority` and bounded audit events
  - the `policyd` service-facing check frame evaluates through the unified authority
  - `nx policy` lives under `tools/nx` with deterministic JSON/exit contracts; `nx policy mode` is explicit host preflight only
- Added host proof coverage:
  - `cargo test -p policy -- --nocapture`
  - `cargo test -p nexus-config -- --nocapture`
  - `cargo test -p configd -- --nocapture`
  - `cargo test -p policyd -- --nocapture`
  - `cargo test -p nx -- --nocapture`
- Synchronized Policy as Code architecture docs and added a local `tools/nx/README.md` entrypoint for the canonical CLI.
- OS/QEMU policy markers remain gated and intentionally unclaimed.

### Changed - 2026-04-24

#### TASK-0046 / RFC-0044 closure sync (`TASK-0046`, `RFC-0044`)

- Closed the Config v1 host-first contract floor:
  - JSON-only authoring for layered config sources under `/system/config` and `/state/config`
  - canonical Cap'n Proto effective snapshots remain the runtime/persistence authority
  - `configd` subscriber/update notification seam is covered by deterministic host tests
  - `nx config push` now writes deterministic state overlay `state/config/90-nx-config.json`
- Added closure-proof coverage:
  - lexical-order layer-directory merge proof in `nexus-config`
  - non-JSON authoring reject proof in `nexus-config`
  - `nx config reload --json` and `nx config where --json` contract tests
  - `nx config effective --json` parity proof against `configd` version + derived JSON
- Synchronized status/index/queue surfaces:
  - `tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md` → `In Review`
  - `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md` → `Done`
  - `docs/rfcs/README.md`, `tasks/IMPLEMENTATION-ORDER.md`, `tasks/STATUS-BOARD.md`
  - `.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/pre_flight.md`, `.cursor/stop_conditions.md`, `.cursor/context_bundles.md`
- Normalized touched Rust source headers to the documented standard (`OWNERS` / `STATUS` / `API_STABILITY` / `TEST_COVERAGE` / `ADR`) and refreshed docs to describe the current proof state.

### Changed - 2026-04-23

#### TASK-0032 / RFC-0041 status synchronization (`TASK-0032`, `RFC-0041`)

- Updated execution/contract status to the requested review state:
  - `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` → `status: In Review`
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md` → `Status: Done`
- Synced RFC index wording in `docs/rfcs/README.md`:
  - `RFC-0041` now tracked as `Done`
  - execution SSOT `TASK-0032` now tracked as `In Review`
- Synced task tracking views:
  - `tasks/IMPLEMENTATION-ORDER.md` now has an `In Review` section with `TASK-0032`
  - `tasks/STATUS-BOARD.md` queue head and contract-status lines now point to `TASK-0032` / `RFC-0041`
  - `tasks/STATUS-BOARD.md` cumulative done table now includes `TASK-0029` and `TASK-0031`
- Updated packaging documentation `docs/packaging/nxb.md` with explicit `pkgimg-build` / `pkgimg-verify` usage notes for PackageFS v2 image generation and verification.

### Changed - 2026-04-23

#### TASK-0032 prep sync + queue/workfile alignment (`TASK-0029`, `TASK-0031`, `TASK-0032`, `RFC-0041`)

- Added `TASK-0029` and `TASK-0031` to the cumulative Done table in `tasks/IMPLEMENTATION-ORDER.md`.
- Created RFC seed contract for the active SSOT task:
  - `docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md`
- Linked the new seed from `tasks/TASK-0032-packagefs-v2-ro-image-index-fastpath.md` and updated `docs/rfcs/README.md` index entries.
- Synced active task prep workfiles for `TASK-0032` posture:
  - `.cursor/context_bundles.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`

### Changed - 2026-04-20

#### TASK-0023B Phase 6 functional closure + RFC-0038 → Done (`TASK-0023B`, `RFC-0038`)

- `TASK-0023B` advanced from `Draft` to `In Review` after Phase 6 (replay capability) reached functional closure across all six cuts.
- `RFC-0038` advanced from `Draft` to `Done`. One environmental closure step remains and is documented inline in the RFC header: external CI-runner replay artifact for P6-05; recipe lives in `docs/testing/replay-and-bisect.md` §7-§11.
- Phase 6 deliverables (cuts P6-01 → P6-06) shipped:
  - `tools/replay-evidence.sh` — bounded `--max-seconds` replay with hard env-override gate (`PROFILE` / `SELFTEST_PROFILE` / `RUN_PHASE` / `REQUIRE_*` / `KERNEL_CMDLINE` rejected), persistent worktree (`target/replay-worktree`) + Cargo cache reuse, automatic `NEXUS_SKIP_BUILD=1` warm-replay (cold ~67s, warm ~14s on dev box), structured logs, deterministic `nexus-evidence` / `nexus-proof-manifest` binary resolution.
  - `tools/diff-traces.sh` + `docs/testing/trace-diff-format.md` + `docs/testing/trace-diff-fixtures.json` — phase-aware classifier with `exact_match` / `extra_marker` / `missing_marker` / `reorder` / `phase_mismatch` classes.
  - `tools/bisect-evidence.sh` — bounded binary-search bisect with mandatory `--max-commits` + `--max-seconds`; synthetic mode extended to `good | drift | bad` so allowlist-absorbed drift is reported separately from regressions.
  - `scripts/regression-bisect.sh` — CI-friendly wrapper.
  - `docs/testing/replay-and-bisect.md` — operator workflow, append-only allowlist policy, evidence-map (§9), synthetic bad-bundle reproducer (§10), and the explicit remaining environmental step (§11).
- Phase-6 proof floor verified locally with reproducible artifacts:
  - empty-diff replay vs good bundle on native (`.cursor/replay-dev-a.json`) and containerized CI-like host (`.cursor/replay-ci-like.json`),
  - synthetic bad-bundle (tampered + re-sealed) classified diff with non-zero exit (`.cursor/replay-synthetic-bad.{log,json}` — `status: "diff", classes: ["missing_marker"]`),
  - 3-commit good→drift→regress bisect smoke (`.cursor/bisect-good-drift-regress.json` — `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`),
  - all hard gates verified (`--max-seconds`/`--max-commits` mandatory exits; `PROFILE` env override rejected with explicit error).
- Status synchronized across:
  - `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`
  - `docs/rfcs/README.md`
  - `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `docs/adr/0027-selftest-client-two-axis-architecture.md` (Current state section refreshed; ADR remains `Accepted` because Phase 4-6 work consumes the two-axis structure rather than altering it)
  - `docs/testing/index.md` (RFC-0038 added to Related RFCs; topic guides extended with §9-§11 anchors)
  - `source/apps/selftest-client/README.md` (Status section rewritten with full P1-P6 closure table + remaining environmental closure step)
  - `.cursor/handoff/current.md`, `.cursor/current_state.md`, `.cursor/next_task_prep.md`
- Sequencing: queue head moves to `TASK-0024` (DSoftBus QUIC recovery / UDP-sec) once the external CI-runner replay artifact for P6-05 is captured and the documented status flip is applied.

### Changed - 2026-04-15

#### TASK-0023 gate-prep sync (`TASK-0023`)

- Archived `.cursor/handoff/current.md` snapshot to `.cursor/handoff/archive/TASK-0022-dsoftbus-core-no-std-transport-refactor.md`.
- Synchronized `TASK-0023` to explicit blocked-state truth:
  - follow-up routing now explicit (`TASK-0024`, `TASK-0044`),
  - RED feasibility point resolved as documented gate outcome,
  - security proof test names aligned to existing host reject suites.
- Updated active workfiles and queue docs for production-grade anti-drift clarity (`.cursor/current_state.md`, `.cursor/handoff/current.md`, `.cursor/next_task_prep.md`, `.cursor/pre_flight.md`, `.cursor/stop_conditions.md`, `tasks/IMPLEMENTATION-ORDER.md`, `tasks/STATUS-BOARD.md`).
- Synced architecture/distributed docs that still referenced `TASK-0022` review state:
  - `docs/architecture/README.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`

#### TASK-0022 closure sync (`TASK-0022`, `RFC-0036`)

- `TASK-0022` is now `Done` after final production-quality verification and closure sync.
- `RFC-0036` is `Complete` and remains aligned as the closed contract seed for this slice.
- `TASK-0023` gated-contract closure is now done with blocked/no-go unlock outcome; sequential queue head is `TASK-0024` unless resequenced.
- `dsoftbus-core` crate boundary and review evidence synchronized into process docs:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
- Fresh quality/security/performance verification pass run:
  - `cargo +nightly-2025-01-15 check -p dsoftbus-core --target riscv64imac-unknown-none-elf`
  - `cargo test -p dsoftbus --test core_contract_rejects -- --nocapture`
  - `cargo test -p dsoftbus -- reject --nocapture`
  - `just test-dsoftbus-quic`
  - `just deny-check`
  - `just dep-gate && just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `just test-e2e && just test-os-dhcp`

### Changed - 2026-04-14

#### DSoftBus QUIC host-first closure sync (`TASK-0021`, `RFC-0035`)

- `TASK-0021` advanced from `In Review` to `Done`.
- Queue head advanced to `TASK-0022`.
- Closure state synchronized across task/board/workfiles:
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `README.md`
- Cargo-deny duplicate handling is now explicit and strict:
  - `multiple-versions = "deny"` remains enforced,
  - narrow compatibility skips were added only for `getrandom` (`0.2/0.3`) and `windows-sys` (`0.52/0.61`).
- Fresh green gate evidence includes:
  - `just test-os-dhcp`
  - `just test-dsoftbus-host`
  - `just test-all`
  - `just deny-check`

### Changed - 2026-04-10

#### DSoftBus mux v2 production closure (`TASK-0020`, `RFC-0033`, `RFC-0034`)

- `TASK-0020` is closed as `Done` with host, single-VM, and 2-VM marker proofs plus deterministic perf/soak and release-evidence artifacts.
- `RFC-0033` status is now `Complete` (mux v2 contract closure).
- `RFC-0034` status is now `Complete` for legacy `TASK-0001..0020` production-closure scope.
- Sequential queue head moved to `TASK-0021` after `TASK-0020` closeout.

### Changed - 2026-03-27

#### DSoftBus mux v2 kickoff (`TASK-0020`, `RFC-0033`)

- Verified `TASK-0019` closeout remains documented as `Done` across task status, board views, and changelog evidence.
- Moved `TASK-0020` to `In Progress` as the active sequential queue head.
- Moved `RFC-0033` to `In Progress` with `TASK-0020` as execution SSOT.
- Synced working-state artifacts for active execution context:
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`
  - `.cursor/context_bundles.md`

### Changed - 2026-03-27

#### ABI syscall guardrails v2 closeout (`TASK-0019`, `RFC-0032`)

- `TASK-0019` status advanced from `In Review` to `Done` after closing host/OS/QEMU proof gates.
- Workspace/task status sources were synchronized for drift-free closure:
  - `.cursor/current_state.md`
  - `.cursor/handoff/current.md`
  - `.cursor/next_task_prep.md`
  - `.cursor/pre_flight.md`
  - `.cursor/stop_conditions.md`
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`
- Root documentation now reflects closure and queue progression:
  - `README.md` (TASK-0019 done, next queue head TASK-0020)
- Additional green gate verification for this closeout:
  - `make build MODE=host`
  - `make test MODE=host`
  - `make run MODE=host RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s`

### Changed - 2026-03-26

#### Crashdump v1 final hardening closure sync (`TASK-0018`, `RFC-0031`)

- `TASK-0018` final hardening slice is now reflected across implementation + proof docs:
  - identity/report validation is fail-closed and deterministic,
  - explicit negative E2E markers are part of the canonical QEMU ladder:
    - `SELFTEST: minidump forged metadata rejected`
    - `SELFTEST: minidump no-artifact metadata rejected`
    - `SELFTEST: minidump mismatched build_id rejected`
- `execd` crash publish path now validates reported metadata against decoded bounded minidump bytes before emitting `execd: minidump written`.
- `statefsd` crash-write subject canonicalization is documented and unit-tested as a pure helper (narrow, path-bound mapping only; no broad SID-0 bypass).
- Task planning/status artifacts were synchronized for queue visibility and anti-drift:
  - `tasks/IMPLEMENTATION-ORDER.md`
  - `tasks/STATUS-BOARD.md`
  - `.cursor` SSOT/handoff/pre-flight/stop-conditions files
- Verification set for this sync includes:
  - `cargo test -p crash -- --nocapture`
  - `cargo test -p execd -- --nocapture`
  - `cargo test -p minidump-host -- --nocapture`
  - `cargo test -p statefsd -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

### Changed - 2026-03-24

#### Networking modularization + address governance closure sync (`TASK-0016B`, `RFC-0029`, `ADR-0026`)

- `netstackd` modular refactor closure is now synchronized in docs and task/rfc state:
  - `main.rs` is entry/wiring only, with runtime split under `source/services/netstackd/src/os/**`.
  - handler and IPC helper seams are now the canonical extension points for follow-on networking tasks.
- Networking address/profile governance is now explicit and centralized:
  - `docs/architecture/network-address-matrix.md` is the SSOT for QEMU + os2vm address profiles.
  - `docs/adr/0026-network-address-profiles-and-validation.md` records policy-level decisions.
- DNS proof validation remains deterministic but is now protocol-semantic (port/QR/TXID) rather than source-IP-pinned, avoiding backend-specific false negatives.
- Task board and implementation-order docs were refreshed to match real task/RFC status progression (`TASK-0016` Done, `TASK-0016B` Complete, `RFC-0028` Completed, `RFC-0029` Completed).
- Verification set for this sync includes:
  - `just dep-gate`
  - `just diag-os`
  - `just test-os-dhcp-strict`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`

### Changed - 2026-02-11

#### Perf/Power v1 closure (TASK-0013; RFC-0023 implemented)

- Kernel QoS syscall decode now deterministically rejects malformed/overflowed wire args with `-EINVAL` (no silent clamp).
- QoS authority model enforced and audited: self-set allows equal/lower only, escalation requires privileged `policyd/execd` path.
- New `timed` service path operational in OS bring-up with deterministic coalescing windows and bounded registration limits.
- Proof ladder extended and validated with deterministic markers, including negative over-limit and reject-path checks.
- Address-space/page-table lifecycle hardening landed during closure debugging to remove `KPGF`/allocation leak regressions in QEMU runs.

### Changed - 2026-02-10

#### Kernel SMP v1 closure sync (TASK-0012 Done; RFC-0021 Complete)

- Hardened SMP v1 proof semantics from marker-presence to causal anti-fake evidence:
  - `request accepted -> send_ipi success -> S_SOFT trap observed -> ack`
- Added deterministic SMP counterfactual proof marker:
  - `KSELFTEST: ipi counterfactual ok`
- Added/validated required SMP negative proof markers:
  - `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
  - `KSELFTEST: test_reject_offline_cpu_resched ok`
  - `KSELFTEST: test_reject_steal_above_bound ok`
  - `KSELFTEST: test_reject_steal_higher_qos ok`
- Canonical SMP harness gate now explicitly uses `REQUIRE_SMP=1` for SMP marker ladder runs.
- Documentation synchronized across task/rfc/testing/architecture/handoff to preserve drift-free follow-up prerequisites for TASK-0013/0042/0247/0283.

#### Build/QEMU reliability sync (default marker-driven run + blk lock serialization)

- `make run` now defaults to marker-driven mode (`RUN_UNTIL_MARKER=1`) so default runs complete green when the selftest ladder reaches `SELFTEST: end`.
- Added serialized lock handling for shared QEMU block image access in `scripts/run-qemu-rv64.sh` to avoid concurrent `blk.img` write-lock failures.

### Added - 2026-01-14

#### Observability v1 (TASK-0006: Complete)

**New Services**:
- `logd`: Bounded RAM journal for structured logs
  - Wire protocol v1: APPEND/QUERY/STATS (versioned byte frames for OS, Cap'n Proto for host)
  - Ring buffer semantics: drop-oldest on overflow, deterministic counters
  - Authenticated origin: `sender_service_id` from kernel IPC metadata
  - RFC: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (Complete)

**Logging Integration**:
- `nexus-log` extended with `logd` sink (`sink-logd` feature)
- Core services integrated: `samgrd`, `bundlemgrd`, `policyd`, `dsoftbusd`
- Existing UART readiness markers preserved for deterministic testing
- Fallback: UART-only if `logd` unavailable

**Crash Reporting**:
- `execd` crash reporting for non-zero exits
  - UART marker: `execd: crash report pid=<pid> code=<code> name=<name>`
  - Structured crash event appended to `logd` (queryable for post-mortem)
  - Stable crash event keys: `event=crash.v1`, `pid`, `code`, `name`, `recent_count`
  - Reserved keys for future: `build_id`, `dump_path`

**Testing**:
- Host tests: `cargo test -p logd`, `cargo test -p nexus-log`
- QEMU markers (all green as of 2026-01-14):
  - `logd: ready`
  - `SELFTEST: log query ok`
  - `SELFTEST: core services log ok`
  - `execd: crash report pid=... code=42 name=demo.exit42`
  - `SELFTEST: crash report ok`

**Documentation**:
- New: `docs/observability/logging.md` (usage guide)
- New: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (contract seed)
- Updated: `docs/architecture/` (10+ files), `docs/testing/index.md`, ADR-0017

**Demo Payloads**:
- `demo.exit42` added to `userspace/apps/demo-exit0` for crash report testing

**Breaking Changes**: None (additive only)

**Known Limitations (v1 scope)**:
- Journal is RAM-only (no persistence)
- No streaming/subscriptions (bounded queries only)
- No remote export (deferred to TASK-0040)
- No metrics/tracing integration (deferred to TASK-0014)

### Added - 2026-01-25

#### Policy authority + audit baseline v1 (TASK-0008: Done; RFC-0015: Complete)

- `policyd` established as the **single policy authority** with deny-by-default semantics.
- Audit trail for allow/deny decisions (via `logd`), binding authorization to kernel `sender_service_id`.
- Policy-gated sensitive operations (baseline): signing/exec/install paths enforced without duplicating authority logic.
- Contract: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`

### Added - 2026-01-27

#### Device identity keys v1 (TASK-0008B: Done; RFC-0016: Done)

- OS/QEMU device identity key generation path proved without `getrandom`:
  - virtio-rng MMIO → `rngd` (entropy authority) → `keystored` (device keygen + pubkey-only export).
- Bounded entropy requests and negative proofs (oversized/denied/private-export reject); no secrets logged.
- Contract: `docs/rfcs/RFC-0016-device-identity-keys-v1.md`

### Added - 2026-02-02

#### Device MMIO access model v1 (TASK-0010: Done; RFC-0017: Done)

- Kernel/userspace contract for capability-gated device MMIO mapping (`DeviceMmio` + mapping syscall).
- Enforced security floor: USER|RW mappings only, never executable; bounded per-device windows; init/policyd control distribution.
- Contract: `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`

### Added - 2026-02-06

#### Persistence v1 (TASK-0009: Done; RFC-0018: Complete; RFC-0019: Complete)

- StateFS journal format v1 + `/state` authority service (`statefsd`) with deterministic host + QEMU proofs.
- IPC request/reply correlation v1 (nonces + bounded reply buffering) to keep shared-inbox flows deterministic under QEMU.
- Modern virtio-mmio default for virtio-blk in the canonical QEMU harness (legacy remains opt-in).
- Contracts:
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
  - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`

### Changed - 2026-02-09

#### Kernel simplification (TASK-0011: Complete; RFC-0001: Complete)

- Kernel tree reorganized into stable responsibility-aligned directories (mechanical moves + wiring only).
- Kernel module headers normalized; invariants and test scope made explicit to lower debug/navigation cost.
- Contract: `docs/rfcs/RFC-0001-kernel-simplification.md`

---

## Previous Releases

See Git history for releases prior to 2026-01-14.
