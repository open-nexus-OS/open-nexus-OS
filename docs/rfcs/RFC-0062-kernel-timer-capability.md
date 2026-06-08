# RFC-0062: Kernel Timer Capability — Deterministic Tick Source for Service Pacing

- Status: Draft
- Owners: @kernel @runtime @ui
- Created: 2026-06-06
- Links:
  - Analysis: `docs/dev/perf/KERNEL-TIMER-CAPABILITY-ANALYSIS.md`
  - Phase 7 dependency: `docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md`
  - Present feedback: `docs/rfcs/RFC-0059-...` §Phase 6d

## Status at a Glance

- Phase D.0 (RFC seed): ✅
- Phase D.1 (VSync cleanup via `Wait::Timeout` workaround): ✅
- Phase D.2 (Kernel timer capability — syscalls + cap system + IRQ): ⬜
- Phase D.3 (windowd real timer event path): ⬜
- Phase D.4 (Validation — kernel unit + QEMU integration): ⬜
- Phase D.5 (Present feedback + frame pacing closure): ⬜

## Scope boundaries

- **This RFC owns**: `CapabilityKind::Timer`, `timer_create`/`timer_set`/`timer_cancel` syscalls, per-hart deadline queue, timer IRQ processing with coalescing, `OP_TIMER_FIRED` wire format, windowd timer integration, present completion feedback channel
- **This RFC does NOT own**: General-purpose alarm service (userspace alarmd), high-resolution timers beyond SBI `set_timer` granularity, SSTC extension, timer-based scheduling preemption

## Context

The kernel already has deadline-based wakeup infrastructure:
- HAL: `Timer::set_wakeup(deadline_ns)` → SBI `set_timer`
- Syscall: `deadline_ns` field in send/recv IPC
- BlockReason: `IpcRecv { deadline_ns }`, `IpcSend { deadline_ns }`
- Timer IRQ: checks expired deadlines and wakes blocked tasks

Phase D.1 uses this via `Wait::Timeout(8.3ms)` as a workaround: windowd blocks on IPC recv with a deadline, and when the deadline expires, the kernel returns `TimedOut` — which windowd treats as an animation tick signal. This works but:
- Timeout errors are not timer events — polluting the error path
- No drift-free periodic rearm — each `Wait::Timeout` computes `now + interval`, accumulating drift
- No coalescing — if the thread doesn't drain the timeout quickly, multiple timeouts pile up
- Not a capability — cannot be transferred, shared, or lifecycle-managed
- No present completion correlation — frame pacing is blind to scanout status

This RFC defines a first-class Timer capability object in the kernel, making timer events a proper IPC primitive.

## Architecture

```
windowd                              kernel                              gpud
   │                                    │                                  │
   │ timer_create(notify_ep, PERIODIC)   │                                  │
   │──────────────────────────────────►  │                                  │
   │                 timer_cap          │  TimerState {                     │
   │                                    │    owner, notify_ep,              │
   │                                    │    period_ns, seq, armed          │
   │                                    │  }                               │
   │                                    │                                  │
   │ timer_set(timer_cap,               │                                  │
   │   first_deadline, interval)        │                                  │
   │──────────────────────────────────►  │                                  │
   │                                    │  deadline_queue.insert(           │
   │                                    │    deadline, timer_cap)           │
   │                                    │  set_wakeup(earliest_deadline)    │
   │                                    │                                  │
   │   ... time passes ...              │                                  │
   │                                    │  ◄── TIMER IRQ                   │
   │                                    │  now = Timer::now()              │
   │                                    │  while queue.min <= now:         │
   │                                    │    t = queue.pop()               │
   │                                    │    enqueue_event(                 │
   │                                    │      t.notify_ep,                │
   │                                    │      OP_TIMER_FIRED {            │
   │                                    │        timer_id, seq, missed,    │
   │                                    │        deadline_ns, fired_ns     │
   │                                    │      })                          │
   │                                    │    if t.periodic:                │
   │                                    │      t.deadline += interval      │
   │                                    │      t.seq += 1 + missed         │
   │                                    │      t.missed = 0                │
   │                                    │      queue.insert(t)             │
   │                                    │  set_wakeup(queue.min)           │
   │                                    │                                  │
   │  ◄── OP_TIMER_FIRED ────────────── │                                  │
   │  tick(now)                         │                                  │
   │  flush_pending_damage()            │                                  │
   │                                    │                                  │
   │  build CommandBuffer ─────────────►│─────────────────────────────────►│
   │                                    │                                  │ render
   │                                    │                    OP_PRESENT_DONE
   │  ◄── PRESENT_DONE ──────────────── │◄─────────────────────────────────│
   │  pacing.record(done_ns - submit)   │                                  │
```

## Timer Capability

```rust
// CapabilityKind variant
pub enum CapabilityKind {
    // ... existing variants ...
    Timer { timer_id: u32 },
}

// Rights
pub struct Rights(u32);
impl Rights {
    pub const TIMER_SET:    Rights = Rights(1 << 4);  // arm/disarm
    pub const TIMER_CANCEL: Rights = Rights(1 << 5);  // cancel
    pub const TIMER_TRANSFER: Rights = Rights(1 << 6); // send to another process
}
```

## Timer State Machine

```
                    timer_create()
  [NonExistent] ──────────────────► [Disarmed]
                                         │
                              timer_set(deadline, interval)
                                         │
                                         ▼
                                    [Armed]
                                    │       │
                          interval>0│       │interval==0
                                    │       │
                                    ▼       ▼
                              [Periodic]  [OneShot]
                                    │       │
                          timer IRQ │       │ timer IRQ
                          rearms    │       │ auto-disarm
                                    │       │
                                    ▼       ▼
                              [Armed]    [Disarmed]
                                         │
                              timer_cancel() / cap_close()
                                         │
                                         ▼
                                    [Freed]
```

## Syscall ABI

### timer_create

```rust
/// Create a Timer capability bound to a notification endpoint.
/// The endpoint receives OP_TIMER_FIRED events when the timer expires.
///
/// # Arguments
/// - notify_ep: handle to the endpoint that will receive timer events
/// - flags: TIMER_PERIODIC or 0 for one-shot
///
/// # Returns
/// - Ok(timer_handle): new Timer capability with SET|CANCEL|TRANSFER rights
/// - Err: InvalidHandle, PermissionDenied, ResourceExhausted
pub const SYSCALL_TIMER_CREATE: usize = 0x20;

fn timer_create(notify_ep: Handle, flags: u32) -> Result<Handle, TimerError>;
```

### timer_set

```rust
/// Arm the timer. The timer fires at `first_deadline_ns` (absolute monotonic
/// nanoseconds). If `interval_ns` is non-zero, the timer is periodic and
/// re-arms automatically at `first_deadline_ns + n * interval_ns`.
///
/// Periodic re-arm is drift-free: next_deadline += interval_ns.
/// Under backpressure (event not yet consumed), missed ticks coalesce
/// into the `missed` counter.
///
/// # Returns
/// - Ok(()): timer armed
/// - Err: InvalidHandle, PermissionDenied(no SET right), AlreadyArmed
pub const SYSCALL_TIMER_SET: usize = 0x21;

fn timer_set(timer: Handle, first_deadline_ns: u64, interval_ns: u64) -> Result<(), TimerError>;
```

### timer_cancel

```rust
/// Disarm the timer without destroying the capability.
/// No OP_TIMER_FIRED event is delivered for a cancelled timer.
///
/// # Returns
/// - Ok(()): timer disarmed
/// - Err: InvalidHandle, PermissionDenied(no CANCEL right)
pub const SYSCALL_TIMER_CANCEL: usize = 0x22;

fn timer_cancel(timer: Handle) -> Result<(), TimerError>;
```

## Timer Event Wire Format

```
Byte offset  Field         Type    Description
─────────────────────────────────────────────────
0            opcode         u8     OP_TIMER_FIRED = 0x30
1-4          timer_id       u32    Timer capability id
5-8          seq            u32    Monotonic fire count
9-12         missed         u32    Coalesced ticks since last delivery
13-20        deadline_ns    u64    Absolute deadline that fired
21-28        fired_ns       u64    Monotonic time when IRQ was processed

Total: 29 bytes, fixed-length, deterministic.
```

Coalescing rule: if a periodic timer fires while its previous `OP_TIMER_FIRED` event is still queued:
- Increment `missed` counter
- Do NOT enqueue another event
- Rearm for next period
- The receiver sees `missed > 0` and knows ticks were dropped

## Timer Queue (per-Hart)

```rust
/// Sorted by deadline_ns, ascending.
/// Operations: insert O(log n), pop_min O(log n), remove O(log n).
struct TimerQueue {
    entries: BTreeMap<u64, Vec<TimerId>>,  // deadline_ns -> timer_ids
}

impl TimerQueue {
    fn insert(&mut self, deadline_ns: u64, timer_id: u32);
    fn remove(&mut self, deadline_ns: u64, timer_id: u32);
    fn pop_expired(&mut self, now: u64) -> Vec<TimerId>;
    fn earliest(&self) -> Option<u64>;
}

/// Per-hart timer state.
struct HartTimers {
    queue: TimerQueue,
    table: BTreeMap<u32, TimerState>,  // timer_id -> state
    next_id: u32,
}

struct TimerState {
    owner_pid: Pid,
    notify_ep: EndpointId,
    deadline_ns: u64,
    interval_ns: u64,
    seq: u32,
    missed: u32,
    armed: bool,
    periodic: bool,
}
```

## IRQ Processing

```rust
fn on_timer_irq(ctx: &mut KernelCtx) {
    let now = ctx.timer.now();
    let expired = ctx.hart_timers.queue.pop_expired(now);

    for timer_id in expired {
        let t = ctx.hart_timers.table.get_mut(&timer_id);
        let event = build_timer_fired_event(t, now);

        // Enqueue event to notify endpoint
        ctx.router.enqueue_ipc(t.notify_ep, &event);

        if t.periodic {
            // Drift-free rearm
            t.deadline_ns += t.interval_ns;
            t.seq += 1 + t.missed;
            t.missed = 0;

            // Check coalescing: if previous event still queued
            if ctx.router.has_pending_event(t.notify_ep, OP_TIMER_FIRED) {
                t.missed += 1;
                // Don't enqueue another event
            } else {
                ctx.hart_timers.queue.insert(t.deadline_ns, timer_id);
            }
        } else {
            t.armed = false;
        }
    }

    // Program hardware for earliest remaining deadline
    if let Some(next) = ctx.hart_timers.queue.earliest() {
        ctx.timer.set_wakeup(next);
    }
}
```

## Determinism and Security

- Absolute deadlines: `first_deadline_ns` is monotonic, not wall-clock.
- Drift-free: `next += interval`, never `now + interval`.
- Coalescing: bounded event queue — at most one pending timer event per timer.
- No timer storms: `missed` counter prevents event flooding under backpressure.
- Capability lifecycle: closing a timer cap disarms and removes it from all queues.
- No cross-process interference: timer events go to the bound endpoint only.
- Bounded state: timer table is fixed-size (configurable, default 64 timers per hart).

## Proof Strategy

### Kernel Unit Tests (D.4)

```rust
// timer_queue.rs
#[test] fn insert_and_pop_in_order() { ... }
#[test] fn remove_middle_entry() { ... }
#[test] fn pop_expired_returns_only_expired() { ... }
#[test] fn queue_empty_after_pop_all() { ... }

// timer_state.rs
#[test] fn periodic_rearm_is_drift_free() { ... }
#[test] fn oneshot_auto_disarms() { ... }
#[test] fn coalesce_increments_missed() { ... }
#[test] fn cancel_disarms_without_event() { ... }
#[test] fn cap_close_removes_from_queue() { ... }
```

### Kernel Integration Tests (QEMU)

```bash
cargo test -p neuron --test timer_integration
```

- `timer_create_periodic_receives_events`: creates periodic timer, verifies N events arrive
- `timer_oneshot_fires_once`: creates one-shot timer, verifies exactly 1 event
- `timer_cancel_no_event`: creates timer, cancels, verifies no event
- `timer_create_fails_no_endpoint`: verify reject path
- `timer_set_fails_no_rights`: verify permission check
- `timer_close_cleans_up`: verify queue removal on cap close

### Windowd Integration (D.3)

- Blocking loop wakes on `OP_TIMER_FIRED` only (not timeout errors)
- Timer is created with right interval for display refresh rate
- No `yield_` or `Wait::Timeout` dependency for animation cadence
- Frame pacing under synthetic input burst: p95 interval within budget

### Present Feedback (D.5)

- gpud emits `OP_PRESENT_DONE` asynchronously after `TRANSFER_TO_HOST + FLUSH`
- `present_id` correlation: windowd matches `PRESENT_DONE` to submitted frame
- Pacing policy: if `done_ns - submit_ns > budget`, reduce quality next frame
- In-flight count accurately tracks outstanding frames

## Implementation Checklist

- [ ] Phase D.2: Kernel timer capability — kernal unit tests pass
  - `cargo test -p neuron timer_queue timer_state`
- [ ] Phase D.2: Syscall wiring — `timer_create`/`set`/`cancel` dispatch
  - `cargo test -p neuron --test timer_integration`
- [ ] Phase D.2: ABI wrappers — `nexus-abi` safe wrappers
  - `cargo check -p nexus-abi`
- [ ] Phase D.3: windowd timer integration
  - `cargo check -p windowd`
- [ ] Phase D.4: QEMU end-to-end timer proof
  - `RUN_UNTIL_MARKER=1 just test-os`
- [ ] Phase D.5: gpud present feedback + windowd pacing closure
  - `cargo test -p gpud -p windowd`
