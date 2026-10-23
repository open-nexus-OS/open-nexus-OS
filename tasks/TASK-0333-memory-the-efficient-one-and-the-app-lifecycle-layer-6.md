---
title: TASK-0333 Memory, the efficient one + the app lifecycle — Block 4 layer 6 (RFC-0100): what dies is freed, unified memory with the GPU committed on touch, shared code, freeze/kill like a phone, restore, pressure, compression before swap
status: In Progress (6a ✅ 2026-10-20, 6b ✅ 2026-10-23 — 6b-1, 6b-2a, 6b-2b, 6b-3 (board cycle 67); 6c in progress 2026-10-23; planned 2026-10-19 with the operator)
owner: @kernel-team @runtime @ui @gpu
created: 2026-10-19
depends-on: [TASK-0286 (M1: frames from the FDT, page-backed VMOs), TASK-0215 (layer 5: the desktop on the GPU)]
follow-up-tasks: [layer 7 SMP (TASK-0330 S1–S3), layer 8 closing (S4–S6, G5, the GPU items layer 5 carried)]
links:
  - Contract: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md (seeded here; filled in 6b)
  - Order: tasks/IMPLEMENTATION-ORDER.md ("Block 4 in layers", layer 6 — recut 2026-10-19: memory before SMP)
  - Absorbs / retitles: tasks/TASK-0286B (M2 → "M2 + the split", seeded at 6b's P0), tasks/TASK-0286C (M3, at 6c's P0), tasks/TASK-0287 (M4), tasks/TASK-0290 (M5; its "Input from Block 1" moves to 6b), tasks/TASK-0287B / 0287C (M6/M7, at 6f's P0), tasks/TASK-0228 (superseded by memd), tasks/TASK-0234 + 0235 (lifecycle, into 6d), tasks/TASK-0236 + 0237 (only the wake-lock / background-declaration half, into 6d), tasks/TASK-0269B (the launch budget, into 6a/6e), tasks/TASK-0080D R3 (`@persist` on suspend, into 6e)
  - Measured today: docs/board/measurements/2026-10-10-mgs-baseline/README.md ("Memory"); the layer's own folder docs/board/measurements/2026-10-20-memory-lifecycle-baseline/ (6a P0)
  - Method: .claude/skills/driver-bringup/SKILL.md (measure → hypotheses with a decision line → gate → code → gate)
  - Prior art read: HarmonyOS (freeze + compressed memory + prewarmed spawn), iOS (early freeze, Jetsam bands, snapshot + state restoration), Fuchsia (capability-started components, suspend as a token, buffered channels), macOS (automatic termination + resume)
---

## Context

Layer 5 left the desktop on the GPU. The operator pulls the memory layer before SMP and widens it
(2026-10-19): a RAM management like a phone's — background apps frozen, killed by priority under
pressure, silently terminated when unused and restored so nobody notices — **dynamic in what the
machine has** (the FDT names the banks; no fixed number anywhere), **highly efficient**, and
**unified with the GPU** as on a modern desktop SoC: one pool, GPU buffers are ordinary pages
charged to their client, growing and shrinking on demand. Today little is needed; when the browser
arrives everything must stay fast.

**Measured first (2026-10-19, the tree and the mgs baseline):** the board has 4 GiB, the whole
system is resident in ~163 MiB (gpud 103.9 MiB = the contiguous layout block, 63 % of the whole
resident set — *corrected 2026-10-22: the 63 % is the block's share of all memory; how many atlas
rows are ever touched was never counted, 6b-2's P0 counts them*; an app-host child 11.6 MiB each;
windowd 10.6). M1 is done (buddy frames,
`vmo_destroy` returns frames). Nothing else exists: **a dead task's VMOs are never freed** (exit
swaps the cap table, `task/mod.rs:958`; `task_image.rs:62-87` returns the exec image only), every
OS service runs on a bump heap that never frees (`nexus-service-entry/src/os/global_alloc.rs`; the
free list is opt-in in vfsd/app-host/screencapd), gpud on that bump without the free lists
(*corrected 2026-10-22: not a heap of its own — the service entry's 384 KiB default*), no demand
paging or CoW (`fault.rs:372` kills), `exec_copy.rs` copies the 7 MB app-host ELF per process, no
pressure signal, no limits. Lifecycle: `AbilityState` (six states) exists and nothing sends
`OP_TRANSITION`; instances are never removed (`MAX_INSTANCES=32`); the kernel cannot freeze or kill
another process (`TaskState::Suspended` is pre-resume only); windowd parks without a deadline on a
full app queue (`client_surface.rs:37-73`, `app_window.rs:695-894`) — a frozen app would hang it;
no death detection for app clients; `@persist` (NXPS v1, `userspace/dsl/runtime/src/persist.rs`)
is not wired; the manifest has no background-task declaration. Launch time is unmeasured
(TASK-0080, TASK-0269B parked). Endpoints ARE buffered (depth 1..256 × 8 KiB): the pattern that
keeps a server off a frozen client exists; the rule does not.

## Three lenses (architecture-review, before code)

- **Scope (layer 6).** Kernel: VMO lifetime on exit, commit-on-touch + CoW on the user fault, the
  file-backed kind and the page cache, process control of a child (suspend/resume/kill, a
  `killable` hint), watermarks from the FDT and a pressure notification, the compressor pool and
  swap (last). Services: the OS allocator frees by default; gpud and windowd split the layout
  block and charge GPU memory to the client; windowd never blocks on an app; abilitymgr drives
  transitions; execd freezes/kills; `memd` is born (policy); app-host wires `@persist` and the
  pressure event. **Out:** SMP (layer 7: no lock-path change here), window UI in windowd, a
  general-purpose allocator port, policy in the kernel, any interim bump of heap/arena sizes,
  fake OOM on the proof ladder, G5's zero-copy import (layer 8, the consumer of unified memory).
- **Invariant.** A VMO is freed only when no capability and no mapping names it
  (`test_reject_reclaim_shared_vmo`, `test_reject_reclaim_mapped_vmo`); the allocator never
  hands a block out twice (`test_reject_double_free`); a GPU-bound page is never purged; no send
  from windowd to an app ever parks (`test_reject_park_on_app`); the kill order never names a
  foreground or visible app (`test_reject_kill_visible`); identity = `sender_service_id`.
- **Contract.** RFC-0100 (the memory object model v2: kinds, commit, CoW, reserve, accounting,
  pressure levels, purgeable/seals, backing stages); ADRs at their packages, numbers taken at
  writing: process-control syscalls of a child; the app lifecycle contract (bands, background
  declarations through policyd); windowd never blocks on an app; one memory authority (memd
  policy, execd mechanism, kernel last resort); the compressor before swap (the "0068"
  placeholder retired — that number is ADR-0068 modal semantics).

## The layer's six steps (each with a board gate)

| # | Step | Builds | Gate (QEMU → board) |
|---|---|---|---|
| 6a | **Measure, then free what dies** | RSS over open/close loops; launch stage markers; the exit funnel frees sole-owned VMOs (a VMO dies with its last capability; `sys_vmo_destroy`'s sole-owner rule unchanged); the OS allocator frees by default (small classes + a bounded large-block list with neighbour coalescing, deterministic); gpud on that allocator; windowd closes a dead client's surface cap | `KSELFTEST: vm exit reclaim ok (vmos= bytes=)`, `SELFTEST: rss loop ok (cycles=10 leaked=0)`, `SELFTEST: launch stage (spawn_us= entry_us= create_us= present_us=)` (a measurement, no `ok`), `gpud: gpu heap (used= of=)` flat over a windowd restart; `[PASS] board-visible` |
| 6b | **Unified memory + M2** — *cut 2026-10-22 into 6b-1 (the kernel commits on touch), 6b-2 (the layout block split, the arena reserved), 6b-3 (the allocator frees by default); CoW moves to 6c, where a private data mapping of a file first needs it — see "6b — the cut" below* | RFC-0100 filled; VMO kinds anonymous / zero-fill / contiguous-DMA / file (6c) / purgeable (6e); commit-on-touch through the user page fault (populate; fatal only for a violation); CoW; **the layout block split**: a contiguous scan-out part (wallpaper, retained, three ring slots ≈ 40 MiB at 1080p, inside the DMA reach) and a page-backed commit-on-touch atlas + gpud's caches, bound into the client's GPU context by `vm_bind` (which commits what it binds — the GPU MMU has no populate path), charged to the client; the app-host frame arena a reserved range committed on touch (`SELFTEST: frame arena spill` leaves the allow-list) | `KSELFTEST: vm commit on touch ok (reserved= committed=)`, `KSELFTEST: vm cow ok (…)`, `gpud: layout split ok (scanout= atlas_committed=)`; board: gpud's RSS ≈ 40 MiB + the touched rows (from 103.9) |
| 6c | **M3 shared code** | one page-cache VMO per bundle file (execd keeps the app-host ELF); text/rodata mapped RO(+X) from it, data/bss as CoW; a build gate for page-aligned `PT_LOAD`; `exec_copy.rs` empty for aligned images, then deleted (retired-name gate) | `KSELFTEST: exec image shared ok (text= ro= cow= copied=0)`; board: an app-host child from 11.6 MiB to its private pages, `rss_sum` drop |
| 6d | **Lifecycle** | kernel `task_suspend/resume/kill` of a child + a `killable` bit execd sets at spawn (apps yes, services no); windowd never parks on an app (acks = one-slot mailboxes flushed at the frame clock; intent = a bounded per-client queue flushed non-blocking each tick, overflow marks the client stalled by name; coalescing dropped as today); app death (EOF, RFC-0079) → caps closed, the band kept as a ghost, abilitymgr told Stopped; abilitymgr transitions from windowd's focus/visibility/occlusion; instances removed at Stopped; manifest `background = [...]` (audio, download, sync) through policyd (`nexus.permission.BACKGROUND`, deny-by-default); execd freezes/kills on abilitymgr's order; input on a Suspended app is a wake request (the first click wakes only) | `KSELFTEST: task freeze ok (pid= resumed=)`, `windowd: app frozen (id= queued= dropped=)`, `SELFTEST: ability transition ok (fg→bg→suspended)`, `windowd: app died (id= ghost=1)`; board: a frozen chat behind a focused app, the shell smooth |
| 6e | **Restore + accounting + pressure (M4 + M5)** | `@persist` wired (snapshot on Background/Suspended into statefs, restore before mount; manifest `restorable`); `memd` born — one authority: bands from abilitymgr, pressure from the kernel, kills through execd; M4 accounting (private/shared/committed/reserved/purgeable; **GPU memory charged to its client**); four pressure levels (normal/warn/critical/emergency) as fractions of the free total per bank from the FDT, the kernel notifying an endpoint as `irq_bind` does, memd fanning `OP_MEMORY_PRESSURE {level}` out to services and, through app-host, to apps as a DSL event; purgeable VMOs + seals (atlas bands, glass cache, blur cache); the ghost band until the first present; a warm restart under 200 ms | `KSELFTEST: mem watermarks ok (total= warn= crit=)`, `SELFTEST: mem pressure ok (level= dropped=)`, `gpud: gpu memory charged (client= bytes=)`, `SELFTEST: app restore ok (fields= warm_us=)`; board: `SELFTEST: mem pressure kill ok (band=suspended reason=oom)` on a victim task |
| 6f | **Compressor → swap → memd journal (M6 + M7)** | an in-kernel LZ4-class pool over anonymous dirty pages; swap on the `swap` partition through blkd (compressed pages only; clean file pages dropped, never written); memd's kill ledger in statefs; a `lowmem` QEMU profile (`QEMU_MEM=128M`) | `lowmem`: `KSELFTEST: mem compress ok (ratio= us_per_page=)`, `SELFTEST: swap out ok (pages=)`, `SELFTEST: memd journal ok`; board: regression only |

Edges: everything ← 6a; 6d ‖ 6b/6c; 6e ← 6b, 6d; 6f ← 6e (M6 ← M5). Kept from the order: S4 ← 6b
(M2), W1 ← 6c (M3), W2 ← 6e (M4).

## Decisions fixed up front (the plan of 2026-10-19)

1. **Unified memory, and what may be lazy.** The scan-out planes stay one contiguous, eagerly
   committed VMO inside the DMA reach (the controller reads base + stride and cannot fault). The
   atlas and gpud's private buffers (scratch, half, glass cache) become page-backed commit-on-touch
   VMOs charged to the client and bound through the GPU MMU; `vm_bind` commits what it binds, a
   purge touches only unbound pages. The layout's row formulas stay; only the backing changes.
   QEMU's virtio backing-entry bound is measured at 6b's P0 before that lane's attach form is chosen.
2. **Who kills, and the bands.** Kernel = mechanism: suspend/resume/kill of a child, a `killable`
   hint, a last resort at the emergency watermark after the handoff deadline (the largest anonymous
   RSS among killable tasks; no bands in the kernel). execd = the only caller. memd = policy:
   foreground > visible > background-declared > background > suspended, killed from the bottom,
   declared background only at critical, foreground/visible never. windowd feeds focus, visibility
   and occlusion to abilitymgr (`OP_TRANSITION`); abilitymgr publishes the bands to memd.
3. **A frozen app and IPC.** No send to an app ever parks. A Suspended app receives nothing; EOF
   closes the surface cap, keeps the ghost band, tells abilitymgr Stopped.
4. **The restorable contract.** `restorable = true` in the manifest; app-host snapshots `@persist`
   on every transition to Background/Suspended; a kill never waits for the app; relaunch restores
   before mount; windowd shows the ghost until the first present replaces it.
5. **Pressure transport.** Watermarks as fractions of the boot-time free total per bank (warn 25 %,
   critical 10 %, emergency 5 % free, floored at a page count), computed where `KINIT: mm frames`
   reads the tree; the kernel notifies an endpoint (the `irq_bind` pattern, reactive, no polling);
   memd alone waits on it. The same line prints different absolutes on QEMU 320M and the 4 GiB board.
6. **Shared code = a page cache, not fork.** execd holds one RO VMO per bundle file; the kernel
   maps text/rodata from it and data/bss as CoW; no process is cloned.

## 6a — measure, then free what dies (✅ 2026-10-20, board cycle 48)

**Scope in:** `source/kernel/neuron/src/syscall/api/task_image.rs` (+ a cap-table walk in
`task/mod.rs`), `mm/vmo.rs` (stats), `source/libs/nexus-service-entry/src/os/{global_alloc.rs,
freelist.rs}`, gpud's allocator adoption, `source/services/windowd/src/compositor/runtime/
app_window.rs` (cap close on client death), `source/services/execd/src/os_lite.rs` and
`source/services/app-host` (stage markers), `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt`,
the proof manifest, `scripts/board-test.sh`, the measurement folder. **Out:** the frame arena (6b;
no interim bump), commit-on-touch, freeze/kill and restore. **Widened 2026-10-20 (ADR-0073),
after cycle 45 showed a closed window never ends its process:** the rule "an object dies with its
last capability" for every kernel object, an exit that releases like a `cap_close` of each, the
EOF verdict independent of the wait mode, `PeerClosed` for a sender without a receiver, and the
death as an event — `child_exit_bind` (syscall 62), execd's reactive reap and its report to
abilitymgr (`OP_INSTANCE_EXITED`), the instance removed, windowd closing a dead client's window
by name.

**P0 — measure (board and QEMU, the stock tree)** — `docs/board/measurements/2026-10-20-memory-lifecycle-baseline/README.md`:
- The loop: a selftest rung (visible lanes) launches the chat through the shell's route and closes
  it, ten cycles; before and after each cycle `KSELFTEST: mm frames (… allocs= frees= vmos= vmo_bytes= spaces= rss_sum=)` and the `KINIT: mm space pid= rss= dma=` lines.
- Launch stages: `nsec()` at execd's exec, app-host entry, the first `SURFACE_CREATE`, the first
  acked present, windowd's mount → `SELFTEST: launch stage (spawn_us= entry_us= create_us= present_us=)`.
- Heap lines across one windowd restart (`SELFTEST: service restart ok` exercises it).

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | An app's VMOs leak on exit (surface, heap) | `vmos` / `vmo_bytes` grow by a constant per cycle over ten cycles |
| H2 | The exec image and the page tables return | `free` delta ≈ the VMO delta only; `pt_frames` flat |
| H3 | windowd's clone of the surface cap keeps the object alive after the kernel fix | `vmos` +1 per cycle until windowd closes it |
| H4 | gpud's heap grows per windowd restart | `gpu heap used=` rises across the restart rung |
| H5 | The app-host child's 11.6 MiB = the image + heap/arena, and all of it returns | the space line vanishes; `free` recovers within a page table's worth |
| H6 | Launch time is the image copy, not the bundle read | `entry_us − spawn_us` ≥ half of `present_us` → 6c's number; else the DSL lane's |

**P1 — host.** The cap table yields its VMO ids (beside `endpoint_send_ids`);
`exit_current_and_release` takes the table before it is dropped and destroys every VMO with
refs == 1 and no mapping in any space; `cap_close` on a VMO cap does the same (refcount death).
The allocator: the free list becomes the default (the feature a no-op for one release, then a
retired-name gate) plus a bounded large-block list with neighbour coalescing, deterministic
first-fit; tests for reuse, double-free refusal, zeroing on reuse. gpud adopts the service entry's
allocator in place of its own bump (the "heap never frees" comments go).
**P2 — QEMU.** windowd closes `vmo_slot` and the event-channel cap on `PeerClosed`; the loop rung
prints `SELFTEST: rss loop ok (cycles=10 leaked=0)` (its FAIL form names `leaked=` and `vmos=`),
the kernel `KSELFTEST: vm exit reclaim ok (vmos= bytes=)` at the boot-end fence; both REQUIRED
(`headless`: reclaim, `visible`: loop); `markers.txt`, the proof manifest and `qemu-test.sh` move
together. **P3 — board.** `[PASS] board-visible` with both rungs, `gpu heap` flat over the restart,
the README's after-numbers. **P4 — gate sweep.** The loop gate shown red once on the stock tree
(before P1) and archived; `just test-all` green; the docs swept (this ledger, RFC-0100's lifetime
line, CHANGELOG, `docs/testing`).

### 6a — results so far

- **P0 ✅ 2026-10-19 (headless, the stock tree):** every holder leaves exactly one object of
  1 048 576 B and 256 frames fewer; the exec phase's own children left theirs too (the record
  went from 40 objects after the greeter to 57 at the loop's start). `SELFTEST: rss loop FAIL
  (cycles=10 leaked=10485760 vmos=10)` archived (`headless-stock-loop-lines.txt`). H1, H2 ✅.
- **P1 ✅ 2026-10-19 (host):** `mm/vmo_lifetime.rs` (the rule, `test_reject_reclaim_shared_vmo`,
  `test_reject_reclaim_mapped_vmo`), `mm/reclaim.rs` (the reap: the zombie's `orphan_vmos`
  recorded at `exit_current` from its table, the ids its space mapped read before the space is
  destroyed, each freed when no table names it and no space maps it; the last `cap_close` of a
  `Vmo`/`VmoRo` capability likewise), `CapTable::vmo_ids`, `VaSpace::vmo_ids` (host-tested);
  `sys_vmo_destroy` untouched. 128 kernel host tests; `just diag` zero warnings.
- **P2 ✅ 2026-10-19 (headless):** the series flat (52 objects, `free` 28 908 through ten
  cycles), `SELFTEST: rss loop ok (cycles=10 leaked=0)`, `KSELFTEST: vm reclaim (vmos=18
  bytes=11010048)` — the ten holders and eight objects of the earlier children. Markers declared
  (`exec.toml`, `end.toml`), REQUIRED in `scripts/qemu-test.sh` and the board ladder.
- **Scope refined by the measurement:** windowd already closes a client's surface capability
  when its window closes (`app_surface.rs`), so with P1 the surface VMO of a closed app dies
  with it — the gap is an app that dies without its window closing, 6d's death detection. The
  OS services' heaps are static arrays whose growth is invisible to the kernel's record until
  6b commits them on touch; the allocator's freeing by default (and gpud's boxed executor per
  windowd restart, 10 KB above the free list's largest class) is judged there, where growth
  becomes RSS the `KINIT: mm space` line shows.
  **2026-10-22: the bounded large-block list was pulled forward by TASK-0344** (under the existing
  `small-object-free-list` opt-in: app-host, vfsd, screencapd) — the chat's spilled layout lost
  ~155 KB of base heap a keystroke and the app died after ~20; the default-on allocator for every
  service and gpud's adoption stay here.
- **P3, cycle 45 (board, 2026-10-19):** the loop green on four harts (`rss loop ok leaked=0`,
  ten `by=reap` reclaims, the create-phase guard holds); **a window closed with its button never
  ends its process** — nine launches, nine live app-hosts, `abilitymgr` instances 4…9, no
  `APPHOST: window closed - exiting` in any log ever recorded. Root cause in the kernel: the
  EOF verdict lived in `recv`'s blocking branch alone, and app-host receives non-blocking after
  a waitset wake (TASK-0324 P7-d) — it saw `WouldBlock`, the latch was consumed, the next wait
  parked forever. Behind it: two endpoints per launch owned by execd and closed by nobody (ids
  never reused, 384 in the table, 96 per owner — a launch counter), an exit that left the
  task's timers/waitsets/fences in their tables, no sender-side `PeerClosed`, and a reaper that
  swept only on its next request.
- **P3b built 2026-10-20 (ADR-0073, "like a phone"):** the kernel's one exit funnel releases
  every owned object and judges every named endpoint (`task_image.rs`, `ipc_reclaim.rs`,
  `ipc/lifetime.rs`; `ipc_eof` truth tables host-tested; ids reused once nothing names them;
  `MmStats` v2 counts endpoints/timers/waitsets/fences); the EOF verdict independent of the
  wait mode; a sender without a receiver gets `PeerClosed`; `child_exit_bind` (62) and
  `OP_CHILD_EXITED`; execd's declared child-death pair (`CHILD_EXIT`, named slots pinned by
  init), its waitset loop and reactive reap, the report `OP_INSTANCE_EXITED` to abilitymgr
  (execd's kernel sender id only, `test_reject_death_report_from_anyone_but_execd`), the
  instance removed (`abilitymgr: stop (inst= app= pid= code= live=)`,
  `forty_launch_exit_cycles_never_fill_the_table`); windowd closes a dead client's window by
  name (`windowd: app died (slot=`). Four kernel selftests every boot; the loop's verdict now
  `SELFTEST: rss loop ok (cycles=10 leaked=0 kobjects=0)`. Proof: QEMU lanes, then cycle 46.
- **QEMU 2026-10-20:** headless and visible green with the four kernel selftests,
  `execd: reaper armed`, `rss loop ok (… kobjects=0)`, `vm reclaim (… endpoints=3)`; the smp
  lane showed `KPANIC: tlb shootdown ack timeout hart=0 … activity=class4` after the MMIO-deny
  fixture's kill in 2 of 5 runs (0 of 11 before this tree): the boot hart's idle spin never
  polled the shootdown mailbox (a lock miss or an IPI trap were its only acks, and a consumed
  doorbell left neither while the requester ran with the lock dropped). It polls each turn now;
  0 of 3 runs after. The lane's other reds (`ipc call budget`, `console off lock`,
  `virtio-blk: timeout`) are in the 2026-10-19 runs too. The visible lane shows
  `settingsd: event send FAIL chan=11 err=QueueFull` twice during the registration burst (the
  shared windowd server queue under the new interleaving; settingsd's resync heals it on the
  next change) — noted, not a rule's fault.
- **Cycle 46 (board, 2026-10-20) — the chain holds:** after every close
  `APPHOST: window closed - exiting` → `KINIT: vm reclaim (… by=reap)` → `execd: exit notified
  (pid= code=0 reason=clean to=abilitymgr)` → `abilitymgr: stop (inst= app= pid= code=0 live=2)`;
  kernel objects flat at 266, endpoint ids 233–235 reused in a cycle, the four kernel selftests
  on four harts, `rss loop ok (… kobjects=0)`. The reap lines named one more leak: one
  129-page VMO per open/close (`vmos` 59 → 64) — execd's own capability to the payload VMO,
  never closed after the copy into the child. Closed (mint→grant→close).
- **Cycle 47 (board, 2026-10-20):** `free`/`vmos` flat across four closes (973 380 / 58).
  The operator's experience, though: slow, and twice "crashed". Read from the log: both boots
  ended in `hidrawd inputd send fail backpressure` — the input chain parked because windowd
  parked on an app (a tap is a CRITICAL delivery, blocking) while the app parked on windowd
  (its present): the mutual wait ADR-0075 / 6d names, now the operator's main pain. The
  slowness is the GPU: `gpu frame us … wait=66 ms` at 4.3 glass layers (Control Center plus
  windows; cycle 45 ran 28 ms at 1.6), i.e. a slideshow during open/close animations; and the
  first frame of the large-surface apps (settings 6.7 s, stash 4.7 s). This tree's own cost:
  three table scans per endpoint capability close — `ipc call budget rt=` 43–44 → 47 µs on
  the headless lane — folded into one census (`eof_scan::census`).
- **Next, in this order (the operator's demand: instant, no freeze, smooth):** (1) the
  non-parking compositor — ADR-0075 pulled into now: windowd never blocks on an app (a per-client
  bounded mailbox, a stalled client named and ghosted, its input dropped by name), app-host
  never blocks on windowd (presents are non-blocking, retried on the frame pulse); (2) the
  frame cost with glass (blur only the damaged region, cache static backdrops — gpud,
  measured per frame); (3) the launch: shared code without the copy (6c), the first frame of
  the DSL apps (atlas, layout) measured by stage; zero-copy wire (the capnp SSOT gate of the
  gfx/driver track) where a copy shows up in `ipc stats (copies= copy_bytes=)`.
- **Cycle 48 (board, 2026-10-20) — 6a's gate green:** the census fold and the payload close on
  the board; the chain at every close, the record flat (`free=973380 vmos=58 kobjects=266`),
  no stall, the operator's full series acked (tiling included), `[PASS] board-visible: 73
  rungs, 8 operator markers`; the headless lane's IPC call back at 43 µs. 6a done; its
  Definition of Done met (measurement folder with both lanes' numbers, host `test_reject_*`
  and truth tables, headless/visible/board green on REQUIRED, the loop gate shown red once on
  the stock tree). Carried to 6b: the service allocator's freeing by default, gpud's boxed
  executor per windowd restart.

## 6b — the cut (2026-10-22, after the tree was mapped)

The operator, 2026-10-22: "the UX lane pauses; layer 6 is finished first". Three surveys of the
tree first (kernel memory path, gpud's layout block, the services' heaps), because 6b's row was
written before the code was read:

- **Kernel.** Every page is committed eagerly: an `Anon` VMO at create (buddy blocks, zeroed in
  the create's phase B), `vm_map` writes every PTE with A/D set, exec allocates and zeroes every
  RW segment's `.bss` and the stack (`exec_image.rs`) — a 4 MiB static heap costs 4 MiB of frames
  and a 4 MiB memset at every spawn. The user page fault kills (`fault.rs:372`); no zero page, no
  per-page state, no CoW. A `Contiguous` VMO takes one buddy block rounded up to a power of two and
  keeps the tail: gpud's layout block is 26 325 pages in an order-15 block — **6 443 pages
  (25.2 MiB) held and counted nowhere**. RSS counts mapped region lengths, not committed pages.
  **The kernel reads and writes user memory directly inside syscalls** (`SUM=1`, a range check
  only: IPC, `vmo_read/write`, `mm_stats`, exec), so an uncommitted page touched there would be a
  kernel fault — a panic today. `vm_unmap` never re-checks the object: a VMO whose last capability
  closed while it was mapped is never freed after the unmap (6a's rule has this gap).
- **gpud.** The layout block is 13 planes, 102.8 MiB (wallpaper, retained, three ring slots, an
  eight-plane atlas), beside it the boot splash (7.9 MiB, kept for gpud's life) and the cursor;
  gpud resident 124 MB on cycle 63. The display controller scans out only the three ring slots
  (6 075 pages must be contiguous); the GPU needs no contiguity (4 KiB pages, a page table per
  context, `vm_bind` per page). windowd writes the atlas with `vmo_write` and never maps it; QEMU
  attaches backing by scatter-gather (254 entries an attach).
- **Services.** gpud runs on the service entry's 384 KiB bump without the free lists; no lane
  restarts windowd (6a's H4 never ran); the small classes have no double-free check. app-host's
  `.bss` is 5 MiB (heap 4 MiB + arena 1 MiB): cycle 63's chat used at most 0.85 MB of its heap and
  the arena in full (it spills); every app-host child is 12.0 MB resident.

| Step | Builds | Gate |
|---|---|---|
| **6b-1 the kernel commits on touch** | a lazy anonymous VMO kind (a flag on `vmo_create`; `Contiguous` stays eager); a region keeps its VMO offset and `vm_map` writes no PTE for an uncommitted page; the user fault populates — a zeroed frame, a 4 KiB PTE — inside a mapped region with the access's right, and stays fatal otherwise; the kernel commits a user range before it touches it (one range primitive for every syscall); exec's `.bss` tail and the stacks lazy, so every static heap and the arena cost only the pages touched; a contiguous object's buddy tail returned; MmStats v3 (committed, reserved; RSS = committed) | `KSELFTEST: vm commit on touch ok (reserved= committed= us_per_fault=)`; board: an app-host child from 12.0 MB to its touched pages, `free` up by the layout block's tail |
| **6b-2 the layout block split, the arena reserved** | the ring slots contiguous, the wallpaper/retained/atlas page-backed and lazy, bound into the GPU per band (`vm_bind` commits what it binds), windowd's `vmo_write` commits; the splash freed after the reveal; QEMU's per-band attach; the atlas's touched rows counted first (P0); app-host's arena a reserved range sized for the largest frame (the spill leaves the allow-list) | `gpud: layout split ok (scanout= atlas_committed=)`; board: gpud from 124 MB to the slots + what is touched |
| **6b-3 the allocator frees by default** | the free lists for every service (a double-free check for the classes), gpud on them, a windowd restart rung (6a's H4 measured at last), the "never frees" comments swept; `vm_unmap` re-checks the object (6a's gap: a VMO whose last capability closed while mapped) | a heap line per service flat over a windowd restart; `test_reject_double_free` for both lists |

Copy-on-write moves to **6c**: nothing in 6b maps a page privately that another holder shares; the
file-backed kind is the first that needs it.

**6b-1 — three lenses.**
- **Scope** — kernel: `mm/vmo.rs` (the lazy kind, per-page frames, committed/reserved),
  `mm/va_space.rs` (a region's VMO offset), `syscall/api/vm_map.rs` + `mm/vm_ops.rs`,
  `core/trap/handler.rs` + `fault.rs` (populate or kill), the user-range check (commit before
  touch), `syscall/api/exec*.rs` (the `.bss` tail and the stack), `mm/frames` (a contiguous
  object's tail), the unmap's re-check, `mm/usage.rs` + `mm/accounting.rs` (MmStats v3);
  `nexus-abi` (the flag); RFC-0100 §2; ADR-0077. **Out:** CoW and the file-backed kind (6c),
  purge and pressure (6e), dirty tracking (A/D stay eager; Svadu/Svade are absent from the board's
  tree — 6e/6f's question), the layout split (6b-2), superpages for lazy regions.
- **Invariant** — a fault populates only inside a mapped region with the access's right
  (`test_reject_populate_outside_a_region`, `test_reject_write_fault_on_a_read_only_region`,
  `test_reject_exec_fault`); a page is committed once (a second fault on it is spurious, never a
  second frame); a page is zeroed before it is mapped; the kernel never touches an uncommitted
  user page (the range check commits first; a missed site stays a loud KPGF); a returned tail is
  never handed out twice; out of frames at a fault is a named exit, never a panic.
- **Contract** — RFC-0100 §2 (kinds, commit, the fault), ADR-0077 (the user fault populates;
  `vmo_create`'s lazy flag), MmStats v3.

**6b-1 P0** — `docs/board/measurements/2026-10-22-commit-on-touch/README.md`: cycle 63's numbers
are the stock tree's (frames in use, every space's RSS, the layout block's tail), the decision
lines written before the code.

**6b-1 P1 + P2 built 2026-10-23 (ADR-0077, RFC-0100 §2 filled).** Host: `frames::trim` (the kept
pieces tile the prefix, the tail merges back — the board's 26 325 of 32 768 frames as a golden, a
proptest), `crate::lazy` (the verdict table: `test_reject_populate_outside_a_region`,
`test_reject_write_fault_on_a_read_only_region`, `test_reject_exec_fault`,
`test_reject_populating_an_eager_region`; a page committed once), `va_space` (`find`,
`record_lazy` unmergeable and outside the window) — 149 kernel host tests. Kernel: `VmoKind::Lazy`
(`commit`, `span`), the user fault fills (`fault.rs::fill_user_fault` → `mm::populate`), every
syscall's user memory through `ensure_user_range` (an unmapped or read-only buffer is `EFAULT`
instead of a kernel page fault), exec's `.bss` tails and stacks lazy (`exec_image::load_segment`),
a contiguous object's tail returned at create, `vm_map` of a lazy object records without PTEs,
MmStats v3. QEMU `headless` and `visible` green: the greeter's app-host 12.1 → 7.0 MB, `rss_sum`
−20.8 MB, +20.3 MiB free; `SELFTEST: vm commit on touch ok (… us_per_fault=6)`,
`KSELFTEST: userptr unmapped ok`. The kernel selftest `ipc eof nonblocking` handed a fake user
address to a dummy task without a space (a range check was all there was); its receiver now gets a
space with one lazy page — what a recv's buffers must be. `vm_unmap`'s re-check of the object
(6a's gap) moved to 6b-3.

**6b-1 ✅ board cycle 64 (2026-10-23, `docs/board/measurements/2026-10-22-commit-on-touch/`):**
the operator's series (greeter, calculator, chat ×3 with typing, settings, stash — each closed with
`code=0`); at cycle 63's fence +15 681 frames free (+61.3 MiB: the layout block's tail and the
untouched heaps and stacks), `rss_sum` −33.3 MB, an app-host child 12.0 → 7.0 MB committed (5.0 MB
reserved), windowd 6.0 → 4.4 MB; 5 µs a first touch; no `KPGF` on four harts, no app crash, no
`alloc-fail`. H1–H6 confirmed. Observed: `settingsd: event send FAIL … QueueFull` ×6 in the boot's
registration burst on the board too (6a's QEMU note; the subscriber resyncs).

**6b-2 — cut in two (2026-10-23).** **6b-2a** takes what needs no new wire and no new kernel path:
app-host's arena reserved for the largest frame (the spill leaves the allow-list), the board's
splash given back after the reveal, and the atlas's rows counted — 6b-2b's P0. **6b-2b** is the
layout block split itself (the ring slots contiguous; wallpaper, retained and atlas page-backed
and lazy; `vm_bind` per band; QEMU's per-band attach), sized from 6b-2a's count.

**6b-2a — three lenses.**
- **Scope** — `nexus-service-entry` (`FRAME_ARENA_BYTES` 1 → 4 MiB, `FRAME_ARENA_HALF`),
  `config/fail-marker-allow-board.txt` (the spill entry gone), `tests/dsl_apps_conformance`
  (the budgets held against the half when the file builds; the maximized window measured), gpud
  `backend/dc` (the splash an `Option`, given back at the first ring flip taken after the reveal),
  windowd `atlas.rs` + `loop_telemetry.rs` (`windowd: atlas rows (peak= used= of=)` on each new
  high), `scripts/check-image-budgets.sh` (comments). **Out:** the layout split (6b-2b), purging a
  reset generation (6e), a smaller app heap.
- **Invariant** — no recorded frame outgrows a half (a const assertion; on the board a spill is
  red again); the splash is given back only after the scan left it — a ring flip whose latch
  read clear after the reveal, never before the switch took, never after a refused one (it keeps
  the glass); the block is unmapped before it is destroyed, by its only holder.
- **Contract** — ADR-0065 (amended: the arena's size), ADR-0077 (`.bss` committed on touch),
  RFC-0093 §5 (the splash until the reveal).

**6b-2a built 2026-10-23; QEMU `headless` + `visible` green.** Host: the chat's frame is 293 848 B
of scene and 500 552 B of layout at 1440×814, 960×620 and maximized 1920×1024 alike (it lays out
its whole first page) — under half of the 1 MiB half. Lanes: `apphost: frame arena (…
of=4194304 spill=0)`; the greeter's app-host 7 000 064 B committed as before, 3 MiB more
reserved (5 029 888 → 8 175 616 B); `windowd: atlas rows (peak=800 used=534 of=8640)` at 1280×800
— 9 % of the atlas ever reached. The splash path runs on the board's display only (QEMU's 2D
splash resource is freed at attach since task #124).

**6b-2a board cycle 65 — decision lines** (baseline cycle 64, `board--2026-10-23T09-48-24`: gpud
`rss=123879424 dma=116137984` — the layout block 107 827 200 B + the splash 8 294 400 B + the
cursor; `free=991583` at the boot's census, which follows the reveal and the first flip):

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the arena holds the largest frame | no `SELFTEST: frame arena spill FAIL` over the chat series, maximize included; the chat's `peak=` under 2 × 1 MiB |
| H2 | the splash comes back | `gpud: splash released (bytes=8294400)` once, after `gpud: dc reveal flip ok`; gpud `dma=` 8 294 400 lower, `free=` ≈ 2 025 frames higher at the census |
| H3 | the atlas is mostly never touched | `windowd: atlas rows (peak=…)` over the series: 6b-2b's committed atlas ≈ peak × 7 680 B against 66.4 MB eager |
| H4 | nothing regresses | no `KPGF`, no app crash, the fleet `ready`, the chat types |

**6b-2a ✅ board cycle 65 (2026-10-23, `board--2026-10-23T10-29-54`,
`docs/board/measurements/2026-10-23-layout-split/`):** `[PASS] board-visible` (76 rungs, 8 operator
markers, no FAIL marker). H1: no spill over the series, the chat maximized twice — its frame
789 164 B of the 4 MiB arena (cycle 64: 524 288 of 1 MiB, `spill=1`); the gate shown red on cycle
64's capture (`gate-red-cycle64.txt`). H2: `gpud: splash released (bytes=8294400)` after the first
ring flip — gpud −8 294 400 B resident and DMA, +2 026 frames free at the census. H3: the atlas
reached 3 676 of 8 640 rows (`used=` at most 2 907): a lazy atlas ≈ 28.2 MB committed against
66.4 MB — 6b-2b's sizing. H4: the ladder's two deliberate faults only, no app crash. Observed for
6b-3: one object replaced by a larger one (+5 345 280 B, count unchanged) across the session with
the clipboard restore, a tile, light mode and the screenshot, flat afterwards — owner unnamed until
6b-3's per-service lines.

**6b-2b — one pool with the GPU (ADR-0078; the operator 2026-10-23: no fixed GPU memory, a share
of the machine like the desktop systems with unified memory, the whole RAM usable).** The board's
layout block becomes two objects over one row numbering: the scan-out ring (contiguous, inside the
display controller's reach) and the surfaces — wallpaper, retained plane, atlas — one lazy object
made for the GPU, bound sparse into windowd's GPU context and entered page by page when a frame
first samples it. The GPU's ceiling and the surfaces' reservation are fractions of the RAM read at
boot.

**6b-2b — three lenses.**
- **Scope** — kernel: `mm/vmo.rs` (a lazy object made for a device takes its frames from the
  device's reach), `syscall/api/vmo.rs` (the flag rule), `syscall/api/vmo_runs.rs` (naming a lazy
  range commits it), `selftest/vmo_runs.rs`; `nexus-abi` (`vmo_create_for_lazy`, a lazy `DmaVmo`);
  `nexus-display-proto` (the grant: whole planes); gpud: `budget.rs` (the ceiling and the
  reservation from RAM, pure), `backend/gpu/surface.rs` (the sparse binding, the pages a rectangle
  touches — pure parts host-tested), `bos.rs` + `device_v2.rs` (a buffer over the lazy object,
  bind without entering, enter a range), `desktop*.rs` (the ring and the surfaces bound apart),
  `backend/dc/*` (the ring its own object; the surface object, its size from the budget),
  `framebuffer_grant.rs` (the grant's length); windowd: `atlas.rs` + the grant (the atlas's rows
  from the granted length). **Out:** QEMU's per-band attach (6f), purge and decommit (6e), GPU memory
  charged to its client (6e, M4), app surfaces of their own (layer 8, G5), a two-level lazy table,
  the GPU page-table pool sized from the ceiling.
- **Invariant** — a device is never told a page that is not committed, and a lazy object's frames
  lie inside its device's reach (`KSELFTEST: vmo lazy device ok (…)`; `test_reject_naming_past_the_object`);
  no draw samples an unentered GPU page — the pages entered come from the descriptor the GPU reads
  (`every_texel_a_descriptor_reads_lies_in_an_entered_page`); the scan-out ring stays one
  contiguous block; a GPU buffer past the ceiling is refused (`test_reject_a_buffer_past_the_ceiling`);
  windowd builds only on a grant of whole planes, at least the layout
  (`test_reject_a_grant_of_partial_planes`).
- **Contract** — ADR-0078; RFC-0100 §2 ("One pool with the GPU"); the grant's length (no new wire).

**6b-2b P0 (from cycle 65, `docs/board/measurements/2026-10-23-layout-split/`):** gpud
`rss=115585024 dma=107843584` — the layout block 107 827 200 B eager + the cursor; the atlas drawn
to row 3 676 of 8 640 in a full series; gpud's own GPU buffers (scratch 8.3 MB, half 4.6 MB, glass
cache 7.9 MB) are page-backed and counted in no space's RSS; the GPU's page-table pool had 957 of
1 024 pages free before the desktop opened. Decision lines for the cycle after the build:

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the display needs only the ring contiguous | `gpud: layout split ok (ring=24883200 …)`; gpud `dma=24899584` at the census (the ring + the cursor; from 107 843 584) |
| H2 | the surfaces cost what is drawn | gpud `rss=` ≈ 50 MB at the census (the ring 24.9, the wallpaper 8.3, the atlas rows drawn by then ≈ 10, its own 7.8; from 115.6 MB) — the retained plane uncommitted until a freeze; `free=` ≈ +15 000 frames |
| H3 | the capacity follows the machine | `gpud: gpu memory (ram=4275466240 ceiling=2850310826 surfaces=530841600 scanout=24883200)`; `windowd: atlas rows (… of=63720)`; `gpud: layout split ok (… held= ceiling=2850310826)` with `held` ≈ 580 MB |
| H4 | nothing regresses | the series as cycle 65 (the chat maximized, tiled, a screenshot — the freeze commits the retained plane), no GPU fault, no `FAIL dc present`, no `FAIL gpu memory`, the frame times of cycle 65 |

**6b-2b built 2026-10-23; QEMU `headless` + `visible` green.** Kernel: a lazy object for a device
commits inside its reach, `vmo_runs` commits what it names (at most 64 pages a call: the commits
run under the kernel lock) — `KSELFTEST: vmo lazy device ok (pages=4 named=3 runs=3 deny=3)`;
`lazy::{nameable, merge_runs}` host-tested (151 kernel tests). gpud: `gpu_share` (the ceiling two
thirds of RAM, the surfaces an eighth in planes, the ledger) and `surface_pages` (the rows' pages,
the entered bitmap) host-tested; the board's display makes the ring (`Framebuffer::scanout`) and
the surface object (`dc/surfaces.rs`, `LazyDmaVmo`), grants the surfaces with their length; the
GPU adopts the ring by bus and the surfaces as a lazy buffer bound sparse (`device_sparse.rs`,
`desktop_surface.rs`), entering the base plane's and each band's rows before a frame's first pass;
the layout's GPU window moved to its own GiB (the private buffers keep their addresses).
`nexus-gfx`: every layout texture a frame draws is a piece of the base or a layer's band
(`frame_board.rs`). windowd: the atlas grows to the granted object's end
(`AtlasAllocator::grow`, `rows_of_object`). The grant refuses partial planes. QEMU keeps the fixed
layout (`of=8640`); the split runs on the board's display only.

**6b-2b ✅ board cycle 66 (2026-10-23, `board--2026-10-23T12-22-07`,
`docs/board/measurements/2026-10-23-layout-split/`):** `[PASS] board-visible` (78 rungs, 8 operator
markers). gpud `rss=49373184 dma=24899584` at the census (from 115.6 / 107.8 MB), +13 165 frames
free, system committed −56 MB; `gpud: gpu memory (ram=4275462144 ceiling=2850308096
surfaces=530841600 scanout=24883200)`; windowd `of=63720`, the atlas reached 5 833 rows with more
windows open; no GPU fault in the session; a steady frame as cycle 65's. Carried to 6b-3's P0: a
band's first entry (named, entered, faulted into gpud's CPU view for the clean) measured apart
from `clean=`. **Finding routed to layer 8 (the operator's call):** imed stopped draining after a
focus change — it blocks in a clock-less call per focus change (the `ime.personalization` read,
the store's flush) and refuses windowd's and inputd's sends from then on, so typed text reached no
field; settingsd and statefsd lived on; not on the split's path, the first such stall in fourteen
cycles. The cure is event-driven (the setting a watch, the flush asynchronous), and the board gate
does not read `WINDOWD:`/`inputd:` FAIL lines.

**6b-3 — every service's heap frees (cut 2026-10-23).** The free lists for every service (the
opt-in feature retired), the small classes' double free refused, gpud on them and the "never
frees" comments made true; logd's journal loses the lifetime budget it kept for the bump (after
256 KiB appended over its life it refused every record — its records live in slots allocated
once, so the budget guarded nothing); `vm_unmap` re-checks the object (6a's gap); the evidence: the kernel
names a space that reached a new high at each reap (`KINIT: mm grew (…)`), gpud's frame line a
band's first entry apart from the clean (`enter=`, 6b-2b's carry). **Moved to 6d:** the windowd
restart rung — a compositor restart (the grant again, a new GPU context, gpud giving back the
dead one's buffers) is the lifecycle's: a death is an event.

**6b-3 — three lenses.**
- **Scope** — `nexus-service-entry` (`freelist.rs`: the bitmap over the base heap and the
  refusal; `lib.rs`, `os/global_alloc.rs`, `os/frame_arena.rs`: no feature gate; `Cargo.toml`), the
  three opted-in services' manifests, app-host's probe (`refused=`), `scripts/check-retired-names.sh`
  (`small-object-free-list` retired); kernel `mm/vm_ops.rs` + `syscall/api/vm_map.rs` (the unmap
  answers its object, the rule runs), `selftest/lifetime.rs`, `mm/growth.rs` (pure) +
  `mm/usage.rs::report_growth` + the reap's call; gpud `desktop_surface.rs` + `desktop.rs`
  (`enter=`); logd `journal.rs` + `os_lite.rs` (the lifetime budget goes); the comment sweep. **Out:** the windowd restart (6d), purgeable memory and limits
  (6e), a general-purpose allocator (the lists stay bounded and deterministic), coalescing classes.
- **Invariant** — a block is on a list at most once: a second free is refused without a byte
  written (`test_reject_a_double_free_of_a_class_block`, the large list's `test_reject_a_double_free`);
  a block not the base heap's never enters a list (`test_reject_freeing_a_block_that_is_not_the_heaps`);
  an object nobody names dies at its last unmap (`KSELFTEST: vm unmap reclaim ok (bytes=)`); the
  growth line reads committed bytes only, names a new high once (`growth.rs` tests,
  `test_reject_naming_a_reused_pid_or_one_past_the_table`); a rotating journal keeps logging past
  any lifetime total (`journal_keeps_logging_long_past_its_lifetime_total`).
- **Contract** — ADR-0065 (amended: the lists every service's), RFC-0100 §1 (the unmap is the
  rule's third caller).

**6b-3 P0 (QEMU `visible--2026-10-23T13-07-58`, the lists still opt-in):** `KSELFTEST: vm unmap
reclaim ok (bytes=8192)` with `KINIT: vm reclaim (… by=unmap …)`; the growth lines name the boot's
warm-up (gpud's 108 MB framebuffer at the attach, the greeter's app-host, `updated`, the selftest
client) and, over the rss loop's ten reaps, no service — QEMU's lane churns too little to tell the
lists apart; the board's series is where it shows. The board's baseline is cycle 66's: free frames
fall by 9–20 at each app's close (cycle 65 the same).

**6b-3 QEMU finds (2026-10-23).** (1) The double-free bitmap measured from the heap's first byte,
the blocks are cut on the absolute 16-byte grid: 14 services refused every free as foreign and the
lanes stayed green — the base is now rounded to the grid, and a refusal is the FAIL-shaped
`SELFTEST: alloc free refused FAIL (…)`. (2) `visible--2026-10-23T15-00-28`: netstackd died with
`INST @ sepc=` a kernel text address — `context_switch_to_task` wrote `sepc` with supervisor
interrupts on, a timer trap in the window up to `sret` overwrote it (a kernel race older than 6b-3,
never seen in 848 archived logs). The switch now masks SIE first and writes the frame's `sstatus`
with SIE clear; `sret` enables it from SPIE. Both lanes green after it
(`headless--2026-10-23T15-56-15`, `visible--2026-10-23T16-00-54`).

**6b-3 board cycle 67 (`board--2026-10-23T16-12-49`, build dev-295a).** H1 ✅ no `alloc-fail`, no
refused free; the census within a few pages of cycle 66 (init-lite +17 pages, selftest-client
−84). H2 ✅ after the desktop only gpud is named (+39.8 MB with three apps open, given back at their
closes: free +2 962, +911 frames); no system service grows; the rss loop's reaps fall by 2 and 5
frames, then flat (cycle 66: 3 and 12). H3 ✅ `vm unmap reclaim ok (bytes=8192)`. H4 ✅ `enter=`
non-zero in the six windows a window opened in, `0/0` in fifteen steady ones. H5 ✅ the series
(tiling by drag and chord, screenshot, clipboard, modal), no FAIL kind absent from earlier board
logs, no netstackd fault. `board-visible` is red on the operator's `typed` alone: letters typed
into an app do not show (the known keyboard defect, in the UX lane); `typed` and `gpu-desktop`
(whose text names typing) left unacked.

## 6c — the cut (2026-10-23, with the operator)

**Measured first (the tree, the built ELFs, board cycle 67).** The app-host ELF is 2.0 MB, not
7 (RFC-0080 took the atlas out): R 1.42 MiB + RX 0.51 MiB shareable per child, one data page
(17 B of file bytes), 8.4 MB `.bss` already lazy. The exec ABI takes a user pointer + length only
(syscalls 13/17); the kernel copies every file byte into fresh frames. The images nest: app-host
`include_bytes!` in execd, execd and every CORE service in init-lite, init-lite in the kernel
image (`.rodata` 9.7 MB, never freed) — each level resident on its own (init-lite rss 6.4 MB,
execd 6.8 MB). A volume service is held twice: init keeps its payload VMO mapped for good
(respawn), the kernel copies it per spawn. The payload VMO's data starts at byte 16 and blkd DMAs
into it. `PT_LOAD`s are offset ≡ vaddr mod 4096, but a segment's first file page is shared with
the previous segment's last. `vm_map` refuses EXECUTE; `vmo_share_readonly` exists (RFC-0080).
Launch time is not the copy (6a's H6, refuted on both lanes): 6c is memory, not speed. The data
segments are 17–32 B: **CoW is not needed** (a private copy of one page per segment).

**Security finding (before any sharing).** User mode may flush cache lines on the board
(`enable_user_cache_maintenance`, Zicbom, every task alike), and the glyph atlas is already one
page set read by every app-host (RFC-0080) — a Flush+Reload probe of which glyphs another app
draws is plausible (not measured). Shared code widens the same surface. The fix is at the root:
cache maintenance in user mode only for a task holding a right for it (the DMA drivers); the
kernel sets `senvcfg` per task at the switch.

**How others do it (read 2026-10-23).** Fuchsia: CORE programs in a bootfs inside the boot image,
immutable verified VMOs, text mapped from the VMO, data a CoW child, executable only through the
VMEX right. iOS: code shared through the page cache, every code page checked against its
signature on page-in, never writable. OpenHarmony: Linux page cache + appspawn, signatures checked
at load. All share code; they differ in how hard they prove it immutable.

**The steps (each with a board gate):**

| # | Builds | Gate |
|---|---|---|
| 6c-0 | (before 6c) the OS build's own flags apply: `.cargo/config.toml`'s target flags were overridden by every recipe's `RUSTFLAGS` (no `+zbb`, no `share-generics` since 2026-06-03) | sizes + board frame times vs cycle 67 |
| 6c-1 | user cache maintenance only with a right; `vmo_seal` (one way, refused for a device-pinned or writably mapped VMO) and `exec_vmo` (ADR-0079): R/RX mapped from the sealed object's frames, the data page private, `.bss`/stack lazy; a build gate for page-aligned `PT_LOAD` file offsets; volume services copied once into a sealed image, verified **after** the seal, the DMA object closed | `KSELFTEST: exec image shared ok (text= ro= copied=)`, `test_reject_exec_unsealed_vmo`, `test_reject_write_map_sealed`, `test_reject_vmo_write_sealed`, `test_reject_seal_device_pinned`, `test_reject_seal_writable_mapped`, `test_reject_exec_vmo_unaligned_segment`, `test_reject_user_cache_flush_without_right`; board: volume services' copies gone |
| 6c-2 | the boot image instead of the nesting: nxboot loads a signed bootfs beside the kernel, the kernel hands it to init as one sealed object, CORE services and app-host exec from page-aligned ranges of it; the `include_bytes!` chain and syscalls 13/17 deleted (retired-name gate) — ADR-0060 holds (recovery still needs no volume) | `KSELFTEST: bootfs ok (files= bytes=)`; board: kernel image, init-lite and execd shrink, an app-host child to its private pages |

**6c-0 built 2026-10-23; board cycle 68 (`board--2026-10-23T17-18-42`).** `.cargo/config.toml`
is the one place the OS target's flags live; `scripts/build.sh` and `tools/os2vm.sh` unset
`RUSTFLAGS`, the justfile's OS recipes run through `os_cargo` and append lint flags with
`--config` (`os_lint_warn` / `os_lint_deny`); `RUSTFLAGS_OS` / `os_rustflags` retired (gate).
Zbb in 40 of 40 shipped ELFs (0 before), the total −0.4 %. The cycle's launch timings cannot
judge a codegen effect (×2 spread per app across cycles — the decision line is recorded as
wrong in the measurement folder). One full stall in a scene never tested before (four quadrant
tiles + the control center modal, the GPU overloaded) — filed by the operator as its own defect,
**TASK-0345**. Carried: `tokio` 1.51.1 → 1.51.6 in `Cargo.lock` (host-only; an advisory published
today turned `just check` red).

**Not in 6c:** CoW (measured unneeded), dropping clean code pages under pressure (6e/6f), a
per-page signature check on fault (the bootfs and the volume are verified whole before use).

## Definition of Done (per step)

1. The measurement folder answers the step's question with archived numbers from both lanes.
2. Host tests prove the step's invariant, with its `test_reject_*`.
3. `headless`, `visible` and `board-visible` are green on the REQUIRED ladder with the step's
   rungs and no new tolerated red.
4. The gate has been shown red once, with the transcript archived.
