---
title: TASK-0333 Memory, the efficient one + the app lifecycle — Block 4 layer 6 (RFC-0100): what dies is freed, unified memory with the GPU committed on touch, shared code, freeze/kill like a phone, restore, pressure, compression before swap
status: In Progress (6a ✅ 2026-10-20, 6b ✅ 2026-10-23 — 6b-1, 6b-2a, 6b-2b, 6b-3 (board cycle 67); 6c ✅ 2026-10-26 (ADR-0081, 989b1474); 6d ✅ 2026-10-27 (35005c8b, board cycle 72b) — 6d-4 → 6g after 6f (RFC-0112); 6e ✅ 2026-10-27 (ADR-0085/0086, both QEMU lanes + board cycle 73; the board's close-and-reopen rung open with the keyboard defect — after 6f); 6f-0 ✅ 2026-10-28 (bb6557f9, board cycle 74); 6f-1 ✅ 2026-10-28 (7004801e, ADR-0087, board cycle 75); 6f-2 ✅ 2026-10-28 (ADR-0088, board cycle 76); 6f-3 ✅ 2026-10-30 (ADR-0086 amended, ADR-0089 — the determinism package; test-all + board cycle 77) — 6f complete; next: the keyboard package, then 6g (RFC-0112); planned 2026-10-19 with the operator)
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
| 6e | **Restore + accounting + pressure (M4 + M5)** — *cut 2026-10-27 into 6e-0 (the kernel's watermarks + one sink), 6e-1 (`@persist` restore), 6e-2 (memd, the bands, the kill through the lifecycle authority), 6e-3 (the fan-out + purgeable VMOs), 6e-4 (GPU memory charged to its client); the `lowmem` profile and `reason=oom` are 6f's — see "6e — the cut" below* | `@persist` wired (snapshot on Background/Suspended into statefs, restore before mount; manifest `restorable`); `memd` born — one authority: bands from abilitymgr, pressure from the kernel, kills through execd; M4 accounting (private/shared/committed/reserved/purgeable; **GPU memory charged to its client**); four pressure levels (normal/warn/critical/emergency) as fractions of the free total per bank from the FDT, the kernel notifying an endpoint as `irq_bind` does, memd fanning `OP_MEMORY_PRESSURE {level}` out to services and, through app-host, to apps as a DSL event; purgeable VMOs + seals (atlas bands, glass cache, blur cache); the ghost band until the first present; a warm restart under 200 ms | `KSELFTEST: mem watermarks ok (total= warn= crit=)`, `KSELFTEST: vmo purge ok (`, `SELFTEST: mem pressure fanout ok (level= sent=)`, `gpud: gpu memory charged (client= bytes=)`, `SELFTEST: app restore ok (fields= warm_us=)`; board: `SELFTEST: mem pressure kill ok (band=suspended reason=oom)` on a victim task |
| 6f | **Compressor → swap → memd journal (M6 + M7)** | an in-kernel LZ4-class pool over anonymous dirty pages; swap on the `swap` partition through blkd (compressed pages only; clean file pages dropped, never written); memd's kill ledger in statefs; a `lowmem` QEMU profile (`QEMU_MEM=128M`) | `lowmem`: `KSELFTEST: mem compress ok (ratio= us_per_page=)`, `SELFTEST: swap out ok (pages=)`, `SELFTEST: memd journal ok`; board: regression only |
| 6g | **The compositor's restart (from 6d-4, RFC-0112)** — moved behind 6f by the operator 2026-10-27: a service's death is a re-resolve for its clients (init re-pins windowd's endpoint into every declared slot, execd re-grants the app-hosts, every client reconnects on EOF, gpud keeps the frame; the bands return as ghosts until the next present) | `windowd: restarted (ghosts=)`; board: kill windowd, the desktop returns |

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

**6c-1 cut in two (2026-10-24, the operator: evaluate who else needs it).** **6c-1a** — the
cache right alone (ADR-0079), its own board cycle; **6c-1b** — sealed images and `exec_vmo`.
Who needs user `cbo`, measured: one call site (`nexus_abi::cache_clean`/`cache_flush`), reached
by sdhci (blkd), xhcid, gpud and the selftest's DMA probe — each from its own device
capability's coherence. Future drivers of non-coherent devices (Ethernet, audio, camera, codec,
NPU, crypto) get the right with the capability init hands them, nothing per driver; coherent
devices' drivers do not; non-drivers never get the instruction — layer 8's zero-copy import
gets a kernel call over objects the caller may write (ADR-0079, built with its first user).
Found on the way: **a user-mode illegal instruction panicked the kernel** (`panic!("ILLEGAL")`)
— any app with an unknown opcode took the system down; it now kills the process
(`[USER-ILL]`, `reason=fault`).

**6c-1a built 2026-10-24.** Kernel: harts boot with user Zicbom off
(`platform::apply_user_cache_maintenance`), `AddressSpace::cache_maintenance` set at the first
`cbo` of a process holding `DeviceMmio { dma_noncoherent: true }` (`trap/cache_right.rs`, the
pure rule `cbo_rule.rs` host-tested: `test_reject_user_cache_flush_without_right`,
`test_reject_cbo_zero_and_other_misc_mem`, `test_reject_cbo_without_zicbom_or_on_a_hart_already_on`),
every `activate` writes `senvcfg` from the space. Userspace: `demo.cache` (execd image 7) →
`SELFTEST: cache maintenance refused ok` (REQUIRED, both lists); the DMA probe follows the
device's coherence. RFC-0098 C4 amended; measurement folder
`docs/board/measurements/2026-10-24-cache-maintenance-right/` (H1–H4 for cycle 69).

**6c-1a ✅ board cycle 69 (`board--2026-10-25T12-17-03`).** Exactly blkd, xhcid and gpud
granted, once each; everything they drive works; the refusal proven on QEMU and the board;
ladder 79/79, no new FAIL kind; ADR-0079 Accepted. `board-visible` red on `typed` alone (the
keyboard defect) and `tile` unacked (no chord tried). NEXT 6c-1b — sealed images + `exec_vmo`.

**6c-1b — a process runs from a sealed image (cut 2026-10-26, ADR-0080).** Measured first: the
exec ABI is pointer + length (13/17) with a copy of every file byte per process; `-z
separate-loadable-segments` puts every `PT_LOAD` on its own file page at +0.1–4 % size (app-host
2 022 120 → 2 028 544, metricsd 79 760 → 82 944, windowd 3 826 760 → 3 830 776; phdrs at 0x40, six
entries, inside page 0); the data segments are 8–32 B; the hand-made probe ELFs (demo-exit0,
hello, recv-wake) put their text at file offset 0x100 under VA 0x1000 — not mappable shared; a
volume service is read by blkd straight into the object init later maps and execs, hashed in
between (the device keeps the runs). **Three lenses.** *Scope:* kernel — `vmo_seal` (64),
`exec_vmo` (63) as the ONE loader (R/RX mapped from the sealed object's frames as `Fixed`
regions naming the object, RW file pages copied, `.bss`/stack lazy), 13/17 deleted, a sealed
object refuses `vmo_write`/writable `vm_map`/`vmo_runs`, a seal is refused for a writably
mapped, device-named, lazy or fixed object; ABI `vmo_seal`/`exec_vmo`/`sealed_image(bytes)`;
`.cargo/config.toml` `-C link-arg=-zseparate-loadable-segments`; one host crate
`tools/exec-image` (the layout rule + the tiny-ELF builder) used by nxb-pack, init-lite's and
execd's build scripts and the probe builders; bundlemgrd `OP_GET_BUNDLE_IMAGE` (bounce → image
→ seal → hash); init keeps the sealed capability (no mapping) for respawn, seals its embedded
images once; execd seals app-host and the probes once. **Out:** CoW, purging, the boot image
(6c-2), per-page signatures. *Invariant:* a page is executable only when mapped by `exec_vmo`
from a sealed object; a sealed object has no writer (`test_reject_exec_unsealed_vmo`,
`test_reject_exec_unaligned_segment`, `test_reject_exec_shared_file_page`,
`test_reject_vmo_write_sealed`, `test_reject_write_map_sealed`, `test_reject_runs_sealed`,
`test_reject_seal_writable_mapped`, `test_reject_seal_device_named`, `test_reject_seal_lazy`;
the kernel's boot proof `KSELFTEST: exec image shared ok (text= ro= copied=)` + `KSELFTEST: exec
image reject ok (…)`); identity stays the kernel's `service_id` from the name `exec_vmo`
receives. *Contract:* ADR-0080; RFC-0100 §2 (the sealed kind replaces the "file (6c)" row — the
page cache of a bundle is its sealed image, held by the spawner) and §4 (the seal); ADR-0060
unchanged (init spawns, bundlemgrd verifies). Measurement folder
`docs/board/measurements/2026-10-26-sealed-images/` (H1–H4 for cycle 70).

**6c-1b built 2026-10-26.** Kernel: `VmoObject { sealed, device_named }`, `vmo::seal` (refused for
lazy/fixed/device-named; the api layer refuses a writably mapped object through
`vm_ops::any_space_maps_vmo_writable`), `vmo_runs` marks the object device-named and refuses a
sealed one, `vmo_write`/writable `vm_map` refuse a sealed one; `exec.rs` rewritten around
`load_image` (R/RX → `vm_ops::map_runs_at` as `Fixed` regions naming the object; RW file pages
copied from the object's `span` pieces; `.bss`/stack lazy) + `exec_vmo_phase_a` (the task half)
+ `KINIT: exec image (pid= vmo= shared_pages= copied_pages= mappers=)`; syscalls 13/17 deleted,
63/64 added, phased/handler lists updated; `selftest/exec_image.rs` → `KSELFTEST: exec image
shared ok (text=1 ro=1 copied=1)` and `KSELFTEST: exec image reject ok (unaligned shared-page lazy
device-named writable-mapped unsealed)` (REQUIRED, both lists); host: `exec_vmo` refuses an
unsealed/non-VMO capability before any space. ABI: `exec_vmo`, `vmo_seal`, `sealed_image(bytes)`,
`exec_image_once` (one-shot probes). Build: `.cargo/config.toml` `-zseparate-loadable-segments`;
`tools/exec-image` (the rule host-tested + `tiny` builder + `rv` encoders) checks every image in
init-lite's and execd's build scripts and in nxb-pack (ELF payloads); the probe ELFs (demo-exit0,
hello — now generated —, recv-wake) sit on the page grid. Userspace: execd seals each embedded
image once (`State::images`), app-hosts share it; init seals the CORE images once
(`spawn::SealedImage`, kept for respawn), volume services via `OP_GET_BUNDLE_IMAGE` (bundlemgrd:
bounce → copy → seal → hash, `bundle served (… copy_ms= …)`), `VolumeSpawned { image }` (no
mapping, no `unsafe`); supervision's probes `exec_image_once`. Retired-names gate: the old
syscall and op names. Docs: ADR-0080, RFC-0100 §2 (the sealed row) + §4, 01-neuron-kernel,
CHANGELOG, measurement folder (cycle 70 lines).

**6c-1b first QEMU run (headless, 2026-10-26) — red twice, both in the kernel.** (1) The syscall
dispatch window was 64 wide (`MAX_SYSCALL`); number 64 (the seal) was dropped by `register`
without a word and init's first `vmo_seal` came back `invalid-syscall` → init panic. Window now
128, `register` asserts the window (fail-closed both ways); the host test
`the_seal_and_the_exec_dispatch_through_the_table` names it, though the kernel's syscall tests
compile only for the target (`mod syscall` is `cfg(target_os = "none")`) — the QEMU rung is the
proof. (2) `KSELFTEST: exec image shared FAIL (… freed=false)`: the second space's tear-down
reclaimed the image as an orphan (nobody named it, no space mapped it) before the selftest's own
`destroy` — correct kernel behaviour, wrong proof. The proof now reads: after the first space
goes, the image is still sealed with one mapper; after the second, it is gone (`kept`,
`reclaimed`).

**6c-1b QEMU green 2026-10-26** (`headless--2026-10-26T09-54-18`, `visible--2026-10-26T09-57-45`):
both ladders complete, both new rungs green, every exec'd service `KINIT: exec image (…
copied_pages=1 …)`, every volume service `bundle served (… copy_ms=0|1 …)` with its hash as
before, no `EXEC-REFUSED` but the selftest's own, the one tolerated FAIL kind as in every earlier
run (`init: FAIL declared slot route send slot`). At the kernel's frame census (same ladder point
as `visible--2026-10-24T18-24-17`): free frames 35 638 → 37 159 (+1 521 = +5.9 MiB), rss_sum
146 489 344 → 139 837 440 — the volume services' second copy, gone (H2's size). Two lessons for
the decision lines: (1) `rss=` counts a `Fixed` region in full, so a space's rss does not fall
when its text is shared — the frame pool does; cycle 70 reads H1/H2 from `free=`, not from an
app-host's `rss=`. (2) `mappers=` climbs 2 → 3 → 4 across pinched's three deliberate crashes:
the respawns run from the ONE sealed image (good), but init never reaps a dead child (no
`wait_child` in nexus-init), so each dead pinched keeps its space — a 6d item (the reaper for
init's children), not a 6c regression (cycle 69 had the same zombies, uncounted).

**6c-1b ✅ board cycle 70 (`board--2026-10-26T10-32-12`, dev-2deb812c).** H1–H4 green, results in
the measurement folder: the app-host image's `mappers=` followed the operator's apps 1 → 6 → 3 → 6
(execd's reaper brings it down), 18 volume services served with `copy_ms=0` (two: 3, 11) and hashed
after the seal, `KSELFTEST: exec image shared ok` / `reject ok` on the board, frame census `free=`
995 646 → 1 011 396 (+61.5 MiB; 16.6 MiB of it the shared text, Σ `shared_pages` = 4 256), FAIL
kinds the same three as cycle 69, boot milestone 11 at 2 000 ms (1 869), ladder 79 rungs, acks
desktop/pointer/tile/modal/clipboard/gpu-desktop. ADR-0080 Accepted. Carried on: init reaps no dead
child (6d); `WINDOWD: FAIL imed relay send` 8× (UX lane, typed defect). NEXT 6c-2 — the boot image.

**6c-2 — the boot image is one container (cut 2026-10-26, ADR-0081).** Measured first (cycle 70's
build): the kernel ELF 10.2 MB of which 9.7 MB is init-lite's ELF, init-lite 9.7 MB of which ≈ 8 MB
are the core ELFs, execd 6.8 MB of which ≈ 2.5 MB are app-host + the probe; every core program's
bytes three to four times in memory and copied twice at boot. *Architect:* the slot image becomes
`[header NXBI0001][flat kernel][bootfs NXFS0001]` under the unchanged NXBD digest (RFC-0089 §7
amended by ADR-0081); one bounded codec (`nexus-bootfs`) shared by the packer (`nx image payload`),
the loader, the kernel and init; the loader places the bootfs at the first 2 MiB boundary past the
kernel's footprint (the stack now inside the kernel's memsz) and hands the range over in
`/chosen/nexus,bootfs-start/-len`; the kernel excludes the range, adopts each file as a fixed
object born sealed (`vmo::adopt_sealed`; `vmo_seal` keeps refusing Fixed) and spawns init = file 0
through `spawn_sealed`, the same path as `exec_vmo`; init reads the table from slot 4, the files
from slot 32+i, and hands `app-host`/`recv-wake-probe` to execd's declared slots 31/32
(`nexus-service-topology::bootfs::HAND_OFFS`). *Security:* the trust is one digest — a flipped
bootfs byte behind a valid NXBD is `Reason::Digest` (loader flow test); a header that is not a
container, a size the descriptor does not cover and a container the window cannot place are three
typed refusals (`payload-header|payload-size|payload-window`); the kernel refuses a bootfs outside
a memory bank, overlapping its image or the tree, or with a bad table at its boot gate. *Pragmatist:*
init-lite's bespoke `link.ld` (origin 0x10400000) deleted — it links like every service; the
`NEXUS_DIRECT_KERNEL` boot retired (no loader, no bootfs); execd keeps its tiny probe ELFs;
`os2vm` boots the loader like the launcher. Not done: A/B at bootfs granularity, the volume.

**6c-2 built 2026-10-26.** Kernel: `core/bootfs.rs` (capture from `/chosen` before the frame
pool, placement check, adoption, `KSELFTEST: bootfs ok (files= bytes=)`), `init_caps` injects the
TOC at slot 4 and file i at 32+i, `load_init_elf` + `selftest/init_image.rs` deleted (selftest/mod.rs
2192 → 1782 LOC), `EMBED_INIT_ELF` gone from the build. Loader: `flow::placement`, the container
read in disk order with the digest streamed over header ‖ kernel ‖ bootfs, `nxboot: bootfs (at=
len=)`, 22 flow tests incl. three new `test_reject_*`. init: `bootstrap/bootfs.rs` (table page
mapped once, every file slot verified by kind and length), `ServiceImage` from the table,
`hand_off_to_execd`; init-lite 9 704 576 → 267 256 B. execd: `images::handed_over`, embedding
gone; 6 841 360 → 4 793 360 B. Build: `scripts/build.sh` writes `build/bootfs-manifest.json` and
packs `boot-payload.bin`; the launcher, `board-image.sh` and `os2vm` take the payload; the
harness requires `nxboot: bootfs (` and `KSELFTEST: bootfs ok (` (bringup.toml: the two
`segment copied`/`shared page` markers retired with the copier). Retired-name gate row added.

**6c-2 QEMU 2026-10-26.** headless red once — the new rung was listed after the exec selftests but
the kernel prints it before the banner (moved to the ladder's head); then headless, visible and
`ota-tamper` green (`headless--2026-10-26T16-16-36`, `visible--2026-10-26T16-19-47`,
`ota-tamper--2026-10-26T16-23-15`: a flipped byte behind a valid NXBD is `verify FAIL (slot=b
digest)` with the container). `ota-fallback` red: the fixtures' os-B was "the running image + a
512-byte build-id trailer" — bytes past the container, `verify FAIL (slot=b payload-size)`. Fixed
at the root: every disk assembler reads the slot image through one `read_kernel` that checks it
is a container whose length the header covers (`image_payload::container`); os-B is the container
re-composed with the trailer on the kernel item (`with_kernel_trailer`: inside the footprint, in
memory the kernel zeroes, the bootfs untouched); the CLI tests write container fixtures
(`tests/common/payload_fixture.rs`) instead of bare bytes. Numbers: frame census `free=` 38 238 →
43 399 (+5 161 frames), `allocs=` 3 929 → 1 516, milestone 11 990 → 952 ms, execd's image
1 670 → 1 170 shared pages (the rest is the baked atlas, RFC-0080 — follow-up: a bootfs file).

**6c-2 ✅ board cycle 71 (`board--2026-10-26T17-05-01`, dev-64f3fda1).** H1–H4 in the measurement
folder: `nxboot: bootfs (at=0xe00000 …)`, `KSELFTEST: bootfs ok (files=18 …)`, init through the one
loader (65 shared pages, 1 copied), kernel image 0x1239c00 → 0x8f7c00, `excluded=` −117, `allocs=`
8 130 → 6 909, no `segment copied`, milestone 11 2 000 → 1 743 ms, app-host's image the handed-over
bootfs object (`vmo=18`, mappers 1 → 6), FAIL kinds the same six (imed relay send 14× — UX lane),
ladder 79 rungs, acks desktop/pointer/tile/modal/clipboard/gpu-desktop, operator "alle funktional".
The census `free=` (+1 455) was taken with three more spaces open (+23 MB rss) — QEMU's +5 161 at
the same point is H1's number. ADR-0081 Accepted. **6c complete.** Follow-ups: the baked atlas as
a bootfs file (execd's 1 170 shared pages); init reaps no dead child (6d); `WINDOWD: FAIL imed
relay send` (UX lane). NEXT 6d.

**Not in 6c:** CoW (measured unneeded), dropping clean code pages under pressure (6e/6f), a
per-page signature check on fault (the bootfs and the volume are verified whole before use).

## 6d — the cut (2026-10-26, with the operator)

**Measured first (board cycle 71 `board--2026-10-26T17-05-01`, the tree).** init *does* reap:
`SupervisionSweep::sweep` drains `wait_nohang_with_reason` every responder round (8 per round), and
every pinched death is announced and respawned (3×). The 6c-1b note "init reaps no dead child" was
wrong. What outlives a dead pinched is its **threads**: the two workpool workers (pids 74/75,
`sid=0x0`, rss 700 416 — the same space as pid 42) share pinched's address space; when the main
task exits, `exit_current` clears its `children`, so the workers are orphaned (no parent, no
reaper), park on their fence for good and each keep a reference on the space — `destroy` answers
`InUse`, the space survives, and the image object's `mappers=` climbs 1 → 2 → 3 → 4 over the three
respawns (`vmo=108`). The kernel names the gap itself (`task/mod.rs`, TASK-0304: "active teardown
of parked daemon threads when their owning service exits" deferred). Only pinched and the
selftest soak probe spawn threads today.

The kernel has no kill and no suspend of a running task: `TaskState::Suspended` means "spawned,
not yet enqueued", `resume_task` is one way, `exit_current_and_release` (ADR-0073's funnel) acts
on the current task only — a cross-task kill or freeze must reach a target that may be running on
another hart. **Security finding:** `sys_task_resume` resumes *any* pid; the ABI doc promises a
parent check that does not exist.

abilitymgr has the states (Created/Started/Foreground/Background/Suspended/Stopped) and the
transition table, but nobody sends `OP_TRANSITION`: cycle 71 shows only `launch` / `spawn ok` /
`caps ok` / `stop`. `exited()` removes the instance; `transition(Stopped)` only sets a state;
recents list Stopped instances. windowd: `focus id= z=` 31×, `minimize` 7×, no visibility or
occlusion line; focus reaches imed and the taskbar feed only; an app's death closes its window
(`close_app_window`, no ghost); ADR-0075's outbox is in (`Ack`/`Intent`/`Latest`, counts only,
a stalled client unnamed). Permissions: an app's permissions are abilitymgr's build-time table
(`APP_MANIFEST_CAPS` against `KNOWN_PERMISSIONS`); the manifest has no `background` field;
policyd checks *service* caps (`[allow] svc`, `OP_CHECK_CAP`) — the plan's "through policyd"
names an authority that does not hold app permissions today.

**Security view.** Suspend/kill of a *child only* (parent check in the kernel) — a service that
could freeze arbitrary pids freezes the shell; the same check lands on `task_resume`. `killable`
is a kernel-held bit set by the parent at `exec_vmo`, never a hint the task gives about itself:
the emergency kill (6e) must never take a service. A frozen app receives nothing: windowd's sends
are bounded (ADR-0075); every other service's reply path into an app is measured in 6d-2 before
the freeze is switched on. Input on a frozen app is one wake request, nothing is queued.

**The steps (each with a board gate):**

| # | Builds | Gate |
|---|---|---|
| 6d-0 | process death takes its threads (TASK-0304's deferred part): at exit the kernel kills every task sharing the exiting task's space that it spawned — a `Killed` mark + dequeue, a hart running the target exits it at its next trap entry — so the space is destroyed at the reap; `task_resume` checks parentage | `KSELFTEST: process exit takes threads ok (threads= space=freed)`, `test_reject_resume_foreign_task`; board: pinched's respawns leave `mappers=1`, two fewer spaces in the census |
| 6d-1 | process control of a child (RFC-0111 seed, ADR-0082): `task_suspend(pid)` / `task_resume(pid)` / `task_kill(pid)` for a child, the whole process (its threads included); a running target parks or exits at its next trap entry (one deferred mechanism for freeze and kill, bounded by the timer); a blocked target stays blocked and is never woken into a run while frozen; `killable` set by execd at `exec_vmo` for apps, refused for the rest | `KSELFTEST: task freeze ok (pid= resumed=)`, `test_reject_suspend_non_child`, `test_reject_kill_non_child`, `test_reject_kill_unkillable` |
| 6d-2 | the app lifecycle contract (ADR-0083), as built: windowd STATES what is seen per app window (`OP_SEEN` 6: focused / visible / hidden / gone, keyed by the window's kernel service id, NONBLOCK + owed, never parks); abilitymgr DECIDES on one clock (Background while visible or hidden; hidden starts a 5 s grace on a one-shot kernel timer in the loop's waitset; grace out = Suspended; seen again = thawed into the state it is seen in; gone = thawed so it can exit); execd, the parent, ORDERS the kernel (`OP_FREEZE` 4 / `OP_THAW` 5 / `OP_KILL` 6, sender = abilitymgr by kernel id, pid = a live child); Super+Ctrl+M (`CODE_MINIMIZE` 14) hides the focused app / restores the first hidden one. A frozen app's full queue is `Frozen` to windowd's bounded sends (ADR-0082) — no "frozen band" state in the compositor | `windowd: minimize id=app` → `execd: freeze (pid=` → `abilitymgr: suspend (inst=` → `execd: thaw (pid=` → `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)` (visible lane, the injector's chord twice; the first run reached `abilitymgr: bg` and was cut by a 530 s harness budget — rerun on `just ci-os-usb-visible`, 900 s); `SELFTEST: exec child freeze thaw ok` (the probe's call to the frozen child refused — the probe now carries a reply cap, the kernel refuses a call without one as malformed first); host: `cargo test -p abilitymgr seen`, `-p windowd seen`, `-p execd orders`; board: a hidden app frozen behind the focused one |
| 6d-3 | **one permission authority = policyd (ADR-0084)** — supersedes this row's first draft ("the authority stays abilitymgr's table"; two build-time copies of the manifests were two authorities that drift the day one is rebuilt alone): `OP_APP_GRANTS_PUT` 10 / `GET` 11 on the policyd wire, `app.grants.publish` (bundlemgrd) / `app.grants.read` (abilitymgr, execd, the harness) in `base.toml`, by kernel sender id; `nx app compile` writes `permissions=` / `background=` into the sidecar, bundlemgrd publishes every installed app once per boot from the VERIFIED volume before answering the first volume request after its (late-grant) policyd leg is live; an unknown name rejects the app whole and removes its earlier entry; abilitymgr asks per launch (`wire::GrantSource`, no answer = DENIED + marker) and records the answer as the instance's grants; execd asks per spawn and provisions exactly the routable grants (no answer = no routes + marker); `background = [...]` (nxb-pack: ≤ 8 modes of 1..=32 `[a-z0-9-]`, never named in `caps` directly) → `nexus.permission.BACKGROUND` → never frozen for being hidden; both `APP_MANIFEST_CAPS` tables, both build-script readers, `KNOWN_PERMISSIONS` and the `caps ok` boot self-check deleted. The allow side has no consumer (no shipped app declares `background`): contract + host proof only, not claimed board-proven | host: `test_reject_app_grants_from_a_stranger`, `test_reject_app_grants_read_without_cap`, `test_reject_unknown_permission_rejects_the_app`, `test_reject_launch_without_published_grants`, `test_reject_background_undeclared`, `every_sdk_route_permission_is_known` (now policyd's); lanes: `policyd: app grants (app= n=)` per app, `SELFTEST: app grants ok (app=calculator n= background=0)`, `SELFTEST: app grants put deny ok` (headless + visible); board: every launch rides the ask |
| 6d-4 | the windowd restart rung (from 6b-3): windowd dies → the shell comes back with every app's band as a ghost until its next present. **Assessed 2026-10-27, not built in this run** — what it takes (and why it is more than a respawn arm): init's respawn is the pinched pilot (`respawnable()` = pinched; the request endpoint dies with its owner and is re-minted + re-distributed to ONE client); windowd's request endpoint is held by every window client (shell, apps via execd's grant, imed, inputd, screencapd, abilitymgr's sightings, execd), so the respawn must re-pin a fresh SEND into every declared windowd slot of the fleet AND execd must re-grant the app-host children; every push channel windowd minted (event channels, the shell's window feed) dies with it, so each client needs an EOF-driven reconnect (re-route, re-create surface, re-present); gpud's scanout and the retained frame must survive the compositor's death or the screen goes black for the restart's duration. That is a fleet re-route protocol (an RFC seed: "a service's death is a re-resolve for its clients", on ADR-0057's STALE path) plus app-host/shell reconnect work — not honest in the same run as 6d-3. Follow-up: RFC-0112 seeded (Draft, four phases: measure the kill → init re-pins the fleet → clients reconnect on EOF → gpud keeps the frame, ghosts), build on 6d-3's clean tree. **Operator's decision 2026-10-27: scheduled AFTER 6f as 6g** — it serves no package of layer 6 (6e's emergency kill never takes a service) | `windowd: restarted (ghosts=)`; board: kill windowd, the desktop returns — **open, 6g** |

**Status 2026-10-27 (one session, uncommitted):** 6d-0 … 6d-3 built; `just check` green after the
ratchet splits (`nexus-wire/src/policyd_app_grants.rs`, policyd's logd append helpers into
`audit_os.rs`, `nxb-pack/src/background.rs`); headless lane `headless--2026-10-27T10-48-36` green
(the grants rung, `exec child freeze thaw ok`, `task kill ok`); visible lane: the chain
`windowd: minimize id=app0` → `execd: freeze (pid=104)` → `abilitymgr: suspend (inst=3)` →
`execd: thaw (pid=104)` ran on `usb-visible--2026-10-27T10-51-48` — the rung was missed once
because its literal carried `→` (the UART path drops non-ASCII bytes; the marker is ASCII now,
`fg->hidden->frozen->fg`, rerun below). Found on the way: the ladder listed the probe's
`KSELFTEST: task freeze ok` after the exec-elf chain (it fires at execd's start); the proof
manifest lacked `SELFTEST: exec child freeze thaw ok` / `SELFTEST: task kill ok` (evidence
`unknown_marker`). 6d-4 assessed, seeded as RFC-0112, not built. Board cycle 72 (`board--2026-10-27T11-18-29`): the 6d rungs all present, but no typing (rung
`inputd: live keyboard route on` missing) and the desktop shell died of a **stack overflow** after
the operator's no-blur toggle + screenshot attempt (`[USER-PF] STORE … sp=0x1ffee1a0`, 16-page stack,
`abilitymgr: stop (inst=2 app=desktop-shell pid=94 code=-22)`, then 11× `FAIL desktop input send`
— not a 6d code path, the shell's binary unchanged; open follow-up). Cycle 72b
(`board--2026-10-27T11-31-07`, same image): **ladder 86/86**, three apps frozen and thawed
(`execd: freeze (pid=86/87/98)` → `thaw`), `SELFTEST: ability lifecycle ok`, 7/7 grants published,
six launches on the ask, the shell lived through no-blur + two screenshots; acks desktop, typed,
pointer, modal, clipboard, screenshot, gpu-desktop, lifecycle (tile: four quadrant tiles, no chord
line — not acked). README: `docs/board/measurements/2026-10-27-lifecycle-one-authority/`.
**6d-0 … 6d-3 ✅ 2026-10-27 (board cycle 72b); 6d-4 → RFC-0112.**

**Found on the board, moved to layer 8 (the operator, 2026-10-27; the closing row of `tasks/IMPLEMENTATION-ORDER.md`):** the desktop shell's stack overflow in cycle 72 (no-blur toggle + screenshot; `[USER-PF] STORE sp=0x1ffee1a0`, 16-page app-host stack, the shell not relaunched) — build the shell with symbols, walk `.text+0x505e6`, a guard page and a measured stack size for app-hosts, the shell relaunched by abilitymgr on death.

**Not in 6d:** memd and the pressure bands (6e), `restorable` / `@persist` (6e), the kernel's
emergency last resort (6e, on 6d-1's `killable`), purgeable memory (6f). App permissions through
policyd WERE built here (6d-3, ADR-0084) — the first draft parked them; the two build-time copies
made the case.

## 6e — the cut (2026-10-27)

The operator's decision: 6e whole in one session, in five steps, each proven on the lanes before
the next; the `lowmem` QEMU profile (a real level change from a pool run low) is 6f's — here the
path is built and the level is simulated once by the proof harness.

| Step | What, as built | Proof |
|---|---|---|
| 6e-0 | **the kernel's watermarks + one sink (ADR-0085)**: free frames per FDT bank against warn 25 % / critical 10 % / emergency 5 % of the boot-time free total (floors 1024/512/256 frames), hysteresis 1/8 of the band on the way up; the level is pool-wide; one frame `OP_MEMORY_PRESSURE` 0x71 (level, free, total) into ONE bound endpoint per level CHANGE — syscall 67 `mem_pressure_bind` (the first binder holds; `None` clears; a frame to a full queue is dropped, the next change tells the truth); `MmStats` v4 (27 fields: committed/reserved/own_purgeable/purged_pages placeholders for 6e-3) | `KSELFTEST: mem pressure notify ok (`, `KSELFTEST: mem watermarks ok (total= warn= crit=)` — headless ✅ |
| 6e-1 | **a restorable app comes back where it was (ADR-0086, the restore half)**: manifest `restorable = true` → bundlemgrd derives `nexus.permission.STATE` → execd provisions `svc.state` (slot 25) → statefsd `/state/app/@self/persist` (the owner = the sender's kernel id, `test_reject_app_key_of_another_owner`); windowd pushes its sighting to the window (`OP_SURFACE_SEEN` 35); app-host writes the runtime's `@persist` snapshot at `hidden` and at the window's close (bounded 2048 bytes, never per change, never on a clock) and restores it at mount before the root effects; Super+Ctrl+W (`CODE_CLOSE` 15) closes the focused app window | `apphost: persist saved (fields= bytes= reason=hidden)` → `reason=close` → relaunch → `SELFTEST: app restore ok (fields=3 bytes=87 warm_us=15236)` — `usb-visible--2026-10-27T15-05-59` ✅ |
| 6e-2 | **memd, the one memory authority (ADR-0086, the memd half)**: `source/services/memd` (`ServiceId::Memd`, platform stage, system volume) binds the sink to its declared pair (`NamedSlot::MemPressure{Recv,Send}`) and waits on it + its server endpoint; abilitymgr pushes the BANDS as a diff after every broker change (`nexus_wire::memd::OP_BAND`: foreground 0 > visible 1 > declared 2 > hidden 3 > suspended 4, `gone` at the death report; NONBLOCK, committed when taken), taken from abilitymgr's sender id alone; kill floor `hidden` at critical, `visible` at emergency, the victim = the highest band, the longest there first, one kill in flight; the order `OP_MEM_KILL {pid}` to abilitymgr (from memd's sender id alone; foreground/starting refused) → execd `OP_KILL`; the kill is proven by the victim's `gone` band, a kernel-sourced one then re-reads the level; the harness probes `STATUS` on every lane and fires `OP_DRILL {critical}` once on a QEMU lane with a display (its sender id alone, never on a board) after an app was frozen again | host: `test_reject_kill_of_foreground_or_visible`, `test_reject_band_from_anyone_but_the_lifecycle_authority`, `test_reject_drill_from_anyone_but_the_harness`, `test_reject_mem_kill_from_anyone_but_memd`, `test_reject_mem_kill_of_foreground`; lanes: `memd: ready` → `memd: pressure (level=normal free= total=)` → `SELFTEST: memd bound ok (level= bands=)` (every lane); `memd: band (pid=` → `SELFTEST: mem pressure drill (` → `memd: kill (pid= band= reason=drill)` → `abilitymgr: kill (inst= reason=memory)` → `execd: kill (pid=` → `SELFTEST: mem pressure kill ok (band=suspended reason=drill)` (usb-visible) — headless `headless--2026-10-27T15-45-18` ✅, usb-visible `usb-visible--2026-10-27T16-04-50` ✅ |
| 6e-3 | **the level told onward + purgeable VMOs (RFC-0100 §4, ADR-0086)**: kernel syscall 68 `vmo_purgeable (slot, unlock)` by a MAP holder — unlock refused for an eager, device-named or mapped object (`vmo_lifetime::purge_unlock_allowed`); unlocked, the object refuses `vm_map`/`vmo_write`/`vmo_read`/`vmo_runs`; the purge frees every unlocked object's frames when the level about to be told reaches warn, before the frame leaves (`MM: pressure purge level= pages=`); lock answers 1 if purged; `MmStats.own_purgeable_bytes` (unlocked, named by the caller's caps) and `purged_pages` filled. memd pushes `OP_LEVEL {level, source, free, total}` NONBLOCK to gpud and windowd on two new declared legs (every kernel frame and every drill; `memd: level told (…)`); gpud names it from memd's sender id alone (`gpud: mem pressure (level= source=)`; the drop is 6e-4's, with the ledger); windowd tells every live app window `OP_SURFACE_PRESSURE` 36 on its event channel (`windowd: mem pressure (level= source= windows=)`); app-host raises the host event `MemoryPressure` (`Normal`/`Warn`/`Critical`/`Emergency`; `apphost: mem pressure (level= handled=)`); the harness drill is two levels a second apart: `warn` (the fan-out) then `critical` (the kill) | host: `test_reject_purge_of_bound_vmo`, `test_reject_purge_of_mapped_vmo`, `test_reject_purge_of_eager_vmo`, `a_purge_returns_the_committed_frames_and_uncommits_every_page`, `test_reject_level_from_anyone_but_memd` (gpud), `level_push_round_trips_and_is_the_longest_request`; every lane: `KSELFTEST: vmo purge ok (pages=4 purged=4 relocked=purged reject=bound,eager,mapped)`; usb-visible: `SELFTEST: mem pressure drill (level=warn …)` → `SELFTEST: mem pressure fanout ok (level=warn sent=2)` + `gpud: mem pressure (level=warn` + `windowd: mem pressure (level=warn` + `apphost: mem pressure (level=warn` — ✅ headless `headless--2026-10-27T16-52-23`, usb-visible `usb-visible--2026-10-27T17-00-20` (the whole chain at warn and at critical; see `docs/board/measurements/2026-10-27-memory-authority/README.md`) |
| 6e-4 | **GPU memory charged to its client (RFC-0100 §3)**: gpud's `charges.rs` — per client (kernel service id) two parts, the framebuffer object it lent (`Part::Lent`, its whole span, charged at the grant) and the client's context buffers (`Part::Context`, `GpuService::held_by`: the board sums its BOs by client, QEMU's display holds none), eight clients, a ninth refused by name; each changed total told to memd on a new declared leg (`slots::gpud::MEMD` 0x2b, `REQUIRED_ROUTES` (Gpud, Memd)) as `OP_GPU_CHARGE {client, bytes}` — absolute, replaces, zero frees, NONBLOCK; memd `gpu.rs` is the authority's copy, taken from gpud's sender id alone | host: `test_reject_gpu_charge_from_anyone_but_gpud`, `test_reject_a_ninth_client`, `a_charge_replaces_and_the_parts_sum`, `zero_in_every_part_frees_the_row…`, `gpu_charge_push_round_trips…`; every display lane + board: `gpud: gpu memory charged (client=windowd bytes= told=1)` → `memd: gpu charge (client=windowd bytes= total=)` — ✅ headless `16-52-23` + usb-visible `17-00-20` (the lent framebuffer object, 107 827 200 bytes, charged and told); the context part is the board's (cycle 73) |

**Status 2026-10-27 (one session, uncommitted):** 6e-0 … 6e-4 all lane-proven — headless
`headless--2026-10-27T16-52-23` and usb-visible `usb-visible--2026-10-27T17-00-20`, both exit 0,
FAIL kinds a subset of the 6e-2 run's with equal or lower counts (the README under
`docs/board/measurements/2026-10-27-memory-authority/`). Host green (neuron, wire, topology,
display-proto, memd, gpud, windowd, app-host, init, proof-manifest), OS-cfg clean under
`-D warnings`, the ladders and the proof manifest carry the new rungs (`KSELFTEST: vmo purge ok (`
on every ladder incl. board-test's headless one; the fan-out and GPU-charge rungs on the visible
lanes and board-test's visible ladder). The structure gate's ratchet split four files on the way
(`nexus-abi/syscall/pressure.rs` — the levels, the frame, the purgeable lock/unlock, re-exported
from `memory`; `nexus-service-topology/slots/memd.rs`; windowd `compositor/pushes.rs` — the
MAGIC-discriminated fire-and-forget pushes imed/settingsd/memd in one place; app-host
`probe/slots.rs` — the child's slots and budgets) and retired a dead `#[allow(dead_code)]` const
in windowd's runtime and a stray `#[test]` on a bundlemgrd fixture. Next: board cycle 73, one
commit.

**Found by the first 6e-3 visible lane (`usb-visible--2026-10-27T16-32-13`, fixed):** memd's
declared leg to gpud was never pinned — init's endpoint table did not name gpud's server pair
("priority-wired", so `request_ep(memd → gpud)` was `None` and the leg was silently skipped) —
and the kernel put the harness's moved reply capability into that lowest free slot: memd "told
gpud" the level by writing into the harness's inbox, every later probe read a shifted answer
(`status=malformed`, then `ime v2 cjk jp` missing). Fixes: gpud's pair in init's table; a
`ReplyInbox` leg without a target endpoint is now `init: FAIL declared leg a->b (no target
endpoint)` by name, never a silent skip (the core plane's two-pair table excepted); the drill is
two arming conditions — `warn` once a thawed app is on screen (a frozen window cannot hear the
level), `critical` once an app is frozen again — and its marker one atomic line.

**Found by board cycle 73 (2026-10-27, `board--2026-10-27T20-36-32`, open — not 6e code):** the
board's keyboard path loses letters. The arrow chords reach windowd (`wm chord (zone=left-half`,
`zone=return`), the letter chords Super+Ctrl+M / Super+Ctrl+W never do (no `wm chord`, no `wm tile
deny` — the fact never leaves inputd), and typing has been red on the board since cycle 64
(`inputd: imed send FAIL` / `WINDOWD: FAIL imed relay send`, 25 lines this cycle, 31 in 72b; the
QEMU usb-visible lane types and closes with the same chord table, `windowd: wm chord
(zone=close`). The host rig (the boot parser + inputd, `de` keymap) parks M and W as codes 14 and
15 — the test extended. Operator 2026-10-27: 6e closes now, 6f next, the keyboard AFTER 6f —
measure the board's keyboard path from the HID report to imed (what the receiver sends under
Super+Ctrl, where the imed send fails), then fix; the operator asked for a Settings page that lists
every keyboard shortcut (tiling, minimize, close, screenshot) — the same package.

**Not in 6e-3:** what gpud DROPS at warn (6e-4: on QEMU its glass textures are the host GPU's
memory, on the board a GPU-private buffer of the one pool — purgeable once the ledger says what it
costs whom); the first purgeable owners (atlas bands, blur cache — follow-up after 6e-4); a
compression or swap stage behind the purge (6f, §5).

**Not in 6e-2:** memd's kill journal (6f), the `lowmem` profile (`reason=oom`, 6f), the
compositor's ghost for a killed window (6g), a process the lifecycle authority never named
(window-only launches, services) — invisible to memd by design (the kernel's emergency last
resort stays the kernel's).

## 6f — the cut (2026-10-28)

The operator's decision (2026-10-27): 6e closes, 6f next, the board's keyboard path after 6f, 6g
(RFC-0112) after that. The RFC's open question R11 — the compressor's ratio and cost per page on
RV64 — is measured before anything is built on it: 6f-0 is the codec and its measurement alone.

Three lenses: **Scope** — in: one new leaf crate `source/libs/nexus-compress` (`no_std`, zero
deps, `forbid(unsafe_code)`), a kernel selftest + its rung on every ladder, later `mm`'s pool,
the squeeze order, memd as the pager, the `swap` partition, memd's journal, the `lowmem` profile;
out: compressing clean file pages (dropped, never written), policy in the kernel (memd orders,
the kernel executes), swapping uncompressed pages, the keyboard (after 6f). **Invariant** — the
decoder is bounded: every length and offset is checked against the blob and the page, a
malformed blob is a typed error, never a read past the blob or a write past the page
(`test_reject_*` in the crate); a squeeze is the parent's order and only on a frozen child.
**Contract** — RFC-0100 §5 (filled per step), ADR-0077's fault path extended (6f-1), the new
syscall and the page-in protocol seeded as RFC/ADR in 6f-1/6f-2.

| Step | What, as built | Proof |
|---|---|---|
| 6f-0 | **the page compressor, measured where it will run (RFC-0100 §5)**: `nexus-compress` — the LZ4 block format (token, literal run, 16-bit offset, match ≥ 4; last sequence literals only), 2048-head hash table owned by the caller (4 KiB, cleared per call: a page's blob never depends on history), LZ4's skip in incompressible stretches, 8-byte match extension, a doubling copy in the decoder for overlapping matches (a run of one byte included); `compress` refuses an input beyond 16-bit positions and answers `OutputFull` when the blob would not fit (a page-sized output: "keep the page as it is"); `decompress` checks every length and offset — truncated, zero offset, offset beyond the output, output over the page, a blob ending after a match — and has no length header: the caller compares the returned length with the page. Kernel: `selftest/pressure.rs::run_compress_selftest` samples the kernel image at one stride (1024 candidates, zero pages counted and skipped, up to 64 content pages), copies each page once (the live stack is in the range), compresses, inflates, compares, and reports ratio, `smaller=`, and the cost per page both ways on the hart's clock | host: 13 tests — round trips over zeros, text, noise, overlapping repeats, long runs, every length 0…300 and the 64 KiB maximum, determinism, `test_reject_input_too_long`, `test_reject_output_full_on_an_incompressible_page`, `test_reject_truncated_blob`, `test_reject_offset_beyond_output` (+ zero offset), `test_reject_output_over_the_page` (match and literal run), `test_reject_blob_ending_after_a_match`, `test_reject_literal_run_over_the_blob`; every lane + board: `KSELFTEST: mem compress ok (pages= zero= ratio=% smaller= us_per_page= inflate_us_per_page= sample=image)`; the numbers in `docs/board/measurements/2026-10-28-backing-stages/README.md` |
| 6f-1 | **the squeeze — a frozen app's private memory recorded, every page back on touch (RFC-0100 §5, ADR-0087)**: syscall 69 `task_squeeze(pid, from_va, out, len)` — the parent alone (`ESRCH`), every task of the child parked (`EBUSY`), one bounded batch a call (≤ 16 committed pages examined, ≤ 4096 page words scanned), the next address answered (`0`: done) and a `SqueezeBatch` v1 record written through the caller's own page table; the objects of the child's alone (`vmo_lifetime::squeeze_allowed`: lazy, not sealed, never told to a device, not unlocked, mapped by one region of the child's space and nowhere else, named by no capability outside the process); per page zeroes dropped / a block ≤ 3 582 B recorded / the rest kept; a lazy page's word absent / frame / tagged record (`lazy_pages.rs`), the records in a store per object (`lazy_store.rs`: 64 B slots, one occupancy and one start word per store page, first-fit, an emptied store page back at once, the object's death or purge all of them); `vmo::commit` inflates (the fault, the checked range, `vmo_read`/`vmo_write`, a device's runs — the codec outside the table lock, a record that does not decode to one page fails the access); one shootdown per batch before any frame goes back; `mm_stats` v5 (`squeezed_pages`, `store_bytes`, `store_frames`, `inflated_pages`). The order: memd at `critical` squeezes the longest-suspended app not squeezed since its band changed before any kill (`emergency` kills at once) → abilitymgr `OP_MEM_SQUEEZE` (8, a suspended instance alone) → execd `OP_SQUEEZE` (7, `nexus_wire::execd`) → the batches → the totals back up; a kernel-sourced squeeze reads the level again (`settle`) | host: the store (9 tests incl. 4 `test_reject_*`), the page table (11 incl. 4), the rule (5 `test_reject_squeeze_*`), the ABI record (2), the wire frames (2), abilitymgr's relay (4), memd's policy (5 new, 2 re-cut); every lane + board: `KSELFTEST: task squeeze ok (pages=4 zero=1 kept=1 … reject=stranger,running,shared,device)`, `SELFTEST: exec child squeeze ok (`; usb-visible: `execd: squeeze (`, `abilitymgr: squeeze (`, `memd: squeeze (`, `SELFTEST: mem squeeze ok (` |
| 6f-2 | **the swap stage — a frozen app's records sealed out to the pager's disk, a touch parks until the page is back (RFC-0100 §5, ADR-0088)**: the kernel seals every record that leaves it — ChaCha20-Poly1305 under the boot's key (`lazy_seal.rs`: the nonce the kernel-wide counter, the associated data the page's identity), the key seeded once by the RNG service (syscall 70, its kernel identity alone) when the pager first needs it (`OP_SEED_SWAP_KEY`, rngd op 2, memd's identity alone: rngd's entropy device is granted after it starts); a lazy page's fourth state SWAPPED (`lazy_pages.rs`) and the swap map (`lazy_swap.rs`: exported under a counter, committed to a slot; a touch cancels an export, a later export supersedes it); syscalls 71 `swap_export(pid, from_va, buf, len)` (the sink's holder alone, never itself; ≤ 16 records and 4096 page words a call; entries `token, ctr, len, sealed`), 72 `swap_commit(token, ctr, slot)` (`ESRCH` unless the page is still that export's record), 73 `swap_page_in(token, slot, sealed)` (opened under the page's counter and identity, inflated, the waiters woken; stale → `ESRCH`, the waiters woken to retry; forged → `EINVAL`, never entered, the waiters killed with cause `0xF2`; no frame → `ENOMEM`, the request stands); a fault on a swapped page parks its thread (`BlockReason::PageIn`, the instruction re-executes), a syscall parks its caller and re-executes, an IPC call already sent makes its reply buffer writable again before the reply leaves its queue; the requests and drops (`OP_PAGE_IN` 0x72 one per page, `OP_SWAP_DROP` 0x73 32 slots a frame) queued on the pressure sink until it takes them; `mm_stats` v6 (`swapped_pages`, `page_ins`, `parked_faults`, `swap_key`). The disk: `swap` 64 MiB after `trace` (16 384 slots; the smallest disk 448 MiB), block selector 7 served to memd alone (blkd's gate), never flashed (`layout::VOLATILE`). memd the pager (`swap.rs` the books, `pager.rs`): at `critical` after the squeeze the longest-suspended squeezed app's records out — export → write the slot → commit, the partition opened once — a request served at once, a thaw pages the app back before it asks, a drop frees the slots; without a key the policy goes from the squeeze to the kill. The plan's "blobs into memd's object" became the kernel's seal: memd holds ciphertext alone | host: the seal (4 tests incl. 3 `test_reject_*`), the swap map and its buffer (7 incl. 4), the page table's swap steps (4 incl. 2), the notices (2 incl. 1), the ABI's buffer and notices (3), the layout, the selector and blkd's gate (memd on `swap` alone, nothing else), memd's books and policy (8 incl. 4); every lane + board: `KSELFTEST: swap ok (pages=3 sealed=312 parked=1 back=1 killed=1 dropped=2 us= reject=key,sink,nokey,stale,tampered,replayed)`; usb-visible: `rngd: swap key seeded`, `memd: swap on (slots=16384)`, `memd: swap out (`, `SELFTEST: swap out ok (`, `memd: swap in (`, `SELFTEST: swap in ok (`, `memd: swap drop (` |
| 6f-3 | **memd's journal + the `lowmem` lane — a real level kills, the kill is recorded, the app comes back (RFC-0100 §5, ADR-0086 amended)**: memd's route to statefsd (`REQUIRED_ROUTES`, slot 13 on the CAP_MOVE inbox; policy `statefs.read` + `statefs.write`); the kill ledger `journal.rs` — `/state/memd/journal`, `NXMJ` v1 (12-byte header: magic, version, count ≤ 16, next sequence; 24-byte records: sequence across boots, pid, band, reason `oom`/`drill`, level, free/total frames, uptime ms), a ring of the newest sixteen, decoded only when it is exactly this layout; the boot's `Ledger` over a `Store` (read at the first kill, every kill appended, the ring written and read back; a kill proven while the store cannot be read waits in memory, a refused write goes with the next kill, a foreign record replaced and said once); written at `KillProven` through statefsd's client (`StateStore` in the OS loop). The `lowmem` lane (`[profile.lowmem]` extends usb-visible, `just ci-os-lowmem`, in test-all; the guest reads `RuntimeProfile::Lowmem`, the full phase scope): the lane's declared scenario (`proof-manifest/scenarios/lowmem.toml`, run by `tools/qmp_scenario.py`) logs in, launches settings, hides it; the harness (`os_lite/lowmem.rs`) takes eager objects once an app is frozen until free frames are halfway between the critical and the emergency watermark (`mm_stats` names both), takes back what the squeeze and the swap-out give, and lets everything go at memd's next kill; the scenario relaunches settings. The plan's `QEMU_MEM=128M` became the default machine with the harness as the memory-hungry workload: the level is the kernel's own, and the desktop still boots. The drill's line is forbidden on the profile (verify-uart). Found on the way: abilitymgr thawed a killed app whose window closed before the death report — an instance whose kill was carried is now `dying` (only the death report moves it, no freeze grace); abilitymgr's and execd's byte-at-a-time console lines could be torn by another process — one builder writes them whole (`nexus_abi::DebugLine`) | host: the journal (10 tests incl. 3 `test_reject_*`: a foreign, a truncated or overfull record, a read-back that differs, a foreign record replaced), abilitymgr (2 `test_reject_*`: the thaw of a dying instance, the freeze of one killed in its grace), `DebugLine` (2 incl. `test_reject_a_line_past_the_cap`), execd's table (3 incl. `test_reject_evicting_a_live_child`), the runtime profile token; `lowmem`: `SELFTEST: mem pressure held (level=critical`, `memd: squeeze (… reason=oom)`, `SELFTEST: mem squeeze ok (`, `SELFTEST: swap out ok (`, `SELFTEST: mem pressure kill ok (band=suspended reason=oom)`, `SELFTEST: memd journal ok (reason=oom`, `gpud`/`windowd: mem pressure (level=critical source=oom`, `SELFTEST: mem pressure released (level=normal`, `SELFTEST: app restore ok (`; usb-visible: `SELFTEST: memd journal ok (reason=drill` |

**Status 2026-10-28:** 6f-0 ✅ — host green (13 codec tests), OS-cfg clean under `-D warnings`,
check/dep-gate/diag/test-host green, the rung on both QEMU ladders and board-test's ladder,
declared in the proof manifest; headless lane green (`headless--2026-10-28T09-19-50`); the
usb-visible lane red at the keyboard rungs for the pre-existing input defect (below — reproduced
on the unchanged HEAD); board cycle 74 (`board--2026-10-28T10-02-54`): `KSELFTEST: mem compress
ok (pages=29 zero=227 ratio=47% smaller=29 us_per_page=49 inflate_us_per_page=16)`, H1–H5 ✅,
90/96 rungs — rung 91 `inputd: live keyboard route on` missing (the board's keyboard dead whole
this cycle, see the README). The numbers decide 6f-1: a frozen process is squeezed WHOLE on
memd's order, a fault inflates synchronously (16 µs). Next: one commit, then 6f-1.

**Found by the 6f-0 visible lanes (2026-10-28) — two things, one fixed, one filed:**

1. *The pointer injector raced the OSK probes (fixed in the harness).* `SELFTEST: ime v2 cjk jp
FAIL` in `usb-visible--2026-10-28T09-08-03`, `09-13-45` and on the unchanged HEAD
(`09-35-54`, the control run) — green in every run of 2026-10-27. In every green run the
injector's first pointer event (`windowd: input visible on`) came AFTER the OSK composition
probes; in every red one before or during them (so too in `usb-visible--2026-10-27T16-32-13`,
whose `cjk jp` miss 6e-3 attributed to the slot hole). The first pointer event makes windowd tell
imed which WINDOW holds focus (`sync_surface_focus_to_imed`, `FIELD_KIND_NONE`), imed resets its
composition on that focus transition (by design — a half-typed accent never leaks across fields),
and the probe's half-typed `nn` is gone. The probes are guest-sequenced, the injector host-timed
(`inputd: ready` + 100 ms): the order depended on the emulator's speed that day. Fix at the root:
`tools/qmp_visible_input_inject.py` waits for the guest's last OSK probe line (`SELFTEST: ime
insert `, ok or FAIL, bounded 20 s, logged when missed) before its first event — `cjk jp ok` in
both runs since (`09-28-07`, `09-43-47`).

2. *The board's lost letter chords reproduce on QEMU (filed for the keyboard package, after 6f —
the operator's order of 2026-10-27).* With the probes ordered, the lane goes red one rung later,
and so does the unchanged HEAD: the injector's Super+Ctrl+M after `SELFTEST: ui v7 tile ok`
yields no `windowd: wm chord (zone=minimize` (`09-43-47` with 6f-0, `09-35-54` on HEAD) and in
`08-56-08` the Super+Ctrl+W after the thaw no `zone=close` — the arrow chords of the same run
(`left-half`, `return`) always arrive. The letter chord lands while windowd is re-creating the
returned window's surface (`WINDOWD: surface destroyed/created`, `ux input FAIL
(to_submit_us=155741)`); the one-shot fact survives a newer sample in windowd's input stage by
design (`input_stage.rs`), so the loss is upstream of it — inputd's letter path (keys to imed)
against its chord table, or hidrawd's report (`wire skip raw=1 norm=0` is the modifier-only
report). It is the board's defect of cycle 73 (letters never reach the chord fact, arrows do),
now with a lane that shows it. Once the chord did arrive (`09-28-07`) the hidden app was never
frozen: `abilitymgr: bg` → `memd: band hidden` → `apphost: persist saved` and then no `execd:
freeze` in 150 s, the grace clock (`NotifyTimer::arm_at`, kernel `timer_set` → `arm_wakeup`)
never waking it — one occurrence, the same package. The lane's one-hart emulator ran slower
today (`KINIT: milestone 11` 975–998 ms against 952–959 ms; the 6f-0 selftest is ≈ 6 ms of it),
which is what turned these latent orders into misses. The 6f-0 rung itself is green in every
run (six visible, four headless): `KSELFTEST: mem compress ok` with the same numbers each time.

**Corrected 2026-10-28 (6f-1): point 2 was wrong.** Every one of these usb-visible runs was started
with a plain `just test-os usb-visible`, which inherits the virgl default budget of 240 s; QEMU was
SIGTERMed by it (`qemu.stderr`: `terminating on signal 15 … (timeout)`) — `09-43-47` 15 s after the
tile phase (before the Super+Ctrl+M was even sent), `09-28-07` during the 5 s grace after the
minimize, `08-56-08` 0.8 s after the lifecycle phase. The lane's recipe is `just ci-os-usb-visible`
(900 s, the justfile says why); 6e's green runs used it. None of these runs shows a lost chord or a
missing freeze on QEMU: the board's keyboard defect (cycles 73/74, the operator's finding) is real
and stays the keyboard package's, but QEMU does not reproduce it so far. Point 1 (the injector's
race with the OSK probes) stands — its FAIL markers are the guest's, not the budget's.

**Status 2026-10-28 (6f-1, lanes):** built and lane-proven — host green (the kernel's 191 tests,
the workspace's), `check`/`diag`/`dep-gate`/`test-host` and the kernel's clippy green; headless
(`headless--2026-10-28T12-07-42`) and usb-visible (`usb-visible--2026-10-28T12-38-24`, through
`just ci-os-usb-visible`) green end to end. The kernel selftest: `KSELFTEST: task squeeze ok
(pages=4 zero=1 kept=1 store=1 stored=386 inflated=4 reject=stranger,running,shared,device)` on
both. The probe child: `SELFTEST: exec child squeeze ok (pages=11 zero=1 kept=2 taken=2 stored=1060
batches=1 us=1283)`, its pattern whole after the thaw. The drill on the settings app at its first
freeze: `execd: squeeze (pid=110 pages=450 zero=0 kept=0 taken=117 stored=463155 batches=29
us=76868)` → `SELFTEST: mem squeeze ok (pages=450 net=333 ratio=25% reason=drill)` — 1.8 MB of
heap and stack into 468 KB, 1.3 MB back to the pool, then the thaw (`ability lifecycle ok`, the
queued `critical` and the next `warn` heard by the thawed window), the restore, a second freeze
squeezed (448 pages) and then killed (`mem pressure kill ok`), the clipboard and the capture
phases after it. Boot cost of the selftest on TCG: `KINIT: milestone 11` 962 → 967 ms. **6f-1 ✅
2026-10-28 (board cycle 75, `board--2026-10-28T13-00-46`, dev-6c04a43c):** the ladder 98/98 with
`KSELFTEST: task squeeze ok (…)` (the same counts as on the lanes) and `SELFTEST: exec child
squeeze ok (pages=11 zero=1 kept=2 taken=2 stored=1060 batches=1 us=632)` — 45 µs a page on the
board's core, `bkl budget ok (max_hold=0ms)`; the FAIL kinds of SELFTEST/KSELFTEST unchanged from
cycle 74; nine acks with their act lines (restore not done this boot: settings was frozen and
thawed, not closed and reopened). The keyboard was alive this cycle (the chords arrived; the old
`imed send FAIL` of typing is back with it — the keyboard package's). Boot: milestone 11 2042 ms
against cycle 74's first boot 1814 ms — 114 ms of it before the kernel (the loader), the selftest
window 611 → 726 ms; the new selftest's share is not isolated (5 ms on QEMU) and is measured first
in 6f-2. Next: one commit, then 6f-2.

**Found by the 6f-1 lanes (2026-10-28):**

1. *The record write of `task_squeeze` faulted the kernel in its own selftest* (`KPGF … stval=0x1003`,
the first headless run): the syscall wrote its record through the caller's user address, which
needs the caller's address space active — the boot selftest drives the syscall with the kernel's.
Fixed at the root: the record goes through the caller's own page table and the direct map
(`write_record`), so the write is the same wherever the syscall is driven from.
2. *The usb-visible lane must run through its recipe* — see the correction above: `just
ci-os-usb-visible` (900 s); a plain `just test-os usb-visible` is cut at 240 s.
3. *A lane cut by its budget killed the NEXT lane's emulator.* `usb-visible--2026-10-28T12-22-05`
died at 358 s (`signal 15 from pid … ()`, no "(timeout)"): the 12-12-02 run's UART monitor had
outlived its QEMU (the launcher's grace loop read `read -t`'s EOF as a timeout) and, when its 840 s
grace ran out, `pkill`ed every emulator on the host. Fixed in `scripts/qemu-launcher.sh`: the loop
returns on EOF, without the kill.
4. *The kernel's clippy gate (`just lint-kernel`, run by CI) was red on the unchanged HEAD* — five
findings in 6d/6e selftest lines (`nonminimal_bool`, `needless_borrow`); fixed in passing (no line
counts changed), the gate green again.

**Status 2026-10-28 (6f-2, lanes):** built and lane-proven — host green (the kernel's 208 tests,
the workspace's), `check`/`diag`/`dep-gate`/`lint-kernel` green, the kernel's crypto (the
workspace's audited AEAD crate, default features off) without a forbidden crate in its graph;
headless (`headless--2026-10-28T18-09-05`) and usb-visible (`usb-visible--2026-10-28T18-12-21`)
green end to end. The kernel selftest on both: `KSELFTEST: swap ok (pages=3 sealed=312 parked=1
back=1 killed=1 dropped=2 us=1602 …)` — the forged answer's one kill named as `MM: page-in refused
(vmo= page= killed=1)`. The chain on the settings app at its first freeze: squeezed (450 pages →
468 KB), then at the next drill `rngd: swap key seeded`, `memd: swap on (slots=16384)`, `memd: swap
out (pid=117 … pages=450 slots=450 reason=drill)` → `SELFTEST: swap out ok (pages=450 slots=450
reason=drill)`; the thaw: the app touched one page before memd heard its band (served by request)
and memd paged the other 449 back (`SELFTEST: swap in ok (pages=449)`); `ability lifecycle ok`,
the restore and the clipboard and capture phases ran on the swapped-and-returned process. The
second freeze: squeezed (448), swapped out (448), killed — its objects' death told the pager all
448 slots. The squeeze selftest's share of the boot, measured as cycle 75 asked: `us=2946` on TCG
(the swap selftest 1602 µs). **6f-2 ✅ 2026-10-28 (board cycle 76, `board--2026-10-28T21-38-30`,
dev-c71683c0, two boots):** the ladder 99/99 with `KSELFTEST: swap ok (pages=3 sealed=312
parked=1 back=1 killed=1 dropped=2 us=15013 …)` and `blkd: gpt ok (parts=8)` on the board's harts
(the forged answer's one kill named once), `init: rng plane none` — the board has no entropy
device, so no key and nothing swapped, as designed; the FAIL kinds identical to cycle 75; the
whole test-all chain green (the usb-visible lane once more after a pointer miss of the injector in
the clipboard phase, a phase 6f-2 does not touch). The selftests' share of the boot: squeeze 9.1
ms, swap 15.0 ms on the board — the first boot's selftest window 660 ms against cycle 75's 726
(milestone 11 1861 against 2042 ms; the second boot of the same cycle 754 ms): the window moves
by ±50 ms between boots of one image, the two selftests are not what grew it. Seven acks with
their act lines (desktop, typed, pointer, tile, screenshot, gpu-desktop, lifecycle); modal,
clipboard and restore have none in this boot's trace (no text field focused, no close of
settings) and stay open.

**Found by board cycle 76 (2026-10-28): execd forgets a live app.** execd's child table holds 16
entries and evicted the OLDEST, alive or not (`track_child`): the operator opened four apps before
the harness's 15 probe children ran, settings (pid 92) was pushed out while it ran, and every
lifecycle order for it was refused by name — `execd: order denied (sender or not a live child;
pid=92)` → `abilitymgr: FAIL freeze (inst=5)` / `FAIL thaw` (the same refusal would meet a
squeeze, a swap-out's prerequisite freeze and memd's kill). Latent since 6d; cycle 75's settings
started after the probes. **Fixed 2026-10-29 (6f-3's run):** the table holds 32 rows and the
bound drops the oldest EXITED row alone, never a live child (`execd/src/children.rs::evict_index`;
host: `test_reject_evicting_a_live_child` and two more); the reap pass is bounded by twice the
table.

**Found by the 6f-3 work (2026-10-29): abilitymgr thawed a killed app.** On the `lowmem` lane the
killed app's window closed before execd's death report reached abilitymgr; the sighting `gone`
of a frozen instance means "thaw it so it can exit" (6d-2), so abilitymgr ordered a thaw of a
process the parent had already reaped (`execd: order denied (… not a live child; pid=117)` →
`abilitymgr: FAIL thaw (inst=3)`) and pushed a `visible` band for the dead pid to memd. The drill's
kill on the usb-visible lane never showed it (the death report came first there). An instance
whose kill abilitymgr carried is now `dying`: sightings and the freeze grace no longer move it,
the death report ends it (`seen.rs`, two `test_reject_*`); the rerun has neither line.

**6f-3's determinism package (2026-10-29, ADR-0089 — the operator's decision: "alles jetzt in
6f-3").** The first test-all runs were red for three reasons, none of them 6f-3's change, and the
operator asked for the class, not another patch:

1. *A marker torn by another process* (`abilitymgr: ` + execd's exit line, then `kill (inst=3 …)`):
   the kernel gathered console bytes per HART, and a task switched out mid-line had the next
   task's line appended to its half. → **Whole lines per TASK** in the kernel (`TaskLines` before
   the hart funnel: 16 half lines, pieces at 256 bytes, write-through when full, the half line
   ended at the task's death; host: 5 tests incl. 2 `test_reject_*`, the lowmem tear replayed);
   `nexus_abi::DebugLine` writes a line in one syscall (abilitymgr and execd use it).
2. *The injector raced the guest*: a pointer burst inside the IPC benchmarks (`ipc call budget
   FAIL (rt=90us)`), a press lost in the busy second after a kill (its retry pressed the clock),
   three early-phase steps that failed in EVERY USB run and were skipped, idle waits that hid
   orderings. → **The guest opens the input window** (`SELFTEST: input window open`, after its
   timing gates and end-phase probes) and **the choreography is data**: `proof-manifest/scenarios/`
   (`usb-visible`, `lowmem`, `input-flood`; the points and the typed word in `targets.toml`, read by
   the shell's conformance tests too), validated by `nexus-proof-manifest scenario` (every literal a
   declared marker — seven sync lines newly declared —, every target named, every action
   acknowledged or `acked_by` a later step, a `then` only after an `expect`; 10 tests incl. 8
   `test_reject_*`), run by ONE runner (`tools/qmp_scenario.py` over `tools/qmp_input.py`) that
   ends at the first unacknowledged step with its name, homes before retrying a lost press and
   logs each step's duration. A step's `expect` is a SET of new lines after the action (services
   write concurrently — an ordered list failed a green close: windowd's `app window closed` before
   app-host's `persist saved`), its `then` each after the last of them (a re-created surface's
   first frame after its resize); a `wait` step counts from the action of the step before it (it
   has none of its own); a click counts only on a HIT read back on its point (a miss there — the
   shell not yet hit-ready after login — waits and presses again). The imperative
   phases (`qmp_inject_modal.py`'s, `qmp_inject_lowmem.py`) and the dead-reckoned early phase are
   deleted. Found by the first run of the declared lowmem scenario: a chord as the keyboard's FIRST
   input got one empty report from QEMU's USB keyboard and nothing after it — the old early phase
   had masked that by pressing `a` first; the first key is now a step of its own (a letter, retried
   until `inputd: live keyboard route on`), the cause open for the keyboard package.
3. *A gate measured the hart's load*: the IPC budget judged the MEAN of 64 calls. → It judges the
   **fastest call** (each timed alone, the clock's cost taken off), the mean printed beside it.
4. *The host's disk stalled the guest*: a `usb-visible` run of the new scenario lost the settings
   launch to `virtio-blk: timeout` ×4 → `blkd: trace os FAIL (write)` → `bundlemgrd: FAIL
   get_payload` — the 2026-10-27 signature, this time without any host build running: under
   QEMU's default cache every guest flush was a host `fdatasync` of the whole image on a disk at
   97 %. → A proof session attaches its per-run image with **`cache=unsafe`** (the flush a no-op on
   the host, writes still ordered in its page cache; interactive sessions unchanged).
5. *The product race* (the window's `gone` before the death report thawed a reaped app) → the
   `dying` state above, **proven over every order**: `memd/tests/kill_chain.rs` drives memd's
   authority, abilitymgr's broker, band feed and kill policy through all 128 interleavings of the
   window's `gone`, the death report and up to two stray sightings — the kill proven exactly once,
   memd hearing nothing but `gone`, no order for the dying process; the test fails without the fix.
6. *A bound made a slow device a dead one*: a `usb-visible` run lost the second hide's swap-out
   and memd's journal write to `virtio-blk: timeout` ×4 → `slots exhausted` — the guest had
   stalled for ~2 s (a frame unacknowledged for 2.0 s just before), the driver's 2 s watchdog gave
   four requests up and their slots stayed busy for the rest of the boot. → **virtio-blk is
   reactive** (ADR-0089 decision 7): a request waits for the device's interrupt alone — no
   watchdog, no poll, no `yield_` (the line bound when the device opens, before the warm-up read;
   rung `blk: irq bound`) —, a failed device says so itself (`DEVICE_NEEDS_RESET` →
   `virtio-blk: device failed (needs reset)`), and slot ownership is a host-tested table
   (`inflight.rs`, 8 tests incl. 4 `test_reject_*`): a slot comes back only with its chain.
   Outside the package (noted): the SD host core's and gpud's device watchdogs — the SD
   controller's own error interrupts can replace the former (board storage, its own task).
7. *The harness was no observer* (operator, 2026-10-29: "the selftest client must be a pure
   observer"): after its ladder, selftest-client's end phase spun on `yield_()` for the whole
   lane — the one hart never idle, and under `-icount` the guest's clock running on the spin's
   instructions — and the input window opened before its last probe (the UI smoke) had run.
   → The input window opens after the harness's own last work; the end phase **sleeps on its
   declared timer** until the drill's next due look and for good once none is due (a board:
   at once); memd's server loop lost its two `yield_` calls (the waitset, or the blocking
   receive, is the wait). What the harness still does there is declared and profile-gated —
   the memory drill (QEMU visible lane) and the `lowmem` hold; an event channel from memd so the
   drill needs no `STATUS` look once a second is a follow-up (ADR-0089 decision 8).
8. *A boot failed the flash read-back* (board cycle 77; operator: "if the disk is a problem, it
   should be dynamic anyway"): one boot between the flash and `board-flash.sh --verify` failed
   `nxdisk0`/`nxdisk1` — per partition every image-owned one matched and exactly the three a
   running system writes differed (`bsb`, `state`, `data`). → The layout declares them
   (`storage::layout::LIVE`, with `trace` — the old special case); `nx image flash-plan` lists
   the `live` spans and gives every region a `verify_sha256` with them zeroed; the read-back
   reads them as zero (host: 2 new tests — a boot's writes to `bsb`/`state`/`data` leave every
   read-back digest unmoved, `test_reject_a_changed_static_partition_on_read_back`); the plan
   rebuilt from cycle 77's image read back clean from the board that boot had written.

**Found by the 6f-2 work (2026-10-28):**

1. *rngd gets its entropy device after it starts* (`policyd: mmio rng allow priv` follows `rngd:
ready`): a seed at its start always missed. The key is seeded on demand — the pager asks rngd
when a swap is first due (a memd → rngd route, rngd op 2, memd's identity alone); nobody else's
traffic decides whether swap works.
2. *A frozen task blocked in an IPC call would have had its reply written into a page a squeeze
took* (latent since 6f-1): the queued-reply tier copied to the user buffer `ensure_user_range`
had proven at the commit point, through the caller's address — a squeezed page has no PTE, a
fault in S-mode panics. The re-executed call now makes the buffer writable again first (a
squeezed page comes back, a swapped one parks the call, which stays committed).
3. *blkd serves eight volumes* — the `blkd: gpt ok (parts=8)` rung on both lists.
4. *The board's flash plan grew a chunk* (the user area ended where `swap` ends): `swap` is
volatile and is left out (`storage::layout::VOLATILE`) — the flash is as long as before, and a
boot that wrote slots never fails `--verify`.

**Not in 6f-0:** any consumer of the codec (the pool, the pager), the `swap` partition, the
`lowmem` profile, memd's journal; a sample of an app's heap (the image's machine code is the
harder content — the ratio here is a ceiling for the pool's gain); a checksum per blob (the swap
stage decides whether the partition's integrity is the blob's or the slot's).

## Definition of Done (per step)

1. The measurement folder answers the step's question with archived numbers from both lanes.
2. Host tests prove the step's invariant, with its `test_reject_*`.
3. `headless`, `visible` and `board-visible` are green on the REQUIRED ladder with the step's
   rungs and no new tolerated red.
4. The gate has been shown red once, with the transcript archived.
