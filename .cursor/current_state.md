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
- **last_decision**: `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (Status: Complete, TASK-0011B delivered)
- **rationale**:
  - Lower kernel debug/navigation cost with explicit module headers and a stable physical layout
  - Make pre-SMP ownership and concurrency boundaries explicit before behavioral SMP work
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

- **active_task**: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (next)
- **seed_contract**: `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (completed seed)
- **phase_now**: TASK-0011B complete; handoff prepared for TASK-0012
- **baseline_commit**: `555d5a0`
- **next_task_slice**: TASK-0012 Phase 1 bootstrap (per-CPU runqueues/IPI scaffolding)
- **proof_commands**:
  - `cargo test --workspace`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- **last_completed**: `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
  - Proof gates: `cargo test --workspace`, `just diag-os`, and `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Outcome: all phases 0→5 complete; logic/ABI/marker contract preserved

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
- DMA capability model (future) — out of scope for MMIO v1
- IRQ delivery to userspace (future) — separate RFC needed
- virtio virtqueue operations beyond MMIO probing — follow-up after statefs proven
- **kernel execution order (current)**:
  - `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md` — complete (phases 0→5, proofs green)
  - `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` — active next (behavioral SMP work)

## Known risks / hazards
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

## DON'T DO (session-local)
- DON'T add kernel MMIO grants via name-checks (init-controlled distribution only)
- DON'T skip policy checks for "trusted" services (deny-by-default always)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T extend TASK-0009 scope to include VFS mount (statefs authority first, mount is follow-up)
- DON'T assume real reboot/VM reset works in v1 (soft reboot = statefsd restart only)
