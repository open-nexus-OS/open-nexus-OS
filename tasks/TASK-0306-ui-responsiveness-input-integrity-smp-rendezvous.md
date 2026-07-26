---
title: TASK-0306 UI responsiveness — input integrity, SMP rendezvous, BKL hold reduction
status: In Progress (2026-07-26) — Phase 1 done, Phase 2 partial, Phase 3 open
owner: @ui @kernel
created: 2026-07-26
links:
  - Evidence: build/logs/manual--2026-07-26T17-50-26/uart.log
  - Soft-real-time spine: docs/rfcs/RFC-0033 (phases 1+2 done)
  - Compositor boundary: docs/rfcs/RFC-0067-windowd-compositor-service-boundary-*.md
  - Playbook: CLAUDE.md
---

## Context

The UI "barely reacted": the control center would not open, the launcher
ignored taps. Measured from a real interactive boot, not guessed. Three
independent causes, none of them the design:

### 1. ACK frames are dropped, and the drop is reported as success

Counted in the log, so the claim is precise:

| marker | count |
|---|---|
| `FAIL desktop input send` | **0** |
| `FAIL desktop input (no event channel)` | 0 |
| `FAIL nonce event send` | 0 |
| `desktop input routed` | 10 |
| **`FAIL desktop event send`** | **2** |
| `STALL present stuck` | 1 |
| `surface present stale` | 4 |

**No tap was ever lost.** The input path already does the right thing —
`send_input_frame` retries a tap up to 400 times with a `yield_()` between
attempts (`app_window.rs:921`).

What is lost is the **ACK** path — present / create / destroy acks:

```rust
// desktop_surface.rs:151, app_window.rs:267 and :291 — all three identical
match nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
    Ok(_) => true,
    Err(_) => {
        let _ = debug_println("WINDOWD: FAIL desktop event send");
        true // channel exists — no shared-endpoint fallback
    }
}
```

**One attempt, no retry, and `true` returned on failure.** The asymmetry is
the whole bug: the frame a client merely *reads* is retried 400 times; the
frame a client is **blocked waiting on** is fired once and discarded.

The consequence is in the timestamps:

```
apphost: submitted 10 layers
[53.258318]  WINDOWD: FAIL desktop event send          <- present-ack dropped
[53.258761]  windowd: loop hz=56 apply=13 present=49
[53.259526]  windowd: STALL present stuck 504ms        <- 1.2 ms later
[53.637176]  WINDOWD: surface present stale id=1 (ignored)
[54.268122]  windowd: loop hz=17 ...                   <- 56 Hz -> 17 Hz
```

A dropped ack does not lose a pixel — it leaves the client waiting for a
reply that will never arrive, until its own timeout fires. That is what the
user experiences as "the UI does not react": not a lost click, a **stalled
client**.

### 2. One hart is lost on every boot, and the boot hart causes it

```
17:50 run: KGATE: smp bringup DEGRADED expected=0xf got=0x7   (hart3 missing)
16:47 run: got=0xb                                            (hart2 missing)
hart3 missing (stage=0 hsm_err=0x0 hsm_state=0)
```

A *different* hart each time ⇒ a race, not a dead hart. `hsm_state=0` is SBI
STARTED; `stage=0` means the hart never reached `kmain_secondary`, whose very
first statement is `stage.store(1)`.

The boot hart waits for it like this:

```rust
// core/smp/bringup.rs:228 — wait_for_online_mask
loop {
    if cpu_online_mask() & expected_mask == expected_mask { return true; }
    if time::read() >= deadline { return false; }
    core::hint::spin_loop();          // <-- HOT SPIN, up to 500 ms
}
```

**The boot hart hot-spins on the vCPU it is waiting for.** Under TCG the
emulator round-robins vCPUs; a spinning vCPU burns its whole quantum and
starves the hart trying to come up. The kernel already knows this — 400 lines
away, `kmain_secondary` parks in WFI with the comment *"under icount/TCG a
spinning vCPU steals whole scheduler quanta from the boot hart"*. The
bring-up wait never got that treatment.

Cost: **25 % of the machine's compute is missing**, which is exactly the
compute that would absorb cause 3.

### 3. IPC syscalls hold the global kernel lock for 10–25 ms

```
long ecall nr=14 25ms   IPC_SEND_V1
long ecall nr=0  21ms   YIELD
long ecall nr=26 13ms   IPC_RECV_V2   (4x over 10 ms)
KINIT: bkl burst max_wait=22722us max_hold=13ms gt10ms=13
```

A 60 Hz frame is 16.7 ms. One send can eat a whole frame. The convoy is
visible in the boot: 17 service selftests start inside a 241 ms window and
drain over 2.6 s — `rngd` spends 1464 ms on ONE test, `abilitymgr` 2340 ms on
ONE test. They are not working, they are queued. (`init_caps`, which does not
share the path, does 62 tests in 251 ms.)

This is the open remainder of the RFC-0033 soft-real-time work.

### Symptoms that follow from the above

- compositor loop: median 65 Hz, **p10 = 20 Hz, min = 14 Hz**
- `windowd: STALL present stuck 504ms`
- present cost avg 4.49 ms of which **84 % is CPU-side enqueue** (`irqw=0` —
  the GPU is not the bottleneck); worst single entry 45.8 ms

### Not bugs (checked, so nobody re-investigates)

- The four `input tap miss` entries are all on empty desktop.
- `max_hold=38ms / gt10ms=1119` in the 16:39 log was host load (parallel
  cargo builds), not the code.

## Goal

A UI that always responds to input, on a machine that uses all its cores.

1. **Delivery integrity (windowd)**: one delivery-class contract instead of
   per-call-site guesswork. A frame a client BLOCKS on (present/create/destroy
   ack, input tap) is retried; a frame the next one supersedes (hover motion,
   region push) may be dropped. No send ever reports success it did not have.
2. **SMP rendezvous (kernel)**: bring-up is deterministic — all harts online,
   every boot. Stop starving the hart we are waiting for.
3. **BKL hold reduction (kernel)**: locate and shorten the 10–25 ms holds in
   `ipc_send_v1` / `ipc_recv_v2` / `yield`.

## Non-Goals

- gpud enqueue cost (finding 4) — separate track, needs its own measurement.
- Whole-scene re-emit / remount frequency — RFC-0067 follow-up.
- Anything about the greeter design (TASK-0305, done).

## Constraints / invariants

- **No fake success**: a send that did not deliver must not return `true`.
- **Bounded**: the retry queue has a hard cap and a fail-visible overflow
  marker; no unbounded growth in a non-freeing allocator.
- **Determinism**: the SMP fix must not depend on a timeout guess. Prove it
  over repeated boots, not one lucky run.
- Marker strings are contracts — new ones go in the docs together.

## Stop conditions (Definition of Done)

- [x] Every ack path is `Delivery::Blocking` (a kernel park with a deadline,
      not a retry loop) and returns the truth;
      `WINDOWD: FAIL …` only ever prints when delivery really failed. Host test.
- [x] 0 `FAIL desktop event send` across 8 boots (was 2); `surface present
      stale` 4 → 0. One `STALL present stuck` remains in 1 of 8.
- [~] All 4 harts on **7 of 8** boots (was 0 of 3). NOT deterministic — the
      residual is provably below our entry stub (`asm=0`, SBI says STARTED).
- [ ] BKL: not started (Phase 3).
- [ ] `just check`, `just diag`, `just test-host` green.
- [ ] Visible boot: control center opens, launcher reacts.

## Plan

1. **Phase 1 — delivery integrity (windowd).** The direct cause of the
   reported symptom, and it is iteration ground (no kernel).
2. **Phase 2 — SMP rendezvous (kernel).** WFI-based wait instead of the hot
   spin; secondary signals the boot hart. Biggest compute win.
3. **Phase 3 — BKL holds (kernel).** Instrument first: find *where* inside
   send/recv/yield the milliseconds go, then move that work out of the lock.
4. **Phase 4 — measure the same boot again** and compare the same numbers.
5. **Phase 5 — hit targets (added 2026-07-26).** Two of the three reported
   symptoms ("control centre broken", "launcher did not react") are not a
   delivery bug at all: the controls are physically too small to hit. Found
   while reading the boot's input trace, so it belongs in this task.

## Evidence

### Phase 1 — delivery integrity (done)

`Delivery::{Blocking, Coalescing}` lives in `client_surface.rs` (the
host-tested contract module; `compositor/**` does not compile on host, so a
test next to the code could never run). `send_client_frame` replaces three
copies of the same one-shot send, and returns the truth.

The payoff was already in the tree: `compositor/mod.rs` has the right
recovery for every ack — `reply_and_close_wait` / `server.send`, both
`Wait::Blocking`, neither losable. The `true`-on-failure return is what kept
that recovery from ever running. Now a lost ack retries, and if it still
fails, the caller falls back to a blocking send.

Measured over 8 boots after the change vs the reported run before it:

| | `FAIL desktop event send` | `STALL present stuck` | `surface present stale` |
|---|---|---|---|
| before (user run 17:50) | 2 | 1 | 4 |
| after (8 boots) | **0** | 1 (one boot) | **0** |

Caveat, stated because it matters: those 8 boots were not driven
interactively, so they exercise less than the reported session did. The
structural claim is the stronger evidence — a dropped ack now needs the
retry AND the blocking fallback to both fail.

### Phase 2 — SMP rendezvous (improved and localized, NOT eliminated)

Two changes:

1. `wait_for_online_mask` parks in WFI on a self-armed SBI timer instead of
   spinning. It also bumps liveness, which removes the documented
   "watchdog: no progress right after cpu0 sched loop" hazard.
2. `BRINGUP_ASM_SEEN`, written by the entry stub as its very first
   instruction, so `stage=0` can be told apart from "never entered".

Measured, all-4-harts rate:

| | boots with all 4 harts |
|---|---|
| before | **0 / 3** (user run + two of mine) |
| after | **7 / 8** |

**It is not deterministic, and I am not calling it fixed.** The one residual
failure reads `asm=0 stage=0`: the hart never executes the *first
instruction* of our entry stub, while SBI reports it STARTED
(`hsm_err=0x0 hsm_state=0`). That is below our code — OpenSBI/QEMU accepted
`hart_start`, claims the hart is running, and never transferred control. The
existing retry cannot help: SBI then answers `ALREADY_AVAILABLE`.

One methodology note worth keeping: the first version of `BRINGUP_ASM_SEEN`
was a plain `static [u64; N]`, which can be placed in `.rodata` (flags `AM`,
no `W` in this image) — the asm store would be dropped and the instrument
would read 0 forever, "confirming" whatever you hoped. It is an
`AtomicU64` now, whose `UnsafeCell` forces writable placement. The `asm=0`
result above is from the corrected instrument.

### Phase 1b — the retry loop was still polling (user challenge, 2026-07-26)

The first fix removed the symptom but kept the wrong architecture: 400
`ipc_send_v1(NONBLOCK)` attempts with a `yield_()` between them. That is
polling, and it is built on the very syscall measured at up to **21 ms** under
lock contention — up to 400 of them per frame.

The kernel already implements the reactive rendezvous, and the ABI already
documents it:

> *"When `sys_flags` does not include NONBLOCK, the kernel may block until the
> queue has capacity or the optional `deadline_ns` expires."*

`syscall/api/ipc_msg.rs` backs that with `register_send_waiter(endpoint)` /
`BlockReason::IpcSend { endpoint, deadline_ns }`, and the receive path pops the
waiter the moment capacity appears. So `Delivery::Blocking` is now **one
syscall that parks and is woken**, with a 16 ms (one frame) absolute deadline
bounding the compositor's exposure to a wedged client. On expiry the caller
still falls back to the blocking reply path, which cannot be lost.

`Delivery::attempts()` became `Delivery::deadline_ns()` — the policy is a
deadline, not a retry budget.

Why it matters beyond elegance: the interactive run at 20:45 recorded
`FAIL desktop input send = 1`. A tap survived 400 retries' worth of yielding
and was still lost. A parked sender does not have that failure mode.

### Phase 5 — hit targets: the pills were unhittable, not unresponsive (done)

The boot trace shows a press at (1262, 52) reaching **no handler at all**. That
looked like another lost event; it is not. The control-centre pill is painted
`height(28)` in a `SHELL_TOPBAR_H = 36` bar, so the press was 20 px below a
29x28 target. Every touch-target guideline puts the minimum at 44 px. Two of
the three symptoms the user reported are this, not delivery.

`.hitSlop(n)` existed in the DSL catalog (`nexus-dsl-core::registry`, id 39),
type-checked, and did **nothing** — there was no `apply_modifier` arm, no field
on `Mods`, no field on `FlexItem` or `LayoutBox`, and no reader in the hit
test. It was documented under "Still declared but NOT implemented". Wired end
to end now:

- `FlexItem::hit_slop` / `LayoutBox::hit_slop` carry it; the layout engine
  copies it at all four construction sites. **`rect` is untouched** — slop
  grows the input rect only, never a pixel of paint. A test asserts both
  halves (painted <= 36, target >= 44).
- `interact::hit_scrolled` runs two passes. An **exact hit always beats a slop
  hit**, whatever the tree order — otherwise a generous slop steals the tap
  that landed squarely on its neighbour. Among slop candidates the **nearest**
  wins (squared edge distance), which is what a user means when they aim
  between two enlarged targets.
- The six top-bar pills in both shell pages carry `.hitSlop(2)` (2 spacing
  steps = 8 px per side → 28 + 16 = 44). The full-bleed panel scrims
  deliberately do not: slop on a screen-sized rect is meaningless.

The test found a real inconsistency on the way: the clock pill was painted
**24 px**, not 28, because it wrapped `TopBarPill` without a height — a 40 px
target even with slop. Normalized to 28 like every other pill in the bar.

Not claimed: the launcher and dock tiles were not audited for the same defect.
The mechanism is now there for them.

Structure gate: `LayoutBox` / `LayoutResult` / `ScrollDamage` moved out of
`engine.rs` (1406 -> 1254 LOC) into `layout/src/boxes.rs`. The ratchet asked
for a split rather than growth and it was right — those are the data contract
the renderer and hit-tester read, not engine algorithms. Baseline ratcheted
DOWN for both touched files.

Proof: `tests/dsl_apps_conformance/tests/shell_hit_targets.rs` (2 tests, they
compile the REAL shell app and lay it out at 1280x800) plus four unit tests on
the precedence rules in `interact.rs`. `just check`, `just diag`
(host+os+kernel), `just test-host` green.

### Phase 6 — two regressions from this task's own fixes (found by the user, fixed)

The boot after Phase 5 was faster (`present us avg=1621 max=8683`, down from
`avg 5100 max 313800`; `entmax_us` 69416 -> 3015) but two controls misbehaved.
Neither was the hit-slop work.

**1. The launcher button cycled the shell profile.** `runtime/input.rs` carried
a fixed 28x28 bottom-left hotspot that called `cycle_shell()` and set
`window_consumed_press` BEFORE any window or desktop routing. ADR-0035
introduced it as "the current dev trigger", justified by that corner being
"always wallpaper". That premise died when the desktop taskbar (56 px) put a
36 px launcher glyph there — about 20x18 px of the button switched the shell
instead of opening the launcher. Worse, the resulting tablet re-mount
(`apphost: profile old app dropped` / `APPHOST: profile remounted`, five times
in the log) replaced the whole top bar, so every control the user aimed at next
was genuinely no longer there. That is the reported "and then nothing works".

Deleted, along with the now-unreferenced `cycle_shell()`. Nothing is lost:
`CONTROL_SHELL_PROFILE` (DSL `settings.set` -> app-host ->
`set_shell_profile_wire`) is the wired production path. An invisible UI
affordance inside the compositor's input router is the boundary violation this
codebase explicitly forbids. ADR-0035 marked superseded.

**2. `WINDOWD: FAIL desktop input send` came back — caused by Phase 1.** Phase 1
replaced a 400x yield-retry loop with one blocking send on a 16 ms deadline,
and the doc justified the bound with "the caller falls back to the blocking
reply path". That fallback is real for ACKs and does NOT exist for input:
`send_desktop_input_kind` logs and returns, and the tap is gone. The bound was
also sized to a 60 Hz frame while the log shows the loop dropping to **13 Hz**
(77 ms) — every failure line sits next to one.

Split the policy by whether a recovery path exists rather than by who is
waiting: `Delivery::Blocking` (acks, fallback exists) keeps 16 ms;
`Delivery::Critical` (taps, no fallback, user intent that nothing can
reconstruct) gets 250 ms — three of the worst iteration we have actually
measured. Still bounded, so a dead client cannot wedge the compositor forever.
Pinned by `a_tap_outlives_a_slow_frame`, which asserts against the measured
77 ms rather than a made-up number.

Lesson recorded: a deadline is only as good as its escape hatch. Reusing one
budget for two call sites was wrong the moment one of them had no fallback.

### Phase 3 — BKL holds: instrumented, target identified, fix NOT applied

The plan said "instrument first: find *where* inside send/recv/yield the
milliseconds go, then move that work out of the lock." The first half is done
and it changed the answer twice, which is exactly why it was worth doing.

**What the numbers looked like going in.** `sweep steady max=17341us tasks=54
calls=4527 mean=101us`. The obvious reading — an O(tasks) scan that is simply
too long — cannot be right: 171x the mean at the same task count is not a cost
curve, it is an outlier. So the sweep was split into its two halves.

**Round 1 — wakes vs scan.** `wakes` / `wakeus` added to the sweep report:

    sweep bring-up max=6914us tasks=47 wakes=1 wakeus=206

One wake, 206 us, 3 % of the total. Reading that alone, the scan looked
guilty (97 %, 143 us per task for a field compare).

**Round 2 — which iteration.** A per-iteration maximum was added, because
"the scan is uniformly slow" and "one iteration stalled" have opposite fixes:

    sweep bring-up max=1155us tasks=45 wakes=1 wakeus=1056 itermax=1057us
    sweep bring-up max=5042us tasks=48 wakes=1 wakeus=83   itermax=4460us

The first says the whole sweep IS the wake. The second says one iteration ate
4.46 ms and it was NOT the wake — a `_ => {}` arm cannot do that, so the wall
clock absorbed an interrupt or a host-side vCPU deschedule under TCG. **The
peak is not ours.** Four boots produced maxima of 6914 / 1155 / 5042 / 5986 us
at the same task count; that spread is the artifact, not a workload.

**Round 3 — the aggregate, which IS ours.** Timing `smp::request_resched`
(`sbi::send_ipi`) directly:

    wake ipi max=2313us mean=63us n=9029
    wake ipi max=6263us mean=52us n=1448

A cross-core wake IPI costs ~50-60 us on average and runs INSIDE the held BKL
from `Tasks::wake`. At 1448-9029 per boot that is 75-570 ms of lock time spent
in firmware-mediated IPIs. And with the measurement noise removed it is usually
the peak too: `max=5986us wakes=1 wakeus=5947` — 99 % of the worst sweep was
one wake. `Scheduler::purge` was cleared by inspection: `sched::Task` is
`{id, qos}`, so 4 CPUs x 4 short queues cannot cost a millisecond.

**Target: get `sbi::send_ipi` out of the BKL.** `request_resched` already sets
`RESCHED_PENDING` + the wake hint synchronously (plain atomics); the IPI only
kicks a hart out of WFI. Record the target CPU in a pending mask and flush the
actual IPIs once, after `KernelGuard` drops. That coalesces duplicates and
removes the firmware round-trip from the lock.

**Not applied, deliberately.** A correct flush has to run on EVERY kernel exit
path, not just the syscall one. Miss one and a hart sits in WFI with
`RESCHED_PENDING` set until its next timer tick — the exact failure class that
produced the documented early-boot fleet collapse. That deserves its own pass
with the bring-up boot gate watched, not the tail of this session.

**Kept in tree:** the wake/wake-IPI attribution (proportionate: two CSR reads
per cross-core wake, and it is the metric the fix must drive down).
**Removed again:** the per-iteration probe. It answered its question, and it
was not free — the sweep mean read 101 us before it, 129-189 us with it, and
40 us after removing it. A diagnostic that distorts the hot path it measures
does not get to stay once it has spoken.

`sched_task.rs` hit 604 LOC (> 600, not grandfathered) as the reports grew, so
the telemetry ops moved to `syscall/api/sched_telemetry.rs`. Scheduling
syscalls stay scheduling syscalls.

### Open finding — present cost is the largest remaining lever (measured, unfixed)

Measured on the same boot, ranked above Phase 3 by impact and recorded here so
the next session starts from numbers rather than a guess:

    avg 5.1 ms   max 313.8 ms   89 % of it CPU-side enqueue
    irqw=1                      → the GPU is NOT the bottleneck
    entmax_us=69416             → ONE ring entry took 69 ms

69 ms for a single entry is not command building, it is waiting. Traced to
`virtqueue::alloc_free_slot` (`source/drivers/gpud/src/backend/virtqueue.rs`):
when the 32-slot control ring has no free slot it parks on the GPU completion
IRQ — correctly reactive, not a spin. The ring is not filled by one present
(~3.3 entries each) but **across** presents, as completions fall behind.

This paces the compositor loop (observed 16-60 Hz), which paces the coalesced
pointer stream, which is what "the mouse is not smooth" actually is. Next step
is a measurement, not a patch: instrument where completions go, before
touching `RING_SLOTS` or the batching.
