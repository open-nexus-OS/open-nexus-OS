# Changelog

All notable changes to Open Nexus OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

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
