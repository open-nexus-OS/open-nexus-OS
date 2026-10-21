<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-20 — memory and the app lifecycle: what dies is freed, what a launch costs (TASK-0333 6a, Block 4 layer 6)

## Question

What does the system give back when a task exits, and what does an app's launch cost by stage?
Layer 6 (`tasks/TASK-0333-…`) begins with the measurement every later step is judged against:
the kernel's memory record over a loop of exits (RFC-0100 §1: a VMO dies with its last
capability), the launch's stages on one clock, and the resident set per address space as the
mgs baseline left it (`../2026-10-10-mgs-baseline/README.md`, "Memory": ~163 MiB over 40 spaces,
gpud's contiguous block 63 % of it).

## Instrument

- **The VMO holder** (`userspace/apps/demo-exit0/build.rs`, execd image 6 `demo.vmo-holder`): a
  hand-assembled payload that prints `child: vmo holder start`, creates a 1 MiB VMO
  (`vmo_create`), maps it read/write (`vm_map`), stores one byte and exits 0 — holding the
  object. The selftest's exec phase spawns ten in a row (`vmo_holder_loop`, after the execd
  hardening rungs) and reads the kernel's record (`mm_stats`, syscall 61) before the loop and
  after every reap: `SELFTEST: mm loop (cycle= free= vmos= vmo_bytes=)` — the series — then
  `SELFTEST: rss loop ok (cycles=10 leaked=0)` when the record after the loop holds less than one
  holder more than before, else the FAIL form with the bytes and objects left behind.
- **The reap line**: `execd: reaped pid= code= (free= vmos= vmo_bytes=)` — the record right
  after execd reaps a child, on every lane, for apps and payloads alike (the app-level series on
  the board: the operator opens and closes apps, execd prints what came back each time).
- **The launch's stages** on the one monotonic clock (`t_us=`): `windowd: launch request app=`
  (the shell asked), `apphost: start` (the child runs), `WINDOWD: surface presented id= seq=1`
  (its first frame reached windowd). Differences read offline from the log.
- Both lanes: `just test-os` (headless; the visible lane adds the shell's launches) and the board
  (`scripts/board-test.sh --profile=board-visible`, the operator's open/close series).

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | An app's VMOs leak on exit (surface, heap): exit swaps the cap table, nothing destroys the object | `vmos` / `vmo_bytes` grow by a constant per cycle over ten cycles (one holder = 1 048 576 B) |
| H2 | The exec image and the page tables return at exit/reap | `free` moves by the VMO delta only; `pt_frames` flat across the loop |
| H3 | windowd's clone of an app's surface cap keeps the object alive after the kernel frees orphans (P1) | after P1, without windowd closing: `vmos` +1 per app close on the board/visible lane |
| H4 | gpud's heap grows per windowd restart | `heap_bytes=` across a windowd restart (one sample a boot until a restart rung exists) |
| H5 | The app-host child's 11.6 MiB = the image + heap/arena, and all of it returns | the `KINIT: mm space` line of a closed app vanishes; `free` recovers within a page table's worth |
| H6 | Launch time is the image copy, not the bundle read | `apphost: start t_us − windowd: launch request t_us` ≥ half of `surface presented t_us − launch request t_us` → 6c's number; else the DSL lane's |

## Results

### The stock tree (headless lane `headless--2026-10-19T17-37-40`; `headless-stock-loop-lines.txt`)

| cycle | free (frames) | vmos | vmo_bytes |
|---|---|---|---|
| 0 | 28 902 | 57 | 139 456 512 |
| 1 | 28 646 | 58 | 140 505 088 |
| 5 | 27 622 | 62 | 144 699 392 |
| 10 | 26 342 | 67 | 149 942 272 |

Every cycle leaves exactly one object of 1 048 576 B and 256 frames fewer — the holder's VMO,
nothing else (the image and the page tables returned: `free` moved by the VMO alone). The gate
red: `SELFTEST: rss loop FAIL (cycles=10 leaked=10485760 vmos=10)`. The reap line showed the
same for the earlier children: `execd: reaped pid=51 … vmos=40` (the greeter), then 57 objects
at the loop's start — the exec phase's own children (exit0, fault, minidump) had left theirs too.
**H1 confirmed, H2 confirmed** (`free` delta = the VMO delta).

### The reclaim (6a P1; headless lane `headless-reclaim-loop-lines.txt`)

A VMO dies with its last capability (`mm/reclaim.rs`, `mm/vmo_lifetime.rs`): at a reap the
objects the zombie named at its exit and the ones its space mapped are freed when nobody else
names or maps them; the last `cap_close` of a VMO capability does the same.

| cycle | free | vmos | vmo_bytes |
|---|---|---|---|
| 0 | 28 908 | 52 | 139 427 840 |
| 1 … 10 | 28 908 | 52 | 139 427 840 |

`SELFTEST: rss loop ok (cycles=10 leaked=0)`; `KSELFTEST: vm reclaim (vmos=18 bytes=11010048)`
at the boot-end fence — the ten holders (10 485 760 B) and eight objects of the earlier children
(524 288 B) that leaked before; the fence's object count 52 against the stock tree's 67. Nothing
else moved: `free` at the fence 28 908 against 26 342, `allocs`/`frees` 3477/285.

### The launch's stages (QEMU visible lane `visible--2026-10-19T18-00-18`; `visible-reclaim-stage-lines.txt`)

The greeter, the boot's first app, under TCG on one hart:

| stage | t_us | from the request |
|---|---|---|
| `windowd: launch request app=greeter` | 3 977 721 | 0 |
| `apphost: start` | 4 005 500 | +27.8 ms |
| `WINDOWD: surface presented id=1 seq=1` | 5 232 513 | +1 254.8 ms |

The spawn (execd's exec, the image copy, the child's first instruction) is 28 ms; the 1.2 s
between the child's start and its first frame is app-host's own path (the atlas, the payload,
the DSL's first layout, the surface) — **H6 refuted for QEMU**: the image copy is not where a
launch's time goes; the board's numbers (cycle 44) decide for the hardware. The lane's loop:
`SELFTEST: rss loop ok (cycles=10 leaked=0)`, `KSELFTEST: vm reclaim (vmos=19 bytes=11038720)`.

### The board (cycle 44, `board--2026-10-19T18-09-58`)

- **The loop broke at its third holder**: `execd: exit pid=70 reason=error code=5` — the payload's
  "a syscall answered an error" exit — then `SELFTEST: rss loop FAIL (holder did not exit 0)`.
  The first two cycles were flat (`free=977397 vmos=56` through cycles 0–2). The one-hart lanes
  never see it; four harts do. The suspect: a `vmo_create` runs its phase B (1 MiB zeroed with
  the kernel lock dropped) with an object no capability names and no space maps yet — the shape
  the lifetime rule frees if a concurrent reap or close lists its (reused) id. Built after this
  cycle: the payload's exit names the syscall (5 create, 6 map), an object between a create's
  phases is marked and left alone by the rule, a vanished object is named
  (`VMO-CREATE lost`, `VM-MAP-FAIL reason=no-object`), the first 24 reclaims print their id and
  the record (`KINIT: vm reclaim (id= bytes= by= free= vmos=)`) — the kernel's lines are not
  folded in the interactive session, execd's are (`NEXUS_LOG_EXPAND=execd`), which is why the reap
  line is absent from the operator's part of this log.
- **The launch's stages on the board**: the greeter 18.3 ms to `apphost: start`, 654 ms to its
  first frame; the shell 43.6 ms and 589 ms; the operator's apps (the four desktop icons, eight
  launches) 240–396 ms from `apphost: start` to the first present, two outliers at 2.6 s and
  3.8 s. The spawn is small on the board too (H6 refuted here as on QEMU); the first frame is
  app-host's path. The operator's launches go through the shell's own `ability.launch`
  (abilitymgr → execd), not windowd's `launch request`, so that stage names only the greeter
  and the shell.
- **The operator closed every app with its close button** (`windowd: press app0 zone=close`,
  `WINDOWD: app window closed slot=0`, eight times). Whether the app-host behind it exits is
  not in this log (execd's lines fold; the memory fence lies before the session) — cycle 45
  reads it from the kernel's reclaim lines.
- Cycle 44's ladder was red on the loop rung; the next cycle judges the fix.

### The board (cycle 45, `board--2026-10-19T21-44-15`; `cycle45-lines.txt`)

- **The loop green on four harts**: ten holders, each reclaimed `by=reap` (`id=57`, 1 048 576 B),
  the record flat through cycle 0…10 (`free=977397 vmos=56 vmo_bytes=187318272`), no
  `VMO-CREATE lost`, no `VM-MAP-FAIL`, no exit 5/6: the guard between a create's phases
  closed the race. Fence: `KSELFTEST: vm reclaim (vmos=19 bytes=11022336)`. Ladder: 71 rungs
  present; the operator did not tile, so the `tile` marker was not acked and the lane ended
  there — honest, not a build fault.
- **A window closed with its button never ends its process.** Nine launches (calculator, chat,
  settings, stash, chat ×4, …); after every `WINDOWD: app window closed` not one reap, not
  one `vm reclaim … by=reap`; abilitymgr counted `inst=4 … inst=9` and removed nothing. The
  marker app-host prints on its way out (`APPHOST: window closed - exiting`) appears in no log
  ever recorded. Read in the code: windowd closes its SEND capability on the app's event
  channel (`close_app_window`), execd and app-host close theirs, so no foreign sender remains —
  but app-host receives NON-BLOCKING after its waitset wake (TASK-0324 P7-d), and the kernel
  decided EOF only in `recv`'s blocking branch: `WouldBlock`, the latch consumed, the next wait
  parked forever (`ipc_msg.rs`). **H3 answered: not windowd's clone — the kernel's verdict.**
- **What the dead app held, by the kernel's tables** (counted for 6a's answer): its address
  space and VMOs (11 MiB, never reaped), two endpoints per launch owned by execd and closed by
  nobody (`MAX_ENDPOINTS` 384, 96 per owner, ids never reused — a launch counter), its waitset
  and timer (the exit swapped the capability table without freeing them; `MAX_WAITSETS` 64),
  and an instance slot in abilitymgr (`MAX_INSTANCES` 32).
- **Launch stages (warm)**: spawn 16–50 ms, first frame 240–390 ms after `apphost: start` —
  cycle 44 confirmed.
- **Built after this cycle (ADR-0073):** every kernel object dies with its last capability, the
  exit funnel releases like a `cap_close` of each, the EOF verdict independent of the wait
  mode, `PeerClosed` for a sender without a receiver, the death as an event
  (`child_exit_bind`), execd's reactive reap and report, abilitymgr's instance removal,
  windowd's dead-client close. Cycle 46 judges: `APPHOST: window closed - exiting`,
  `KINIT: vm reclaim (… by=reap)` and `abilitymgr: stop (inst= … live=)` after every close;
  `SELFTEST: rss loop ok (… kobjects=0)`; the four kernel selftests on four harts.

### The board (cycle 46, `board--2026-10-20T11-21-25`; `cycle46-lines.txt`)

- **The chain holds at every close** (six closes: calculator, chat, settings, stash, chat,
  chat): `APPHOST: window closed - exiting` → `KINIT: vm reclaim (… by=reap)` →
  `execd: exit notified (pid= code=0 reason=clean to=abilitymgr)` →
  `abilitymgr: stop (inst= app= pid= code=0 live=2)`. Kernel objects flat (`kobjects=266` at
  every reap), the endpoint ids 233/234/235 reused in a cycle (`KINIT: ipc reclaim`), the
  four kernel selftests on four harts, `rss loop ok (cycles=10 leaked=0 kobjects=0)`.
- **One leak named by the reap lines**: per open/close `vmos` +1 and `vmo_bytes` +528 384
  (59 → 64; `free` −129 frames each) — execd's payload VMO (`PAYLOAD_VMO_BYTES` = 16 + 512 KiB
  = 129 pages): execd created it, copied the capability into the child and never closed its
  own. **H5 answered: everything of the child returns except what execd still named.**
- **Launch stages (warm)**: calculator 535 ms, chat 834–1076 ms, settings 6.7 s, stash 4.7 s
  from `apphost: start` to the first present — the two large-surface apps are the outliers.

### The board (cycle 47, `board--2026-10-20T11-33-22`; `cycle47-lines.txt`)

- **The payload capability closed**: `free`/`vmos` flat across four closes
  (`free=973380 vmos=58 kobjects=266` at every reap).
- **The operator's experience: slow, "crashed twice".** Both boots end the same way
  (`trace-all.log` holds both): `dbg: hidrawd inputd send fail backpressure`,
  `hidrawd: chain I2 wire send FAIL (inputd route)`, then `tx hz=0` — the input chain parked.
  windowd parks on an app's full event queue (a tap is `Delivery::Critical`, blocking) while
  the app parks on windowd's full request queue (its present): mutual wait. No panic, no
  fault beyond init's fault-probe fixture and the selftest's `demo.fault`. This is ADR-0075 /
  6d ("windowd never parks on an app"), measured here as the operator's main pain.
- **The slowness is the GPU and the first frame, not the IPC**:
  `gpu frame us (… avg=69677 … wait 66394)` at `glass=4.3` layers (Control Center plus
  windows; cycle 45 ran 28 ms at 1.6) — a slideshow during open/close animations; the IPC call
  on the headless lane went 43–44 → 47 µs with three table scans per endpoint capability
  close, folded into one census after this cycle.

### The board (cycle 48, `board--2026-10-20T11-50-56`; `cycle48-lines.txt`) — 6a's gate

- The census fold and the payload capability close on the board: the chain at every close
  (calculator, chat, settings, stash, chat), the record flat (`free=973380 vmos=58
  kobjects=266` at every reap), the four kernel selftests, `rss loop ok (cycles=10 leaked=0
  kobjects=0)`, `vm reclaim (vmos=19 bytes=11022336 endpoints=3)`; no input stall this cycle
  (the mutual-wait hazard of cycle 47 stays 6d's, next). The operator's full series
  (typing, pointer, modal, tiling, clipboard, screenshot) acked; `[PASS] board-visible: 73
  rungs, 8 operator markers`. The headless lane's IPC call is back at 43 µs.
- Launch stages (warm): calculator 434 ms, chat 459–912 ms, settings 7.3 s, stash 4.6 s —
  the large-surface apps' first frame stays the outlier (6c / the DSL lane).
- GPU frame time 36–69 ms at several glass layers (the next package after the non-parking
  compositor).

### The board (cycle 49, `board--2026-10-20T12-32-45`; `cycle49-lines.txt`) — the UX contract's first reading (RFC-0108)

| Line | Windows | Reading |
|---|---|---|
| `gpud: ux frame` | 10, all `FAIL` | 14–32 of 32 frames past 16.7 ms; `max_us` 92–315 ms; `gpu frame us` avg 44–84 ms with 95 % in `wait` (the GPU) at 2.4 Mpx of layer pixels per frame — whether 1.6 or 6.1 glass layers |
| `windowd: ux input` | 12, all `FAIL` | the first staged input waits 0.6–5.0 s for the frame that carries it (the submit loop throttled by frames in flight of 50–300 ms each) |
| `apphost: ux launch` | 8 | calculator/chat/greeter/shell `warn` 456–881 ms; settings `FAIL` 8.1 s, stash 3.7 s |

The chain of 6a holds beside it (five closes, `free=973364 vmos=58 kobjects=266` at every
reap); one transient `hidrawd inputd send fail backpressure` recovered this cycle. The lesson for
the order: the frame cost (the whole screen redrawn every frame, on the GPU) drives both the
slideshow and the input's wait; the non-parking compositor prevents the freeze.

**The cost model (read from the same windows, TASK-0335 P0):**

| window | avg | passes | area_px | GPU wait | wait per pass |
|---|---|---|---|---|---|
| 1 | 30 ms | 4.1 | 1.23 M | 28 ms | 6.9 ms |
| 5 | 44 ms | 6.5 | 1.42 M | 42 ms | 6.4 ms |
| 7 | 84 ms | 13.0 | 1.23 M | 79 ms | 6.1 ms |
| 10 | 48 ms | 4.3 | 1.60 M | 46 ms | 10.7 ms |

The GPU's wait is ~6.5 ms per pass, nearly flat across areas and draws (two windows with the
largest glass scenes 18 ms a pass); the boot trace puts a full-screen pass at 12 ms
(`gpu pass trace … 12300:2:3`), the raw fill at 1 ns a pixel (`gpu fill ok (… ns_per_px=1.0`).
So the GPU is memory-bound at a few nanoseconds per **textured** pixel: a frame costs the pixels
its passes draw — the slot's area × its layers, each re-blurred glass's scene below it over its
reach, the blurs — not its draws or its passes. 5–10 Mpx a frame today; 16.7 ms buys ~3. The
16 KiB system-level cache every binding bypasses is no lever at these sizes. `gpud: gpu work`
(cycle 50) counts the pixels by pass kind to confirm the model and name the dominant kind.

## Verdict (6a)

What dies is freed: a VMO, an endpoint, a timer, a waitset, a fence die with their last
capability; a task's exit releases like a `cap_close` of each; a death is an event the reaper
wakes on and the lifecycle authority hears; a window closed ends its process and leaves the
kernel's record where it was. The launch is not the image copy (spawn ≤ 50 ms); the first
frame of a DSL app is (0.4–7 s). The allocator's freeing by default is judged in 6b, where
static heaps become visible RSS.
