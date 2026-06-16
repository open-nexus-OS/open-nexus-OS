# ADR-0033: Soft-real-time spine — waitset + timeline fence + DriverKit submit

- Status: Accepted (Phases 1–4); Phase 5 (clean idle) deferred — see below
- Created: 2026-06-16
- RFC: `docs/rfcs/RFC-0033-soft-real-time-spine-waitset-fence-driverkit.md`
- Builds on: ADR-0032 (gpud command ring — the submit-ring prototype this generalises),
  RFC-0023 (QoS / `timed` / timer objects)
- Related: `tasks/TRACK-NEXUSGFX-SDK.md`, `tasks/TRACK-DRIVERS-ACCELERATORS.md`

## Context

Both device tracks name the same sync contract — "timeline fences + **waitsets** +
deadlines (**avoid busy-wait**)" — and NexusGfx names "compositor presents using fences +
**vsync spine**". The OS had timer objects + QoS + `timed` (RFC-0023) but **no waitset and
no timeline fence**, so a service that must be responsive to commands *and* self-paced (a
present loop, any DriverKit device server) had no way to wait on *multiple* sources. windowd
worked around it with a recv **timeout as a clock** + a `NonBlocking` poll loop, which the
kernel's degenerate-spin scheduler path reset every iteration → `spin_hz≈2320`, `present_hz≈25`
instead of a deterministic display rate. And the DriverKit "submit + fence + backpressure"
contract had no shared implementation — gpud's multi-entry ring (ADR-0032) was a one-off.

## Decision

Build a first-class soft-real-time spine, additively (no recv/scheduler hotpath change in
Phases 1–4; only Phase 5 touches the idle path, and it is deferred):

### Phase 1 — Waitset kernel object (syscalls 38–40) — landed

Key insight: timers already "fire" by **sending to a notify endpoint** and waking its
recv-waiter, so endpoints and timers **both signal via endpoints**. A **waitset** is therefore
just "wait on N endpoints, wake on the first ready" — it reuses the existing router /
recv-waiter / wake / deadline machinery; no new signalling. A **timer member** gives a waitset
deterministic pacing via the timer's *fixed* cap-deadline (`process_expired_timers`), not the
recv-timeout-reset path. `CapabilityKind::Waitset(u32)`, `source/kernel/neuron/src/waitset.rs`
(`WaitsetTable`, ≤16 members, dedup), `BlockReason::Waitset`, `Router::pending` (non-consuming
readiness probe), `waitset_create/add/wait`. The single-endpoint recv path is untouched.

### Phase 2 — Timeline fence kernel object (syscalls 41–43) — landed

`CapabilityKind::Fence(u32)`, `source/kernel/neuron/src/fence.rs` (`FenceTable`: monotonic
`u64` value, bounded waiters, one-per-pid), `BlockReason::Fence`. `fence_signal(v)` advances
monotonically (`value = max(value, v)`) and wakes every waiter the new value satisfies;
`fence_wait(target, deadline)` blocks until `value ≥ target`. The completion/ordering
primitive for the submit ring.

### Phase 3 — `nexus-driverkit` lib — landed

`source/libs/nexus-driverkit/` (pure, `no_std`, allocation-free, `forbid(unsafe_code)`):
`SubmitRing` (≤32-slot busy-bitmask + round-robin alloc + backpressure + monotonic tickets +
a `completed()` count a fence mirrors), `Qos {Frugal,Normal,Burst}`, `BufferBudget` (bounded
bytes + count). It is the device-agnostic generalisation of gpud's `CtrlQueue`; a device
server shrinks to MMIO / command-encoding / reset.

### Phase 4 — consumers — landed

- **gpud** `CtrlQueue` reimplemented on `nexus_driverkit::SubmitRing` (busy/next_slot/slots →
  the ring; `try_alloc`/`complete`/`is_in_flight`/`abandon`/`reset`). Behaviour-preserving:
  reserve-at-alloc is equivalent to the old reserve-at-publish because every alloc is
  unconditionally followed by publish. Boot-verified: uniform ~64 µs presents, no stall.
- **windowd present pacing**: during animation/damage windowd now **blocks** (woken by the
  already-armed one-shot pacer timer-cap, via the now-enabled timer IRQ) instead of the
  `NonBlocking` poll + `yield_()` self-pace. Boot-verified: `spin_hz` 2320 → **0**.

### Phase 5 — clean idle / retire degenerate-spin — DEFERRED (deliberately)

Folding IPC/timer/fence deadlines into one earliest-deadline idle (WFI-to-deadline) and
retiring the kernel degenerate-spin is the riskiest change: it touches the trap/scheduler
idle hotpath, the degenerate-spin is **load-bearing** (a syscall `Reschedule` with no runnable
task does not currently reach the kmain idle loop, so the self-wake keeps timed waits live),
and a prior naive attempt regressed to 0 presents and was reverted. The RFC scoped it to land
**last, behind the proven waitset**. Its user-visible payoff is **already banked** — windowd's
`spin_hz` is 0 via the Phase-4 timer-cap blocking, so retiring the kernel fallback is now a
low-payoff power/determinism cleanup, not a responsiveness fix. It is deferred to an isolated,
boot-tested change rather than bundled into this spine.

## Consequences

- Pacing, cross-device submit, and present scheduling stand on first-class primitives instead
  of per-service recv-timeout hacks. The DriverKit contract is locked in a real consumer (gpud).
- **Host is the oracle** for the pure logic: `waitset.rs`/`fence.rs` are the only un-gated
  kernel modules (pure `alloc` + raw ids) so `cargo test -p neuron` runs them (8 + 8 tests);
  `cargo test -p nexus-driverkit` runs the ring/qos/buffer goldens (21). QEMU markers prove the
  syscall integration: `KSELFTEST: waitset wake/timeout ok`, `fence wait/timeout ok`. See
  `docs/architecture/02-selftest-and-ci.md` § "Host unit tests for kernel logic" for why the
  rest of the kernel runs 0 host tests and the riscv `cargo check` + QEMU markers are the gates.
- The full windowd waitset present-scheduler (a waitset over {command, vsync-timer,
  gpud-completion-fence}) earns its multi-source keep only once gpud signals a completion fence
  (`ring.completed()` → `fence_signal`) that windowd `fence_wait`s on — a follow-up, since the
  slowdown is already fixed without it.

## Alternatives considered

- **`VIRTIO_GPU_FLAG_FENCE` for present completion** (ADR-0032): rejected — QEMU's fence
  completion broke the used-ring/response model + hung post-scanout.
- **recv-timeout as the pacing clock**: rejected — the degenerate-spin resets the deadline
  each iteration, so it cannot pace deterministically (~3.6–12 Hz, not 120 Hz).
- **A general `poll`/`epoll` matrix**: out of scope — minimal level-ready waitset only.
