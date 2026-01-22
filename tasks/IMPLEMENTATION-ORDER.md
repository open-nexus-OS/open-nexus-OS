# Implementation Order (Dynamic): What to Build Next

This file is a **dynamic execution-order view** over `tasks/TASK-*.md`.

It exists so that:

- new tasks can be added without worrying about “correct sorting”,
- the team can keep a single “what’s next?” plan,
- and we can reorder as dependencies, risk, and scope change.

This file is **not authoritative** for scope/DoD; each `TASK-*.md` remains execution truth.

For a Kanban-style status view (Draft/In Progress/Done + blockers), see: `tasks/STATUS-BOARD.md`.

## How to maintain this file (rules)

- **Prefer phases over perfect linear ordering**: keep a short list of “current” items and a larger queue.
- **Reordering is allowed** at any time, but must not contradict task dependencies (“Depends-on”, “Gated on”, “Unblocks”).
- **Never use this list to skip the 100% rule**: only mark a task “active” if its Stop conditions can be met (or you explicitly split/extract prerequisites per `tasks/README.md`).
- **Keep entries lightweight**:
  - Task ID + short reason + prerequisites.
  - Avoid duplicating full task content here.

## Lanes (workstreams)

We track ordering in lanes so unrelated work doesn’t block planning:

- **KERNEL**: NEURON kernel and kernel-adjacent ABI primitives
- **DRIVERS**: device-class services, DriverKit, and acceleration tracks
- **RUNTIME/SECURITY**: policyd, syscall guardrails, audit, identity
- **BRING-UP**: QEMU/RISC-V bring-up milestones

## Current “Next Up” (recommended)

### KERNEL (Type safety + SMP foundations)

1. `TASK-0011-kernel-simplification-phase-a.md`
   - **Why**: keeps kernel navigable for SMP debugging (lowest-risk foundation).

2. `TASK-0011B-kernel-rust-idioms-pre-smp.md`
   - **Why**: ownership model + error conventions + type hygiene pre-SMP.

3. `TASK-0281-kernel-newtypes-v1c-handle-typing.md`
   - **Why**: low-risk, reduces “handle confusion” bug class.
   - **Prereq**: TASK-0011B.

4. `TASK-0282-kernel-capability-phantom-rights-v1.md`
   - **Why**: compile-time rights/kind checks inside kernel paths.
   - **Prereq**: TASK-0011B, TASK-0267 context.

5. `TASK-0283-kernel-percpu-ownership-wrapper-v1.md`
   - **Why**: prevents cross-CPU mutable access by construction (`!Send`).
   - **Prereq**: TASK-0012 planning alignment.

6. `TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
   - **Why**: SMP bring-up (per-CPU runqueues + IPIs).
   - **Prereq**: TASK-0011 + TASK-0011B.

7. `TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md`
   - **Why**: affinity + QoS budgets to support latency-sensitive device-class services.
   - **Prereq**: TASK-0012 + TASK-0013.

### DRIVERS (stable boundary first)

1. `docs/adr/0018-driverkit-abi-versioning-and-stability.md`
   - **Why**: prevents API fragmentation; defines stability rules.

2. `TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md`
   - **Why**: extracts DriverKit core contract (`CAND-DRV-000`) into a testable v1.
   - **Prereq**: TASK-0010 + TASK-0031 + TASK-0013.

3. `TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md`
   - **Why**: proves ownership-based “zero-copy” buffer lifecycle (host-first).
   - **Prereq**: TASK-0031.

### BRING-UP (RISC-V virt)

- `TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md`
- `TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md`
- `TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md`

### RUNTIME/SECURITY (guardrails + policy authority)

- `TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md`
- `TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md` (true enforcement)
- `TASK-0019-security-v2-userland-abi-syscall-filters.md` (guardrail; not a boundary)
- `TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`

## Backlog (keep short; re-rank as you learn)

- `tasks/TRACK-DRIVERS-ACCELERATORS.md` (direction + gates)
- `tasks/TRACK-NETWORKING-DRIVERS.md` (direction + gates)
- `tasks/TRACK-NEXUSGFX-SDK.md` (SDK direction)
- `tasks/TRACK-NEXUSMEDIA-SDK.md` (SDK direction: audio/video/image)
- `tasks/TRACK-NEXUSGAME-SDK.md` (SDK direction: games)