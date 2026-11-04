<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0096: A wake preempts — a task of a class above the running task's runs at the trap exit, and the soft-realtime chain is Interactive

- Status: Proposed
- Date: 2026-11-04
- Links:
  - Tasks: `tasks/TASK-0351-layer-8-on-the-gpu-an-occluded-window-costs-nothing-bindings-grow-a-fullscreen-a.md` (P5: the fixed cost of a pass)
  - RFCs: `docs/rfcs/RFC-0033-soft-real-time-spine-waitset-fence-driverkit.md` (wake on the first to deliver), `docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md` (the latency contract, the hops a frame pays)
  - Related ADRs: `docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md` (the soft-realtime chain's placement), `docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md` (earliest-deadline timer), `docs/adr/0094-one-kernel-three-executors-an-effect-s-mathematics-is-defined-once-in-nexus-gfx.md` (performance is a series)

## Context

The board's cycle of 2026-11-04 (`build/logs/board--2026-11-04T12-28-47`) priced a desktop pass at
5.8 ms whatever its area (`gpu pass sweep`: base 5.7, 16x16 5.9, full 6.0, the full quad under a
16x16 damage 6.9) and the hover frame's one pass at 13.1 ms — while gpud's own bring-up passes
in the same firmware cost 0.5–0.8 ms (`gpu passes`, `gpu area ok us=830`), and the one case that
does real shading (the blended full-screen quad) added 6.6 ms on top. The GPU was not the fixed
cost. The pass trace said what was: a wait of 1 ms slices woke at 607, 3770, 9896, 11768 and
13069 µs — each slice 0.3–5.1 ms after its deadline.

The kernel delivers a device interrupt (`irq::deliver`) and a fired timer
(`process_expired_timers`) by sending to the endpoint and `tasks.wake`, which enqueues the waiter
in its class. The S_EXT handler then returns to the interrupted task; the S_TIMER handler rotates
the running task only on its tick (10 ms, RFC-0098 C3); a syscall that wakes a peer (an IPC send,
a notify) returns to its caller. A woken driver runs when the running task blocks or when the
rotation reaches it. The bring-up passes were fast because nothing else ran; the desktop's are
slow because windowd, app-host and the apps do — and every one of them, drivers included, runs in
the `Normal` class (`QosClass` exists with strict order PerfBurst > Interactive > Normal > Idle,
consulted only at a switch point; nothing raises a service above Normal).

The stock system's 16x16 frame costs 0.64 ms: its driver's wake is a scheduler preemption. Ours
must be too, or the frame's p99 (≤ 16.7 ms, the UX goal) is bounded by the tick, not by the work.

## Decision

- **A wake preempts.** When a wake makes a task of a class strictly above the running task's
  runnable on this hart, the hart switches to it at the trap exit — in the S_EXT handler after the
  interrupt's delivery, in the S_TIMER handler after the timers' and deadlines' delivery
  (whatever the tick's rotation says), and in the syscall epilogue when the caller is still the
  current task. The scheduler answers the question (`Scheduler::ready_above_current`); the switch is
  the one the tick already makes (`preempt_current_user_task`; in the syscall path only the pick is
  staged, the caller's saved frame already holds its return). A wake of the running task's own
  class or below waits its turn as before.
- **The soft-realtime chain is Interactive.** init raises the services a frame or a key passes
  through — gpud, windowd, inputd, hidrawd, xhcid, touchd, imed: `service_topology::soft_realtime`,
  the one list ADR-0049's placement reads — to `Interactive` right before their wake
  (`task_qos_set_for`, the admin path), and again on a respawn. Everything else — apps, app-host,
  the background services — runs `Normal`; a service may lower itself, never raise.
- **Out of scope:** priority inheritance across IPC, per-task deadlines, a shorter tick, and the
  placement of the chain across harts (layer 7). An app is never Interactive by its own asking
  (ADR-0095 keeps an app's GPU context at normal priority for the same reason).

## Consequences

- **Positive**: a GPU interrupt, a key, a present or an ack reaches its driver within interrupt
  latency; a pass costs what the GPU takes; the frame's p99 is bounded by work. The cross-hart case
  already behaved this way (an acked resched IPI preempts, A4): the same-hart case now matches it.
- **Negative / accepted cost**: a `Normal` task can be preempted mid-slice by any wake of the
  chain — a CPU-bound app yields more often while the compositor is busy. That is the point. The
  scheduler's host tests do not run: `sched` is compiled only for the target (a pre-existing gap
  the two new tests join); the proof is the QEMU lanes' preemption-sensitive markers and the
  board's trace line (`late_us=`, the most a timer-ended slice woke after its deadline) beside the
  sweep's `q16_us`.
- **Follow-ups**: un-gate `sched` for the kernel's host tests; the board cycle that reads
  `late_us` ≈ 0 and `q16_us` within twice the bring-up pass; emit cost (~90 µs a draw on the CPU)
  as its own phase; HSR for opaque windows (pass type opaque) when the scene grows past 12
  windows.

## Alternatives considered

- **Interactive without preempt-on-wake**: the chain would be picked first at the next switch
  point, but the next switch point is still the tick — up to 10 ms late, one wake per pass stage.
- **A shorter tick** (1 ms): bounds the latency to the tick, burns CPU on every hart, and still
  pays up to a tick per wake. The latency is a property of the wake, not of the clock.
- **gpud spinning on the fragment word**: forbidden by the reactive rule (RFC-0033; no polling
  loops) and would only move the problem to windowd's wake.
- **The display and input chain on a hart of its own** (ADR-0049's placement with SMP): the right
  thing for layer 7, and it still needs preempt-on-wake on that hart — the apps that present are
  the ones competing with the compositor.
