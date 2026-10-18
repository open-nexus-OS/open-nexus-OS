---
title: TASK-0330 SMP v3: n harts from the FDT (boot-sized per-hart state, 64-bit CpuSet ABI, MAX_CPUS deleted) + cluster topology, per-hart PLIC/IRQ affinity, the IPC router and the address spaces out of the BKL, per-ASID shootdown
status: Draft (recut 2026-10-06 to the end state — n harts from the FDT, not `MAX_CPUS` 4 → 8; no distributed kernel; seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; starts after Block 3)
owner: @kernel-team
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Lock classes and budgets: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md, source/kernel/neuron/src/core/trap/budgets.rs
  - Per-hart EDT: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md
  - Scheduler half: tasks/TASK-0306-ui-responsiveness-input-integrity-smp-rendezvous.md (Phase 3)
  - IPC fastpath numbers to hold: docs/rfcs/RFC-0096-ipc-performance-contract-v2-call-reply-recv-fastpath.md
  - Gate consumer: tasks/TRACK-TIME-AS-RESOURCE.md
  - RFC/ADR seeds (at P0): RFC-0102 SMP lock decomposition, ADR-0072 cluster-aware spreading, ADR-0073 BKL split order (from R9)
---

## Origin

No ledger carried the remaining SMP work: 0012/0012B/0042/0247/0277/0283/0288 are Done, `TASK-0306` Phase 3 (BKL hold) is the scheduler half and stays its own ledger (S2). Minted for target picture S; B/C parts seeded at P0.

## Context

`MAX_CPUS = 4`, per-hart runqueues live inside ONE big kernel lock (`core/trap/runtime.rs`) covering scheduler, tasks, IPC router, address spaces, timers, waitsets and fences; TLB shootdown is a full flush on every hart; PLIC delivery is boot-hart only; the display chain is pinned to cpu0. The board has 8 harts in two clusters.

## Goal

**End state, not a bigger constant** (operator decision 2026-10-06): the hart count comes from
the FDT at boot; every per-hart structure is allocated from a boot slab sized by it; the only
compile-time ceiling is the CPU-set width (`CpuSet` = 64 bits, an ABI with its own RFC, the next free number — replacing the
`u8` affinity mask, the real limit of 8 today); `MAX_CPUS` is deleted with a retired-name gate
(`CPU_MASK_BITS` stays). The cluster topology (`cpu-map`: the board's 2 × 4 harts) is a kernel
model exported to init/execd — no literal masks anywhere in userspace. S1 (this ledger): the
above + PLIC contexts and IRQ affinity on every hart (`KSELFTEST: smp online ok harts=8
clusters=2`, IRQs served off hart 0) + the HSM retry bug fixed (`retry_missing_harts` passes a
virtual start address, `bringup.rs:273`, while the first start uses `virt_to_phys` — the likely
MTTCG "lost hart" flake behind `[profile.smp] SMP="2"`). S3 (`0330B`): IPC router + waitsets out
of the BKL. S4 (`0330C`): address spaces out of the BKL, per-ASID targeted shootdown. S6 (new,
small): the compute-distribution seam — ADR-0074 "jobs travel, capabilities never; one kernel per
machine; brokers compose", pinched `Backend::{Local, Remote}` with `Remote` refused by name, the
job's determinism contract host-tested. **Not a multikernel**: replicating kernel state per core
buys nothing on a coherent 8-hart SoC and costs every cap/task/VMO twice; the scaling the
multikernel principle promises comes from the lock decomposition (S2–S4), and distribution
across machines is a userspace concern (RFC-0028: no remote execution, no capability transfer).

### Measured inventory (S0, 2026-10-06 — what `MAX_CPUS` sizes today; `source/kernel/neuron/src/`)

| Static | Where | Boot-sized in S1 |
|---|---|---|
| `HART_LOCALS` (a `TrapFrame` each, fixed address for `trap.S`, `tp` range check) | `core/smp/mod.rs:95` | stays static at `CPU_MASK_BITS` (≈32 KiB) |
| secondary stacks 3 × 64 KiB in .bss | `core/smp/bringup.rs:42` | per started hart from the frame pool |
| resched/IPI counters, wake hints, timer ticks, steal gate | `core/smp/mod.rs:104-123`, `runtime.rs:30-85` | slab |
| IRQ stash `[[AtomicU64; 16]; N]` | `core/irq.rs:99` | slab (the boot hart's entry before the first trap) |
| PLIC S-contexts | `hal/platform.rs:50` | slab |
| deadline shadow (ADR-0052) | `core/trap/runtime.rs:357` | slab |
| console line buffers | `hal/console_line.rs:52` | slab (boot hart's entry early) |
| TLB mailboxes + activity | `core/smp/tlb.rs:46,70` | slab |
| `Scheduler.cpus`, `TaskTable.current` | `sched/mod.rs:145`, `task/mod.rs:504` | `Vec` from `hart_count()` |
| `affinity_mask: u8`, `ALL_CPUS_MASK` | `task/mod.rs:32,288`, `task/affinity.rs` | `CpuSet(u64)` |
| literal masks `0b0001/0b1111/0b1110`, `0xF`, `MAX_WORKERS = 4`, `1 << (idx & 7)`, `PINCHED_WORKERS = 2` | `init/affinity.rs`, `bootstrap/resume.rs:32`, `execd/sched_recipe.rs:18`, selftest `soaks.rs:21`, `nexus-workpool/pool.rs:29,182`, pinched | from the exported topology; never a worker on the soft-RT hart |

Bring-up today: `start_secondary_harts` loops `1..MAX_CPUS`; the board's harts 4–7 (the whole
second cluster) stay in HSM STOPPED unnamed. Boot hart ≠ 0 is a silent assumption — named as a
FAIL in S1.

## S0 measured (2026-10-10, `docs/board/measurements/2026-10-10-mgs-baseline/`)

- The lock accounting had two faults: tick bounds at the emulator's timebase (the board's
  ">10 ms" bucket counted from 4.2 ms) and a window that counted the call that reset it. Fixed
  with the instrument (`core/trap/lock_stats.rs`, host-tested); the FAIL gate of both lanes
  now reads the kernel's lines.
- Steady state on the board, four harts: `bkl budget ok (max_wait=10615us max_hold=10ms
  nr=16)`; holds above 10 ms: 1; top holders are the two console writes (10.5 ms, 5.0 ms) and
  the IPC call (2.3 ms). Bring-up: 22 ms hold, 48 ms wait, seven console writes of 10–19 ms.
- IPC sends by hart: 80 / 1 / 1969 / 2761.
- Consequence — **S2a, new and first in S2**: console writes leave the kernel lock, the board
  gets its own budget (the constants are the emulator's; the tracked line in
  `config/fail-marker-allow-board.txt` goes with it).
- Still S0's: the warm/cold series for the start hang, before S1 touches the secondary-hart
  start. `-smp 8` ×10 moves to S1's gate (four harts run until `MAX_CPUS` is gone).

## S2a — the console off the kernel lock (Block 4 layer 2, 2026-10-12 → ✅ board 2026-10-13)

- **Three lenses.** Scope: `source/kernel/neuron/src/{console_tx.rs, hal/console_queue.rs,
  hal/platform.rs, core/irq.rs, core/kmain.rs, core/panic.rs, core/trap/handler.rs,
  core/trap/lock_stats.rs, syscall/api/{sched_telemetry,sync_objects}.rs}`, the marker
  `KSELFTEST: console off lock ok (` in `end.toml`, `qemu-test.sh` and `board-test.sh`; out: the
  scheduler lock (S2), the IPC router (S3), the budgets' constants (they stay the emulator's
  until S2 measures the board), any change to what is logged. Invariant: a console byte enters
  the queue only after RFC-0107's ring recorded it and leaves in the ring's order (the trace
  contract stays byte-equal); the queue is bounded and a full queue sends its oldest bytes polled
  before it takes a new one — never a drop, never a reorder; the console's interrupt line is the
  kernel's (`irq_bind` refuses it). Contract: RFC-0107 (the ring), RFC-0098 C3 (the console from
  the tree — its `interrupts` property is what the queue drains on), ADR-0046 (the lock budgets).
- **Measured cause** (S0, `docs/board/measurements/2026-10-10-mgs-baseline/`): the steady state's
  longest lock holders were syscall 16 (10.5 ms) and 44 (5.0 ms) — the console writes — and the
  bring-up's 16–19 ms holds were `nr=44`: a line costs its length in serial time at 115 200 baud,
  under the lock.
- **Built**: `console_tx::TxRing` (8 KiB FIFO, host-tested), `hal::console_queue`: a byte goes
  to the ring, then the queue; the UART's transmitter-empty interrupt (the console node's
  `interrupts`, enabled at the PLIC for the boot hart once the runtime runs) drains a FIFO
  burst per interrupt and turns itself off with the last byte; a full queue sends a burst
  polled first; a panic turns the queue off and drains it before its words; a tree without a
  line drains at the timer tick. `irq_bind` refuses the console's line.
- **Exposed and fixed on the way** (`docs/board/measurements/2026-10-12-console-off-lock/`):
  windowd dropped frames of user intent (text commits, keys, acks, the screenshot's shutter) on a
  full client queue — `WINDOWD: FAIL app event send` — which the console's stall after every
  line had throttled out of sight; `send_app_frame` and `send_desktop_frame` now take the parking
  delivery (`Delivery::Critical`), never dropping. The first cut's byte-wise polled fallback with
  the FIFO on cost 9.6 ms a console syscall on the board (the gate red on a real fault); the
  fallback waits once and writes a burst.
- **Gate**: `KSELFTEST: console off lock ok (max_us= budget_us=1000)` — the longest hold of
  either console syscall in the steady window, REQUIRED on every QEMU lane and in the board's
  headless ladder; its FAIL form is caught by the FAIL gate. Shown red by the measured numbers
  (`lock_stats::tests::test_reject_the_boards_console_holds_against_the_budget`: 10 540 µs
  against 1 000); green on QEMU and the board with the queue.
- **Board** (`docs/board/measurements/2026-10-12-console-off-lock/`): the second cut's interrupt
  never fired on the board (`irq_bursts=0`, gate red at 16 444 µs) — the console UART is a
  PXA-style 16550 whose IER carries a DMA-requests bit (the vendor console transmits by DMA; PLIC
  42 counts zero on the stock system) and whose MCR OUT2 gates the interrupt output. Third cut:
  the DMA bit cleared, OUT2 set, both read back in the boot line; a pusher fills an idle
  transmitter itself and the drain waits for the queue's lock (the queue no longer depends on
  the line's trigger mode). `board--2026-10-13T08-28-06`: `console off lock ok (max_us=273)`,
  `console tx (irq_bursts=2348 kicks=962 full=1059 lock_fallback=0)`, `bkl budget ok
  (max_wait=3050us max_hold=0ms)`, hold histogram `le10ms=0 gt10ms=0` — the first board boot
  with the kernel-lock budget green; its allow-list line is removed. `[PASS] board-visible`
  50 rungs, 7 acks.
- **Handed on**: `KSELFTEST: ipc call budget FAIL (rt=97us budget=64)` on that boot (56/60
  before, QEMU 43 unchanged): the mean of 64 round trips to samgrd under a boot that no longer
  waits for the console, with a shifted hart placement — **S3**'s number (placement, IPI cost),
  tolerated on the board with that reference. The operator's "typing after maximizing is barely
  possible": app-host's frame arena spills on the maximized layout (input path healthy) — the
  memory lane's (TASK-0290 "Input from Block 1").

## S2b — the serial line reads the ring (Block 4 layer 2, 2026-10-18 → ✅ board 2026-10-18)

- **Three lenses.** Scope: `source/kernel/neuron/src/{console_tx.rs, hal/console_queue.rs,
  hal/console_ring.rs, hal/platform.rs, hal/console_line.rs, core/panic.rs,
  core/trap/{fault,handler}.rs, syscall/api/sched_telemetry.rs, lib.rs}`; RFC-0107 (a note: the
  serial line is the ring's reader). Out: the console syscalls' lock class (S2), the line
  discipline, the folding of crash reports (RFC-0068), the console's rate, the interrupt
  routing and the scheduler's stealing (S1–S3), the ring's layout. Invariant: no console write
  waits for the transmitter; the serial line sends the ring's bytes in the ring's order; a line
  the writers lap skips to the oldest byte kept and says so in one line, counted; every QEMU
  lane's trace contract stays byte-equal. Contract: RFC-0107 (the ring), S2a (the interrupt,
  the board's UART registers), ADR-0046 (the lock budgets).
- **Measured cause** (the GPU lane's board cycles 25–26; measurement README "S2b"): every
  bring-up filled S2a's 8 KiB queue (`full=1139`, `max_depth=8192`), and a write into a full
  queue waited for the line with its hart's interrupts off — `debug_write` under the kernel lock
  for its line's serial time (`bkl bring-up burst max_hold=20ms nr=44 gt10ms=41`), a crash
  report of the supervision probe 134 ms on its hart, which held the GPU containment rung's
  teardown for 128 ms (`f_teardown_us=129833` against 1450). The board's serial line has no
  reader: the proof is the ring on the boot disk.
- **Built**: `console_tx::Cursor` (host-tested: the ring's order a burst at a time, nothing past
  the head, the lap skipped to the oldest byte kept, a burst the writers reached while it was
  read not sent, the note's text) replaces the 8 KiB `TxRing`; `hal::console_queue` is the
  line: a write records its byte under the ring's lock and kicks the line after it — an idle
  transmitter filled at once, the transmitter-empty interrupt live while the line owes bytes;
  the switch to reading the ring happens between two bytes under the ring's lock; a panic sends
  what the line owes, polled, then its words; the tick backs up a kick that found the line's
  lock held. Lines `KINIT: console tx (irq_bursts= kicks= behind_max= lapped= lock_fallback=)`,
  `KINIT: user fault reports (n= max_us= total_us=)`.
- **QEMU** (2026-10-18): `headless`, `visible` green, the trace contract byte-equal,
  `behind_max=1 lapped=0`, `user fault reports (n=6 max_us=980)`; the two-hart `smp` lane red
  for its own reasons as before (no new FAIL), its trace contract byte-equal.
- **Gate** (board, H1–H5 of the measurement README): the bring-up's holds no longer the
  console's, `lapped=0` with `behind_max` above the old queue, a crash report below 10 ms, the
  steady console hold as before, the GPU rung's teardown near 1.5 ms.
- **Board** (cycle 27, two boots of one image, `s2b-board-lines.txt`; `[PASS] board-visible`,
  66 rungs, 7 acks on the second boot): `bkl bring-up burst max_wait=1499us max_hold=0ms nr=6
  gt10ms=0` and `1475us … gt10ms=0` (cycle 26: `28868us max_hold=20ms nr=44 gt10ms=41`) — H1;
  `console tx (… behind_max=22491 lapped=0 lock_fallback=0)` and `22249 … lapped=0` — H2; `user
  fault reports (n=6 max_us=3030)` and `3126` against the report's 133.7 ms of serial time — H3;
  `console off lock ok (max_us=249)` and `195` — H4; H5 not decided by an overlap (no report fell
  into the GPU rung; its teardown 1504 and 1501 µs). The two boots agree within a few percent.
  On the way: windowd's GPU context opens in 6.1 ms, not 10 (TASK-0215 P3's H65 corrected: the
  3.4 ms were the bring-up's lock waits); `ipc call budget ok (rt=44us)` in both boots (one
  cycle, not a verdict on its flake). The first boot's screenshot hung after a dragged selection
  — the same signature as 2026-10-14, before S2b; handed to TASK-0215 P4.

## Non-Goals

The scheduler lock itself (`TASK-0306` Phase 3 = S2), the placement policy (`TASK-0042B` = S5), heterogeneous scheduling beyond cluster awareness.

## Packages (from the order file; the P0 rewrite fixes them)

- **S0** — measure: the inventory above; `-smp 8` MTTCG ×10 boots against the retry bug (H1); R9 BKL hold histogram + IPC rate per hart on the board at 4 harts (the baseline the split order follows); the `cpu-map`.
- **S1** (0330) — n harts from the FDT (boot slab, `CpuSet` ABI, `MAX_CPUS` deleted), cluster topology exported, per-hart PLIC contexts, IRQ affinity, retry fix. Gates: host tests (topology vs `bpi-f3.dtb`/`virt.dtb` goldens, `CpuSet` rejects, pin rejects), `ci-os-smp` (2 harts) green, `smp8` best-effort until H1 holds ×10, board `KSELFTEST: smp online ok harts=8 clusters=2` + IRQ-off-boot-hart marker, `board-headless`/`-visible` green, retired-name gate.
- **S3** (0330B, at P0) — IPC router + waitsets off the BKL. Gate: RFC-0096 numbers per hart; time-as-resource gate green.
- **S4** (0330C, at P0) — address spaces off the BKL + per-ASID shootdown. Gate: shootdown-count gate (targeted, not full).
- **S6** (small) — the compute-distribution seam: ADR-0074, pinched `Backend::Remote` declared and refused by name (`test_reject_remote_backend_until_built`), the job's determinism contract over both backends host-tested. No network code; dsoftbus discovery (TASK-0331) is a later dependency.
- **Not in this lane** (operator 2026-10-06: the lane must not grow): the first parallel consumer — the band-parallel CPU present (gpud's executor on the workpool; 30 ms full frame at 1080p, the typing flicker, xhcid starved meanwhile) — rides on S5 under G3/G4 / TASK-0251 finding 6.

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: S0's measurements (R9, the `-smp 8` boots, the inventory) are done BEFORE any kernel edit; the split order (ADR-0073) is written from the histogram, not from intuition.
- **RED**: `HART_LOCALS` at the mask width changes `trap.S` layout constants — released only with the `cpu_current_id` fallback counter at 0 on QEMU and the board.
- **YELLOW**: the boot slab must exist before the boot hart's first trap uses the IRQ stash / console line (the boot hart's entry stays static or the slab precedes the first trap).
- **YELLOW**: the board hung twice at the start of SMP scheduling after warm resets (2026-10-09,
  image `dev-e5b3a848`, kernel unchanged since a good boot that day): the rescued console ring
  ends mid-line after `KINIT: cpu0 sched loop` with cpu2's line missing, before init's first
  line; a cold power cycle booted. Evidence and reading:
  `docs/board/measurements/2026-10-09-smp-start-hang/`. S0 measures it (warm vs cold, ×N boots)
  before S1 touches the secondary-hart start.
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
