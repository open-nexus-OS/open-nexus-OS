# RFC-0033: Soft-real-time spine — waitset + timeline fence + DriverKit submit

- Status: Proposed (v1)
- Owners: @kernel-team @runtime @ui
- Created: 2026-06-16
- Links:
  - Tracks: `tasks/TRACK-NEXUSGFX-SDK.md`, `tasks/TRACK-DRIVERS-ACCELERATORS.md`
  - Tasks: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (QoS/`timed`, **implemented**),
    `tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md` (DriverKit core)
  - Builds on: `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md` (timer objects + QoS exist)
  - Consumes: `docs/adr/0032-gpu-command-ring-and-pipelined-present.md` (gpud ring = the submit prototype)
  - ADR (to add on acceptance): `docs/adr/0033-soft-real-time-spine.md`

## Status at a glance

- Phase 1 — **Waitset** kernel object (wait-on-multiple): proposed.
- Phase 2 — **Timeline fence** kernel object: proposed.
- Phase 3 — **`nexus-driverkit`** userland lib (submit ring + fence + backpressure + buffers): proposed.
- Phase 4 — consumers (gpud reactive on DriverKit; windowd present-scheduler on a waitset): proposed.
- Phase 5 — **clean idle/deadline** (retire the degenerate-spin): proposed (last; riskier).

## Context

Both device tracks name the **same** sync contract — "timeline fences + **waitsets** +
deadlines (**avoid busy-wait**)" — and NexusGfx names the present contract — "compositor
presents … using fences + **vsync spine**". Today the OS has timer objects + QoS +
`timed` (RFC-0023) but **no waitset and no timeline fence**, so:

- A service that needs to be responsive to commands **and** self-paced (e.g. a present
  loop, or any DriverKit device-class server) has no way to wait on *multiple* sources.
  The gpud spin-demo worked around it with a recv **timeout** as a clock, which cannot
  pace deterministically (the degenerate-spin scheduler path resets `now+interval` every
  iteration → ~3.6–12 Hz instead of 120 Hz). See ADR-0032.
- The DriverKit "submit + fence + backpressure" contract (`TASK-0280`) has no shared
  implementation; gpud's multi-entry ring (ADR-0032) is a one-off prototype of it.

This RFC defines the missing spine so pacing, cross-device submit, and present scheduling
stand on first-class primitives instead of per-service hacks.

## Key design insight (why this fits the existing kernel cheaply)

Timers already "fire" by **sending a message to a notify endpoint** and waking that
endpoint's recv-waiter (`HartTimers` + `process_expired_timers`). Endpoints and timers
therefore **both signal via endpoints**. So a **waitset is just "wait on N endpoints,
wake on the first to deliver"** — it reuses the existing router / recv-waiter / wake /
deadline machinery; no new signalling mechanism. A **timer member** gives a waitset
deterministic pacing (the timer's *fixed* cap-deadline drives it via the working
`process_expired_timers` path — not the recv-timeout-reset path that fails today).

## Goals

- A **Waitset** object: block on a set of endpoints (incl. timer-notify and
  fence-notify endpoints); wake on the first ready; bounded; deterministic.
- A **Timeline fence** object: monotonic `u64` value; signal advances it; a wait for
  `target` is satisfied when `value ≥ target`; integrates with the waitset.
- A shared **`nexus-driverkit`** lib: bounded submit ring (per-slot lifecycle +
  backpressure), fence-based completion, buffer/budget helpers, QoS hints — the cross-
  device (GPU/NPU/VPU/Audio) "submit + fence + buffers" contract.
- Consumers prove it: gpud becomes a reactive DriverKit device; windowd's present
  scheduler paces via a waitset at the display rate; spin-demo self-pacing retired.

## Non-Goals (v1)

- Not a general `poll`/`epoll` with edge/level config matrix — minimal level-ready set.
- No kernel-side shader/command validation (stays in the device service).
- No SMP affinity/shares changes (TASK-0042 owns that).
- The clean-idle/WFI rework is **designed here but landed last** (Phase 5), behind the
  proven waitset, so the scheduler hotpath is never destabilised mid-build.

## Proposed design (normative)

### 1. Waitset object (Phase 1) — additive, no recv/scheduler hotpath change

- `CapabilityKind::Waitset(WaitsetId)`; kernel object = a bounded `Vec<EndpointId>`
  (**≤ 16 members**) + owner pid.
- Syscalls (free numbers): `SYSCALL_WAITSET_CREATE = 38`, `_ADD = 39`, `_WAIT = 40`.
  - `waitset_create() -> Cap(Waitset)`.
  - `waitset_add(ws_cap, ep_cap)` — add an endpoint (RECV right required); `-ENOSPC` over 16; `-EINVAL` on non-endpoint.
  - `waitset_wait(ws_cap, deadline_ns) -> ready_slot` — return the **first member with a
    pending message** immediately; else register the task as a recv-waiter on **all**
    members + block (`BlockReason::Waitset { ws_id, deadline_ns }`) + `set_wakeup`. On
    wake (any member delivered, or deadline): **deregister from all members**, return the
    ready member index, or `-ETIMEDOUT`. The caller then `ipc_recv`s the ready endpoint.
- Wake path reuses `router.send → pop_recv_waiter → tasks.wake`; the only new bit is
  multi-endpoint waiter registration + on-wake readiness scan + deregistration. **The
  existing single-endpoint recv path is untouched** (this is purely additive).

### 2. Timeline fence object (Phase 2) — additive

- `CapabilityKind::Fence(FenceId)`; object = `{ value: u64, waiters }`.
- `fence_create() -> Cap(Fence)`; `fence_signal(cap, value)` (monotonic: `value =
  max(value, v)`, wakes satisfied waiters); `fence_wait(cap, target, deadline)`. A fence
  exposes a **notify endpoint** so it can be a **waitset member** (unifying completion +
  command + timer waits on one waitset). Bounded waiters.

### 3. `nexus-driverkit` lib (Phase 3) — host-first userland crate

- `SubmitRing`: bounded in-flight slots + per-slot lifecycle + backpressure (generalises
  gpud's `CtrlQueue`: `enqueue`/`harvest`/`alloc_free_slot`/`wait`); completion via a
  timeline fence (or, v1, an endpoint message — both are waitset members).
- `Buffers`: VMO/filebuffer handles, slices, **budgets** (bounded).
- `Qos`: Frugal/Normal/Burst hints on submit (maps to `QosClass`/`timed` windows).
- Device-specific code shrinks to **MMIO/command-encoding/reset only**; the ring,
  fences, budgets, tracing hooks live in the lib (cross-device: GPU/NPU/VPU/Audio).
- **Host-first**: a CPU-mock backend + golden host tests lock the contract before any
  device wiring (per the tracks' extraction rule).

### 4. Consumers (Phase 4)

- **gpud**: `CtrlQueue` reimplemented on `nexus-driverkit::SubmitRing`; completion via a
  fence; gpud is **purely reactive** (blocks on its server endpoint). Spin-demo self-
  pacing retired.
- **windowd present scheduler**: a waitset over `{command-endpoint, vsync-timer-notify}`;
  paces at the display rate (QoS-tiered), drains input per tick (frame-coalesced), submits
  to gpud. Pacing lives in the compositor, not the driver — the present contract.

### 5. Clean idle/deadline (Phase 5) — last, behind the proven waitset

- Fold IPC recv/send deadlines + timer caps into one earliest-deadline source; idle =
  **WFI to the earliest deadline**, woken by the timer/device IRQ; retire the
  degenerate-spin. Makes every timed wait deterministic + low-power, not just timer caps.

## Constraints / invariants (hard)

- **Bounded**: ≤ 16 waitset members; bounded fence waiters; bounded ring depth. Over-limit
  → `-ENOSPC`, deterministic, no partial state.
- **Determinism**: no timing-fluke success; host goldens are the oracle (QEMU timing is not).
- **Additive safety**: Phases 1–4 do **not** modify the existing recv/scheduler/idle
  hotpath; the proven input chain + preemption + mmio present stay green at every step
  (boot regression net: 0 KPGF/PANIC/USER-PF). Only Phase 5 touches the idle path.
- **Ownership/Send-Sync**: kernel objects use typed ids (`WaitsetId`/`FenceId` newtypes);
  `TaskTable`/`Scheduler` stay `!Send`/`!Sync`; no new `unsafe impl Send/Sync`.
- **Capability-gated**: waitset/fence are caps; `waitset_add` requires RECV right on the
  endpoint; no ambient authority.

## Security

- Threat model: waiter/queue exhaustion (DoS), cross-task wake injection, fence value
  rollback. Mitigations: hard bounds + `-ENOSPC`; a task only waits on endpoints it holds
  RECV caps for; fence is monotonic (no rollback); deterministic reject tests.

## Failure model

- `waitset_wait` deadline → `-ETIMEDOUT` (waiter fully deregistered, no leak).
- Over-bound add / create → `-ENOSPC`. Non-endpoint add / bad cap → `-EINVAL`.
- Dropped waitset/fence cap deregisters its waiters (no dangling waiter).

## Proof / validation

> **Kernel host-test reality (important).** Every `neuron` module is `#[cfg(target_os =
> "none")]`, so `cargo test -p neuron` compiles *none* of the kernel and runs **0 tests** on
> the host; the kernel's in-tree `#[cfg(test)] mod tests` never build off-target. The real
> kernel gates are therefore (a) `cargo check -p neuron --target riscv64imac-unknown-none-elf`
> (= `just diag-kernel`) for type/exhaustiveness/borrow checking, and (b) QEMU boot + `KSELFTEST:`
> markers for runtime behaviour. To still get a **deterministic host oracle** for pure data-
> structure logic, the spine's table modules (`waitset.rs`, `fence.rs`) are the *only* ungated
> kernel modules: pure `alloc` + raw `u32` ids, no router/MMIO coupling, so their
> `#[cfg(test)] mod tests` **do** run under `cargo test -p neuron`. See
> `docs/architecture/02-selftest-and-ci.md` § "Host unit tests for kernel logic".

- **Host (oracle)**: `cargo test -p neuron` runs the ungated table tests —
  `waitset::tests::*` (readiness aggregation, member/table bounds, dedup, free) and
  `fence::tests::*` (monotonic signal, satisfied-waiter selection, waiter bounds);
  `cargo test -p nexus-driverkit` — ring lifecycle + backpressure + fence completion
  goldens (CPU mock).
- **OS/QEMU**: markers `KSELFTEST: waitset wake ok` / `waitset timeout ok`,
  `KSELFTEST: fence wait ok` / `fence timeout ok`, `SELFTEST: driverkit submit/fence ok`;
  gpud/windowd boot proof (mmio + virgl): present cadence at display rate, 0 faults;
  `SMP=2`/`SMP=1` reruns green.

## Minimal-v1 vs future deluxe

- **v1**: waitset (level-ready, ≤16, endpoint members incl. timer); DriverKit ring with
  endpoint-message completion + bounded buffers + QoS hint; gpud reactive + windowd
  waitset pacing. Fence object + clean-idle land right after (Phase 2/5).
- **Deluxe (later)**: edge/level config, fence value-based batching, vsync-domain
  multi-display, power-governor integration, vendor-blob isolation (IOMMU).

## Implementation checklist

- [ ] Phase 1: Waitset object + syscalls 38–40 + nexus-abi + host tests (additive).
- [ ] Phase 2: Timeline fence object + nexus-abi + host tests.
- [ ] Phase 3: `nexus-driverkit` crate (ring + fence + buffers + QoS) + host goldens.
- [ ] Phase 4: gpud on DriverKit (reactive) + windowd present-scheduler waitset.
- [ ] Phase 5: clean idle/deadline (retire degenerate-spin) + SMP reruns.
- [ ] ADR-0033 + `docs/architecture` spine doc; header/test audit.
