# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the *current* system state.
It is intentionally compact and overwritten after each completed task.

Rules:
- Prefer structured bullets over prose.
- Include "why" (decision rationale), not implementation narration.
- Reference tasks/RFCs/ADRs with relative paths.
-->

## Current architecture state
- **last_decision**: `tasks/TASK-0014-observability-v2-metrics-tracing.md` (phase-0 sink-path stabilized with explicit deterministic slot wiring contract in `nexus-log`)
- **rationale**:
  - Lower kernel debug/navigation cost with explicit module headers and a stable physical layout
  - Make pre-SMP ownership and concurrency boundaries explicit before behavioral SMP work
  - Lock TASK-0012 authority boundaries early (no duplicate SMP stack in TASK-0247 extensions)
  - Deterministic persistence substrate for `/state` with bounded replay and CRC32-C integrity
  - Deterministic request/reply correlation (nonces + bounded dispatcher) to avoid shared-inbox desync under QEMU/OS
  - QEMU harness defaults to modern virtio-mmio (legacy opt-in) for virtio-blk determinism
  - QEMU smoke proofs must be deterministic; networking proofs are opt-in (ADR-0025)
  - StateFS v1 remains a service authority (no VFS mount in v1)
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - W^X for MMIO (device mappings are USER|RW, never EXEC)
  - Policy decisions bound to kernel `sender_service_id` (not payload strings)
  - All security decisions audited via logd (no secrets in logs)
  - Kernel remains minimal (device enumeration, policy logic in userspace)
  - CRC32-C (Castagnoli) is the StateFS v1 integrity contract

## Current focus (execution)

- **active_task**: `tasks/TASK-0014-observability-v2-metrics-tracing.md` (implementation complete for planned slices; task intentionally kept In Review pending explicit closure command)
- **seed_contract**: `docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md` (design seed / active contract for TASK-0014)
- **contract_dependencies**:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (bounded log sink baseline)
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` (`/state` substrate baseline for retention slices)
  - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` (shared-inbox correlation floor)
  - `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (timed producer baseline)
- **phase_now**: TASK-0014 implementation complete; review evidence synced and proofs green
- **baseline_commit**: `f44a4f7`
- **next_task_slice**: closure handoff readiness + evidence preservation (status remains In Review until explicit close instruction)
- **proof_commands**:
  - `cargo test --workspace`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- **last_completed**: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
  - Proof gates: `cargo test --workspace`, `just dep-gate`, `just diag-os`, `just test-os`, `SMP=2 REQUIRE_SMP=1 ...`, `SMP=1 ...`, `make build`, `make test`, `make run`
  - Outcome: QoS ABI hard rejects + timed coalescing service + deterministic audit/marker proofs

## Active invariants (must hold)
- **security**
  - Secrets never logged (device keys, credentials, tokens)
  - Identity from kernel IPC (`sender_service_id`), never payload strings
  - Bounded input sizes; validate before parse; no `unwrap/expect` on untrusted data
  - Policy enforcement via `policyd` (deny-by-default + audit)
  - MMIO mappings are USER|RW and NEVER executable (W^X enforced at page table)
  - Device capabilities require explicit grant (no ambient MMIO access)
  - Per-device windows bounded to exact BAR/window (no overmap)
- **determinism**
  - Marker strings stable and non-random
  - Tests bounded (no infinite/unbounded waits)
  - UART output deterministic for CI verification
  - QEMU runs bounded by RUN_TIMEOUT + early exit on markers
- **build hygiene**
  - OS services use `--no-default-features --features os-lite`
  - Forbidden crates: `parking_lot`, `parking_lot_core`, `getrandom`
  - `just dep-gate` MUST pass before OS commits
  - `just diag-os` verifies OS services compile for riscv64

## Open threads / follow-ups
- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` — **COMPLETED** (host tests + QEMU persistence markers green under modern virtio-mmio)
- `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md` — **COMPLETED** (Phases 0/1/2 green; LO v2 nonce frames implemented for multiplexed logd RPCs)
- `userspace/nexus-ipc::reqrep` exists (bounded reply buffer + unit tests) and core services use strict nonce matching on shared inboxes.
- Deterministic host tests now exist for IPC budget + **logd/policyd wire parsing**: `cargo test -p nexus-ipc` (no QEMU required for these proof slices)
- `tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md` — draft; no longer blocked on persistence (TASK-0009 done); remaining gates are the update/apply/verify/commit implementation + proofs
- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` — **CLOSED**:
  - implemented: `timed` service + deterministic coalescing windows + per-owner timer cap + `timed: ready` + `SELFTEST: timed coalesce ok`
  - implemented: QoS decode overflow hard reject (`-EINVAL`) and timer over-limit reject tests
  - implemented: init-lite timed routing/wiring including deterministic self-route allow for service bootstrap
  - implemented: exec-path stability unblock:
    - fixed VMO arena identity-map/base mismatch (removed deterministic `KPGF` at `0x81800000` boundary),
    - fixed post-unblock deterministic `ALLOC-FAIL` via AS/page-table reclamation on child reap (`destroy` + `PageTable::drop` heap-page free under `bringup_identity`),
    - latest `RUN_PHASE=mmio` smoke is green (`exit_code=0`, no KPGF/ALLOC-FAIL).
  - implemented: privileged QoS authority bound to kernel service identity (`execd`/`policyd`) instead of capability-slot shortcut.
  - implemented: explicit audit trail for QoS/timer decisions (`QOS-AUDIT` + `timed: audit register ...`).
  - proof reruns green: host gates + `just test-os` + SMP=2 + SMP=1 after final patch.
- `tasks/TASK-0014-observability-v2-metrics-tracing.md` — **IN REVIEW**:
  - phase-0a logd hardening and phase-0 metricsd+nexus-metrics baseline are green in mmio proofs,
  - `metricsd -> nexus-log -> logd` path stabilized by explicit per-service `configure_sink_logd_slots(...)` contract,
  - current proven runtime progress:
    - `SELFTEST: metrics security rejects ok`
    - `SELFTEST: metrics counters ok`
    - `SELFTEST: metrics gauges ok`
    - `SELFTEST: metrics histograms ok`
    - `SELFTEST: tracing spans ok`
    - `SELFTEST: metrics retention ok`
    - `SELFTEST: device key pubkey ok`
    - `SELFTEST: statefs put ok`
    - `SELFTEST: statefs persist ok`
    - `SELFTEST: ota stage ok`
    - `SELFTEST: ota switch ok`
    - `SELFTEST: ota health ok`
    - `SELFTEST: ota rollback ok`
    - `SELFTEST: bootctl persist ok`
  - latest ladder proof: `RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` returned `exit_code=0` with no missing marker and included `[INFO metricsd] retention wal verified`.
  - final closure run in this slice also passed:
    - `just dep-gate`
    - `just diag-os`
    - `cargo test --workspace`
    - `RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - additional hardening applied in this slice:
    - `selftest-client` logd STATS path migrated to CAP_MOVE + nonce-correlation on shared reply inbox (eliminates false zero-count/delta regressions under alias sender IDs),
    - `policyd` identity checks now normalize known bring-up alias IDs for sender-bound checks and delegated subjects (`updated` alias included),
    - `metricsd` retention path now uses bounded non-blocking `/state` writes with deterministic WAL/rollup/TTL behavior and proof marker.
    - fail-closed nonce-correlated delegated-cap decode helpers are now centralized and unit-tested in `execd`, `rngd`, `keystored`, and `statefsd`.
  - approved implementation reality:
    - kernel stabilization exception is accepted for this slice (heap budget increase + alloc diagnostics), with no kernel ABI expansion.
  - full-scope closure slices implemented and proofed; task remains open only by explicit status policy.
- DMA capability model (future) — out of scope for MMIO v1
- IRQ delivery to userspace (future) — separate RFC needed
- virtio virtqueue operations beyond MMIO probing — follow-up after statefs proven
- **kernel execution order (current)**:
  - `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md` — complete (phases 0→5, proofs green)
  - `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` — complete (SMP baseline + anti-fake markers + negative tests)
  - `tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md` — complete (bounded enqueue, trap/IPI contract hardening, CPU-ID guarded hybrid path)
  - `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` — complete (QoS ABI + timed coalescing closure)
  - `tasks/TASK-0014-observability-v2-metrics-tracing.md` — in review (metricsd + tracing export via logd; closure pending explicit command)
- **exported prerequisites**:
  - `TASK-0013`: consumes 0012+0012B scheduler baseline (no alternate scheduler authority)
  - `TASK-0014`: consumes `TASK-0006` + `TASK-0009` + `RFC-0019` + `TASK-0013`; exports baseline for `TASK-0038/0040/0041/0143/0046`
  - `TASK-0042`: extends affinity/shares without violating 0012+0012B ownership and bounded-steal invariants
  - `TASK-0247`: may harden SBI/HSM/IPI/per-hart timers on top of 0012+0012B only
  - `TASK-0283`: optional `PerCpu<T>` hardening layer refines 0012+0012B contracts only
  - `TASK-0013` must keep mutable trap-runtime boundary unchanged (boot-hart-only until `TASK-0247`)

## Known risks / hazards
- **Post-TASK-0012 carry-over**:
  - Keep SMP proof ladder sequential; parallel QEMU runs can still produce lock/contention artifacts.
  - TASK-0012B/TASK-0013/TASK-0042 must preserve `test_reject_*` determinism and the strict IPI evidence chain.
  - TASK-0012B must make queue/backpressure and CPU-ID fast-path/fallback contracts explicit and tested.
- **QEMU smoke gating**:
  - Default `just test-os` now reaches `SELFTEST: end` deterministically and early-exits within the 90s harness timeout (no missing-marker deadlocks).
  - `REQUIRE_QEMU_DHCP=1` is green because we accept the honest static fallback marker when DHCP does not bind.
  - `REQUIRE_QEMU_DHCP_STRICT=1` is green and now reaches DHCP bound again:
    - Root cause (resolved): virtio-net RX parsing assumed a 10-byte header, but QEMU delivered frames with the 12-byte MRG_RXBUF header → Ethernet frames were misaligned and RX traffic was unreadable.
    - After fixing the header length, strict mode reaches `net: dhcp bound ...` and emits the dependent proofs (`SELFTEST: net ping ok`, `SELFTEST: net udp dns ok`, `SELFTEST: icmp ping ok`).
  - **Operational gotcha**: do not run multiple QEMU smoke runs in parallel; they contend on `build/blk.img` and can trip QEMU “write lock” errors. Run sequentially in CI/dev.
- **Shared inbox correlation**:
  - **StateFS** now uses a nonce-correlated `SF v2` frame shape for shared reply inbox calls (removes “drain stale replies” from the persistence proof path).
  - Init-lite routing now supports a backwards-compatible routing v1+nonce extension so ctrl-plane queries no longer rely on stale-drain patterns.
- **Policy timing**: early boot race between policyd readiness and init cap distribution
  - Current: retry loops with bounded timeout (1s deadline)
  - Future: explicit readiness channel to avoid retry polling
- **MMIO slot discovery**: dynamic probing of virtio-mmio devices at fixed addresses
  - Current: hardcoded QEMU virt addresses (0x10001000 + 0x1000 * slot)
  - Future: proper device tree parsing if targeting real hardware
- **TASK-0013 completion risk**:
  - closed in this slice; no remaining exec/QoS/timed blocker observed in proof ladder.
- **TASK-0014 execution risks (active)**:
  - cardinality/rate limits must be enforced deterministically to avoid observability-induced DoS.
  - local-v2 scope must remain strict (no remote/cross-node creep from `TASK-0038`/`TASK-0040`).
  - proof must validate logd export path, not only local in-memory counters.

## DON'T DO (session-local)
- DON'T add kernel MMIO grants via name-checks (init-controlled distribution only)
- DON'T skip policy checks for "trusted" services (deny-by-default always)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T extend TASK-0009 scope to include VFS mount (statefs authority first, mount is follow-up)
- DON'T assume real reboot/VM reset works in v1 (soft reboot = statefsd restart only)
