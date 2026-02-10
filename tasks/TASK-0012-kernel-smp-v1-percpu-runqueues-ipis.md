---
title: TASK-0012 Performance & Power v1 (kernel): SMP bring-up + per-CPU runqueues + IPIs (QEMU riscv virt)
status: In Review
owner: @kernel-team
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md
  - Kernel overview: docs/architecture/01-neuron-kernel.md
  - QEMU platform determinism: docs/dev/platform/qemu-virtio-mmio-modern.md
  - QEMU smoke proof policy: docs/adr/0025-qemu-smoke-proof-gating.md
  - Kernel SMP/parallelism policy (normative): tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md
  - Depends-on (orientation): tasks/TASK-0011-kernel-simplification-phase-a.md
  - Pre-SMP ownership/types contract (seed): docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md
  - Pre-SMP execution/proofs: tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md
  - Testing contract: scripts/qemu-test.sh
  - Unblocks: tasks/TRACK-DRIVERS-ACCELERATORS.md (per-CPU driver scheduling, multi-queue devices)
  - Unblocks: tasks/TRACK-NETWORKING-DRIVERS.md (multi-queue NIC scheduling baseline)
enables:
  - TASK-0012B: SMP v1b hardening bridge (scheduler/SMP internals, bounded queue behavior, trap/IPI contract hardening)
  - TASK-0013: Perf/Power v1 (QoS ABI + timed coalescing)
  - TASK-0042: SMP v2 (affinity hints + QoS budgets)
  - TASK-0247: RISC-V bring-up v1.1b extension (SBI HSM/IPI hardening + per-hart timers + storage integration)
  - TASK-0283: Per-CPU ownership wrapper adoption (`PerCpu<T>`) for additional compile-time hardening
  - TRACK-DRIVERS-ACCELERATORS: per-CPU driver scheduling baseline for device-class services
  - TRACK-NETWORKING-DRIVERS: per-CPU scheduler baseline for future NIC/offload work
follow-up-tasks:
  - TASK-0012B: harden scheduler/SMP internals (bounded queue behavior + trap/IPI contract + CPU-ID fast path) while preserving TASK-0012 markers/proofs
  - TASK-0013: add userspace QoS/timer behavior on top of SMP v1 without introducing a second scheduler authority
  - TASK-0042: extend SMP with affinity/shares while preserving TASK-0012 ownership and determinism invariants
  - TASK-0247: extend RISC-V specifics (SBI HSM/IPI hardening + per-hart timers) on top of TASK-0012 baseline only
  - TASK-0283: optionally introduce `PerCpu<T>` wrappers where they reduce cross-CPU mutation risk
---

## Context

NEURON is currently effectively single-hart. To scale performance and enable power-aware policies we
need SMP bring-up on QEMU `virt` (RISC-V) with a minimal, debug-friendly scheduler model:

- secondary hart boot,
- per-CPU runqueues,
- IPIs for rescheduling,
- and a simple work-stealing policy.

Kernel changes are inherently high-debug-cost, so we gate each step with deterministic KSELFTEST
markers.

## Goal

Boot with SMP enabled (e.g. QEMU `-smp 2`) and prove:

- secondary CPU(s) come online,
- scheduling runs tasks across CPUs,
- IPI resched path works,
- and basic work stealing works when one queue is empty.

## Non-Goals

- QoS ABI and userland QoS policy (handled in TASK-0013).
- Interrupt-driven virtio; polling is fine.
- Advanced load balancing / fairness / starvation guarantees beyond simple stealing.
- RISC-V-specific SMP extension scope (per-hart timers, broader HSM/IPI hardening, and bring-up packaging) handled by `TASK-0247`.

## Constraints / invariants (hard requirements)

- Preserve existing single-hart behavior when SMP=1.
- Deterministic markers for boot + selftests.
- Avoid unbounded logging and debug-only flood.
- QEMU SMP proofs MUST run on modern virtio-mmio defaults (`virtio-mmio.force-legacy=off`); legacy mode (`QEMU_FORCE_LEGACY=1`) is debug-only and not part of green proof gates.
- SMP marker checks in `scripts/qemu-test.sh` MUST be explicitly gated (for example `REQUIRE_SMP=1` or equivalent SMP-aware condition), so default single-hart smoke remains deterministic.
- SMP implementation must follow the kernel parallelism policy `TASK-0277` (ownership model, lock rules, deterministic invariant proofs).
- SMP implementation MUST concretely build on the ownership/type-safety contracts established in RFC-0020 (TASK-0011B):
  - Prefer per-CPU ownership over shared mutable scheduler state.
  - Reuse the “kernel handle newtypes” pattern for SMP identifiers (e.g. CPU/Hart ID) rather than raw integers.
  - Treat pre-SMP `!Send/!Sync` markers as intentional forcing functions: SMP work must either keep types thread-bound (per-CPU) or introduce synchronization and justify any change in auto-trait behavior.
- Carry-over hardening note from TASK-0011B: `source/kernel/neuron/src/arch/riscv/trap.S` currently assumes global `__stack_top` on U-mode trap entry; TASK-0012 MUST switch trap entry to a per-hart kernel stack source before multi-hart traps are considered complete.

## Red flags / decision points

- **RESOLVED (formerly RED)**:
  - Hart boot method on QEMU `virt`: use SBI HSM `hart_start` as the TASK-0012 baseline for secondary hart bring-up.
  - If HSM support is unavailable in the runtime environment, fail fast with explicit bring-up failure evidence and keep TASK-0012 blocked (no hidden fallback path).
  - `TASK-0247` extends this baseline (RISC-V-specific hardening/timers) and MUST NOT create a parallel SMP authority.
- **YELLOW**:
  - Scheduler correctness under concurrency: keep locks minimal and auditable; prefer simple data
    structures first (per-CPU VecDeque + locks) before lock-free experiments.
- **GREEN**:
  - Kernel already has a QoS bucket scheduler (`QosClass`) and a deterministic tick model; SMP can
    extend this rather than redesigning scheduling from scratch.

## Security considerations

### Threat model

- **Per-CPU data races**: Concurrent access to scheduler state during SMP initialization
- **IPI spoofing**: Malicious tasks attempting to send IPIs to arbitrary CPUs
- **Work stealing attacks**: Tasks attempting to steal from other CPUs to bypass scheduling policy
- **CPU affinity bypass**: Tasks attempting to migrate to restricted CPUs
- **Resource exhaustion**: Unbounded IPI queues or work stealing causing DoS
- **Information leakage**: Per-CPU statistics revealing scheduling patterns

### Security invariants (MUST hold)

All existing kernel security invariants from TASK-0011 remain unchanged, plus:

- **Per-CPU isolation**: Each CPU's scheduler state is isolated (no cross-CPU mutable access without explicit synchronization)
- **IPI authentication**: IPI sender is verified (hardware CPU ID, not user-controllable)
- **Work stealing bounds**: Work stealing is bounded (max N tasks per steal operation)
- **CPU online mask integrity**: CPU online mask is atomic and cannot be corrupted by concurrent updates
- **Scheduler invariants preserved**: QoS ordering, no task loss, no task duplication during migration
- **No priority inversion**: Work stealing does not violate QoS priorities (steal from same or lower QoS only)

### DON'T DO (explicit prohibitions)

- DON'T share mutable scheduler state between CPUs without synchronization (use per-CPU ownership)
- DON'T trust user-provided CPU IDs for IPI targets (validate against online mask)
- DON'T allow unbounded work stealing (enforce max tasks per steal)
- DON'T skip TLB shootdown when migrating tasks between CPUs
- DON'T use `static mut` for shared state (use atomics or per-CPU arrays)
- DON'T allow tasks to query or manipulate other CPUs' runqueues directly
- DON'T log sensitive scheduling information (task IDs, CPU assignments) in production builds

### Attack surface impact

- **Minimal**: SMP adds IPI handlers (new interrupt surface) but IPI authentication is hardware-enforced
- **Controlled**: Work stealing is explicit and bounded (no arbitrary cross-CPU access)
- **Mitigated**: Per-CPU ownership prevents most concurrency bugs at compile time (Rust ownership model)

### Mitigations

- **Per-CPU ownership**: Each CPU owns its scheduler (Rust's ownership prevents data races)
- **Atomic CPU mask**: CPU online mask uses atomic operations (no locks, no races)
- **IPI validation**: IPI sender is hardware CPU ID (unforgeable)
- **Bounded stealing**: Work stealing limited to N tasks per operation (prevents DoS)
- **TLB shootdown**: Task migration triggers TLB flush on target CPU (prevents stale mappings)
- **Deterministic markers**: KSELFTEST markers prove SMP correctness (no timing-dependent tests)

### SMP-specific security requirements

When implementing SMP features, ensure:

1. **Per-CPU stacks**: Each CPU has isolated stack with guard pages (no shared stack)
2. **IPI mailboxes**: Bounded queues (prevent memory exhaustion)
3. **Work stealing policy**: Only steal from same or lower QoS class (preserve priorities)
4. **CPU affinity**: Tasks cannot bypass affinity restrictions via work stealing
5. **Audit records**: SMP events (CPU online, IPI, migration) are logged for security analysis

## Contract sources (single source of truth)

- `docs/architecture/01-neuron-kernel.md` (scheduler overview + determinism)
- `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` (ownership model + newtypes + explicit Send/Sync boundaries)
- KSELFTEST marker contract (must be added/updated in kernel selftests)
- QEMU acceptance harness + marker ordering contract: `scripts/qemu-test.sh` (and `docs/testing/index.md`)

## Stop conditions (Definition of Done)

- QEMU run with SMP>=2 produces:
  - `KINIT: cpu1 online` (and higher as configured)
  - `KSELFTEST: smp online ok`
  - `KSELFTEST: ipi counterfactual ok`
  - `KSELFTEST: ipi resched ok`
  - `KSELFTEST: test_reject_invalid_ipi_target_cpu ok`
  - `KSELFTEST: test_reject_offline_cpu_resched ok`
  - `KSELFTEST: work stealing ok`
  - `KSELFTEST: test_reject_steal_above_bound ok`
  - `KSELFTEST: test_reject_steal_higher_qos ok`
- Single-hart run (SMP=1) remains green with existing markers and unchanged default smoke semantics.
- Host + compile gates remain green:
  - `cargo test --workspace` passes
  - `just diag-os` passes
- Required proof commands:
  - `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- Docs stay in sync:
  - Update `docs/architecture/01-neuron-kernel.md` and `docs/architecture/README.md` to reflect any new SMP invariants/markers and any scheduler model changes.
  - If SMP marker-gating behavior in the harness changes, update `docs/testing/index.md` in the same slice.

## Touched paths (allowlist)

- `source/kernel/neuron/src/**`
- `scripts/run-qemu-rv64.sh` (only if needed to parameterize `SMP`)
- `scripts/qemu-test.sh` (marker expectations for SMP runs, gated/optional)
- `docs/architecture/01-neuron-kernel.md`
- `docs/architecture/README.md`
- `docs/testing/index.md` (only if marker-gating behavior/commands change)

## Follow-up alignment (anti-drift)

- `TASK-0012B` is the explicit hardening bridge between baseline TASK-0012 behavior and policy/extensions; it must not redefine marker authority.
- `TASK-0013` consumes the TASK-0012 + TASK-0012B scheduler baseline but does not redefine SMP authority.
- `TASK-0042` extends scheduling policy (affinity/shares) and must preserve TASK-0012 + TASK-0012B ownership and determinism boundaries.
- `TASK-0247` extends RISC-V specifics (HSM/IPI hardening and per-hart timers) on top of TASK-0012 + TASK-0012B; no duplicate SMP stack is allowed.
- `TASK-0283` is an optional hardening layer (`PerCpu<T>`) that should refine, not replace, TASK-0012 + TASK-0012B behavior proofs.
- `TRACK-DRIVERS-ACCELERATORS` and `TRACK-NETWORKING-DRIVERS` both depend on this SMP baseline remaining deterministic and auditable.

## Plan (small PRs)

1. **CPU discovery + online mask**
   - Provide `cpu_current_id()` and `cpu_online_mask()`; log `KINIT: cpuN online` once per hart.
   - Use a dedicated CPU/Hart ID newtype (RFC-0020 newtype pattern) rather than raw integers.

2. **Secondary hart boot**
   - Bring up harts 1..N-1 deterministically.
   - Use SBI HSM `hart_start` as the baseline boot path on QEMU `virt`.
   - Wire per-hart kernel stack pointers for trap entry (`trap.S`) so U-mode trap path no longer relies on global `__stack_top`.
   - Keep `sscratch` semantics deterministic per hart (save/restore user SP only for the current hart context).

3. **IPI resched**
   - Implement a minimal S-mode IPI resched signal and handler; prove via selftest marker.

4. **Per-CPU runqueues**
   - Replace the single runqueue with per-CPU queues.
   - Prefer strict per-CPU ownership of runqueues (minimize shared mutable state).
   - If any structure must become cross-CPU shared, add explicit synchronization and document the ownership change in `docs/architecture/01-neuron-kernel.md` (tie back to RFC-0020 ownership model).

5. **Work stealing**
   - Simple round-robin steal when local queue empty; prove via selftest marker.
   - Steal implementation must remain bounded and should not require “reach into” another CPU’s runqueue without an explicit, audited synchronization boundary.

6. **Proof wiring + docs sync**
   - Keep default single-hart smoke deterministic; gate SMP-only markers behind explicit SMP proof mode.
   - Update architecture/testing docs in the same slice when marker contracts or ownership rules change.

## Acceptance criteria (behavioral)

- SMP=2 reliably boots and emits all required SMP markers (`KINIT: cpu1 online`, `KSELFTEST: smp online ok`, `KSELFTEST: ipi counterfactual ok`, `KSELFTEST: ipi resched ok`, `KSELFTEST: work stealing ok`, plus `test_reject_*` negative markers).
- SMP=1 remains green with unchanged default smoke marker semantics.
- Secondary-hart trap entry no longer depends on global `__stack_top` for multi-hart correctness.
- Host/compile proof gates remain green (`cargo test --workspace`, `just diag-os`).
- SMP proof commands are explicit and reproducible (`SMP=2` and `SMP=1` marker-gated runs).
- Follow-up task boundaries remain drift-free (TASK-0013/0042/0247/0283 and both TRACK dependencies).
