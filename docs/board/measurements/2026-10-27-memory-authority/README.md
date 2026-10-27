<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-27 — the kernel says how tight memory is, one authority listens, a restorable app comes back where it was (TASK-0333 6e, ADR-0085/0086)

## Question

Nothing in the system knew how much memory was left, no one was told when it ran short, a hidden
app's memory could only be freed by a kill nobody ordered, and an app that was ended came back
empty. With 6e the kernel derives three watermarks from each bank's boot-time free and tells ONE
bound endpoint per level change; `memd` — the one memory authority — holds that endpoint, learns
every app process's band from the lifecycle authority, orders a kill through it at critical, and
tells the level onward to gpud and (through the compositor and app-host) to every app window as
the host event `MemoryPressure`; a cache's owner can hand its pages to the kernel (purgeable VMOs,
purged at warn before the frame leaves); the GPU's memory is charged to the client it is held for;
and an app with `restorable = true` writes its `@persist` fields when hidden or closed and reads
them back before its first frame. Does all of that hold on the board, with the real display, the
real GPU and the operator's apps — and does nothing regress?

## Instrument

- kernel: `KSELFTEST: mem watermarks ok (total= warn= crit=)`, `KSELFTEST: mem pressure notify ok
  (level=normal then=warn reject=denied free= total=)` — the watermarks from the banks, one frame
  per change, a second binder refused; `KSELFTEST: vmo purge ok (pages=4 purged=4 relocked=purged
  reject=bound,eager,mapped)` — the purge model's three refusals, the purge at warn, the lock that
  says so; `MM: pressure purge level=warn pages=`;
- the authority: `memd: ready` → `memd: pressure (level=normal free= total=)` → `SELFTEST: memd
  bound ok (level= bands=)` (the sink is memd's); `memd: band (pid= band=)` as the operator hides
  and shows apps; `memd: level told (level= source= gpud= windowd=)` on every kernel frame;
- GPU memory charged to its client: `gpud: gpu memory charged (client=windowd bytes= all= told=1)`
  at the framebuffer grant and after the compositor's GPU context opened (the board: its context
  buffers summed) → `memd: gpu charge (client=windowd bytes= total=)`;
- the restore (the operator): a restorable app (settings) hidden → `apphost: persist saved
  (fields= bytes= reason=hidden)`; closed with Super+Ctrl+W → `reason=close`; opened again →
  `SELFTEST: app restore ok (fields= bytes= warm_us=)` and the app where it was;
- NOT on the board: the drill (`SELFTEST: mem pressure drill`, `fanout ok`, `kill ok`) — it ends
  an app the operator may be using; the QEMU visible lane runs it (below);
- as before: `KSELFTEST: mm frames (… free= …)`, `KINIT: milestone 11`, the FAIL kinds, the UX
  windows (`gpud: ux frame`, `windowd: ux input`), the operator's series and acks.

## Before (board cycle 72b, `board--2026-10-27T11-31-07`, dev-f054fd80)

- no watermarks, no pressure endpoint, no memd, no purge, no GPU charge, no restore; a hidden app
  is frozen (6d) but never freed; `mm_stats` v3;
- frame census `free=996710` of 1 043 883 with six app-hosts (rss 7–8.8 MB each), gpud rss 85.7 MB
  + dma 24.9 MB, reserved 478 MB; `KINIT: milestone 11` 1 806 ms;
- FAIL kinds: `WINDOWD: FAIL imed relay send` 15×, `inputd: imed send FAIL` 16× (the typed-input
  defect, UX lane), `gpud: ux frame FAIL` 26×, `windowd: ux input FAIL` 20×, `init: FAIL declared
  slot route send slot=…8` 1×, `route-deny: bundlemgrd -> execd` 1×, `gpud: FAIL gpu gate scene`
  1× (allow-listed, layer 8), `FAIL internal` 1×, `settingsd QueueFull` 1×, `hidrawd: chain I2 wire
  send FAIL` 1×; the board selftests without RTC/RNG/keystore as every cycle.

## Hypotheses

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the kernel half holds on the board's banks | `KSELFTEST: mem watermarks ok (total= warn= crit=)` with `warn` = 25 % of the boot-time free (two banks summed), `KSELFTEST: mem pressure notify ok (… reject=denied …)`, `KSELFTEST: vmo purge ok (pages=4 purged=4 relocked=purged reject=bound,eager,mapped)` |
| H2 | one authority, the level and the bands reach it | `memd: ready`, `memd: pressure (level=normal …)`, `SELFTEST: memd bound ok (level=normal bands=)`; one `memd: band (pid= band=)` per lifecycle change of the operator's apps (foreground / visible / hidden / suspended), `gone` at each close; `memd: level told (… windowd=1)` (gpud=1 once gpud is up) |
| H3 | GPU memory is charged to its client | `gpud: gpu memory charged (client=windowd bytes= all= told=1)` at the grant and after `gpud: gpu library ok (client=windowd`, `bytes` ≥ the scan-out planes + the surface object's span, the second figure larger by the context's buffers; `memd: gpu charge (client=windowd bytes= total=)` with the same figures |
| H4 | a restorable app comes back where it was | the operator opens settings, navigates, hides it (`persist saved (… reason=hidden)`), shows it, closes it with Super+Ctrl+W (`reason=close`), opens it again: `SELFTEST: app restore ok (fields= bytes= warm_us=)` and the page it was on — ack `restore` only with these act lines |
| H5 | nothing regresses | the ladder complete (cycle 72b's 86 rungs + the new ones), every service `ready`, the operator's series (apps, tiling, modal, clipboard, screenshot, lifecycle), FAIL kinds ⊆ cycle 72b's set, `free=` and `KINIT: milestone 11` within noise of cycle 72b, the UX windows not worse |

## Results

### QEMU headless (`headless--2026-10-27T16-52-23` vs cycle 72's `headless--2026-10-27T10-48-36`)

- **H1** ✅: `KSELFTEST: mem watermarks ok (total=77251 free=43338 warn=19312 crit=7725 emerg=3862
  level=normal)` (one bank; warn = 25 % of the boot-time free, the floors unused at this size);
  `KSELFTEST: mem pressure notify ok (level=normal then=warn reject=denied free=77230 total=77251)`;
  `MM: pressure purge level=warn pages=4` → `KSELFTEST: vmo purge ok (pages=4 purged=4
  relocked=purged reject=bound,eager,mapped)`.
- **H2** ✅: `memd: ready` → `memd: pressure (level=normal free=72958 total=77251)` → `memd: level
  told (level=normal source=oom gpud=1 windowd=1)` → `gpud: mem pressure (level=normal source=oom)`
  (read when gpud came up — the push waited in its queue) and `windowd: mem pressure (level=normal
  source=oom windows=0)`; `SELFTEST: memd bound ok (level=normal bands=1)`.
- **H3** ✅: `gpud: gpu memory charged (client=windowd bytes=107827200 all=107827200 told=1)` at
  the framebuffer grant (the 1280×800 object: the scan-out planes and the atlas, whole) →
  `memd: gpu charge (client=windowd bytes=107827200 total=107827200)`; no context part on QEMU
  (its GPU is the display's own).
- **H5** ✅ (QEMU): ladder complete (`SELFTEST: Completed`, chain markers 13/13); census `free=`
  43 339 → 43 338, `excluded=4573` both; `KINIT: milestone 11` 940 → 934 ms; FAIL kinds the same
  set as the 6e-3 run before it (`settingsd QueueFull` 3/3, `dsoftbus` 2/2, `ux launch` 1/1,
  `declared slot route send slot=8` 1/1, `route-deny bundlemgrd -> execd` 1/1) plus ONE new
  line, `init: FAIL declared slot server recv slot=3` — gpud's server pair pinned twice once it
  entered init's minted table (the bulk distribution and gpud's own wiring arm); fixed before the
  visible run (the arm skips a pair already pinned, as windowd's does).
- Found on the way (the first 6e-3 visible run, `usb-visible--2026-10-27T16-32-13`): memd's leg
  to gpud had never been pinned and the harness's moved reply capability landed in that slot —
  see the ledger; the fix is the pair in init's table and a FAIL by name for a leg without one.

### QEMU visible (`usb-visible--2026-10-27T17-00-20` vs the 6e-2 run `usb-visible--2026-10-27T16-04-50`)

Exit 0, ladder complete. The drill runs here (not on the board): `warn` once a thawed app is on
screen (`resumed ≥ 1`, after the lifecycle phase), `critical` once an app is suspended (after the
restore's hide).

- **H1** ✅: as headless — `KSELFTEST: mem pressure notify ok (… reject=denied free=77230
  total=77251)`, `MM: pressure purge level=warn pages=4`, `KSELFTEST: vmo purge ok (pages=4
  purged=4 relocked=purged reject=bound,eager,mapped)`, `KSELFTEST: mem watermarks ok (total=77251
  free=42236 warn=19312 crit=7725 emerg=3862 level=normal)`.
- **H2** ✅: `memd: ready` → `memd: pressure (level=normal free=72949 total=77251)` → `memd: level
  told (level=normal source=oom gpud=1 windowd=1)`; `SELFTEST: memd bound ok (level=normal
  bands=1)`; the bands follow the harness's lifecycle series exactly as in the 6e-2 run (pid 77
  foreground; 106 foreground; 107 foreground → hidden → suspended → foreground → gone; 108
  foreground → hidden → suspended).
- **The level told onward (6e-3)** ✅, one chain, in order: `memd: level told (level=warn
  source=drill gpud=1 windowd=1)` → `SELFTEST: mem pressure fanout ok (level=warn sent=2)` →
  `gpud: mem pressure (level=warn source=drill)` → `windowd: mem pressure (level=warn source=drill
  windows=1)` → `SELFTEST: mem pressure drill (level=warn status=3 pid=0 band=foreground)` (status
  3 = nothing to kill at warn, as specified) → `apphost: mem pressure (level=warn handled=0)` (the
  settings app declares no `MemoryPressure` handler; the event reached it). At critical:
  `memd: kill (pid=108 band=suspended reason=drill)` → `memd: level told (level=critical …)` →
  `fanout ok (level=critical sent=2)` → `gpud`/`windowd: mem pressure (level=critical … windows=0)`
  → `SELFTEST: mem pressure drill (level=critical status=0 pid=108 band=suspended)` → `SELFTEST:
  mem pressure kill ok (band=suspended reason=drill)`.
- **H3** ✅: `gpud: gpu memory charged (client=windowd bytes=107827200 all=107827200 told=1)` →
  `memd: gpu charge (client=windowd bytes=107827200 total=107827200)` (QEMU: the lent framebuffer
  object only; the context part is the board's).
- **H4** ✅ (harness-driven): `apphost: persist saved (fields=3 bytes=87 reason=hidden)` → …
  `reason=close` → `memd: band (pid=107 band=gone)` → `memd: band (pid=108 band=foreground)` →
  `SELFTEST: app restore ok (fields=3 bytes=87 warm_us=18043)`.
- **H5** ✅ (QEMU): census `free=` 42 240 → 42 236 (`excluded` 4571 → 4573); `KINIT: milestone 11`
  953 → 988 ms (noise band of this lane); FAIL kinds ⊆ the 6e-2 run's set, every count equal or
  lower: `settingsd QueueFull` 4/4, `dsoftbus connect`/`ping` 1/1 each, `apphost: ux launch FAIL`
  4 → 3, `init: FAIL declared slot route send slot=8` 1/1 (keystored, pre-existing), `route-deny
  bundlemgrd -> execd` 1/1, `windowd: ux input FAIL` 8 → 2, `gpud: ux frame FAIL` 0/0; no
  `virtio-blk: timeout`; the headless run's `declared slot server recv slot=3` is gone.

### Board cycle 73 (`board--2026-10-27T20-36-32`, dev-6c38e8d2, vs cycle 72b)

Ladder complete: 95 rungs present (72b: 86 + the nine 6e rungs). Operator acks with act lines:
desktop, pointer, modal, tile (`wm tile` ×6, `wm chord` ×2), screenshot (`screencapd: saved
(kind=screen …)`), gpu-desktop, lifecycle (`minimize id=app` ×4 → `execd: freeze` → `abilitymgr:
suspend` → `execd: thaw`). NOT acked: `typed` and `clipboard` (no act line — the board's keyboard
defect below) and `restore` (settings was hidden but never closed on this boot; the close chord
is a letter chord). `board-test` therefore reports the missing operator markers.

- **H1** ✅: `KSELFTEST: mem watermarks ok (total=1043873 free=996672 warn=260968 crit=104386
  emerg=52193 level=normal)` — two banks summed, the watermarks 25 / 10 / 5 % of the pool's
  frames, the floors far below; `KSELFTEST: mem pressure notify ok (level=normal then=warn
  reject=denied free=1043849 total=1043873)`; `MM: pressure purge level=warn pages=4` →
  `KSELFTEST: vmo purge ok (pages=4 purged=4 relocked=purged reject=bound,eager,mapped)`.
- **H2** ✅: `memd: ready` → `memd: pressure (level=normal free=1039558 total=1043873)` → `memd:
  level told (level=normal source=oom gpud=1 windowd=1)` → `gpud: mem pressure (level=normal
  source=oom)`, `windowd: mem pressure (… windows=0)`; `SELFTEST: memd bound ok (level=normal
  bands=6)` with the operator's six apps; every hide/show a band push — `foreground`, `visible`,
  `hidden`, `suspended` (two apps frozen by the grace), back to `foreground` on the thaw.
- **H3** ✅: at the grant `gpud: gpu memory charged (client=windowd bytes=530841600
  all=530841600 told=1)` (the 1920×1080 layout block: scan-out planes + atlas) → `memd: gpu
  charge (client=windowd bytes=530841600 total=530841600)`; after `gpud: gpu library ok
  (client=windowd ctx=0x600 …)` the figure is 540 614 656 — the context's buffers, 9.8 MB on top —
  told again and taken by memd with the same figures. The second figure is larger by the context,
  as the hypothesis said.
- **H4** ◐: `windowd: minimize id=app2` → `memd: band (pid=89 band=hidden)` → `apphost: persist
  saved (fields=3 bytes=87 reason=hidden)` (settings, the restorable app, wrote its record at the
  compositor's `hidden`); the close and the reopen were not performed on this boot (see the
  keyboard defect) — `SELFTEST: app restore ok` stays the usb-visible lane's proof
  (`warm_us=18043`). The board's rung is open, re-run with the keyboard package.
- **H5** ✅ (within noise): census `free=` 996 710 → 996 672 (−38 frames: memd's process, its
  endpoints and the three new legs), `vmos` 171 → 174, `pt_frames` 279 → 277; `KINIT: milestone
  11` 1806 → 1853 ms (+47 ms: one more service in the fleet, memd's bind and its first frame);
  FAIL kinds ⊆ cycle 72b's set, the UX windows not worse (`gpud: ux frame FAIL` 26 → 24, `windowd:
  ux input FAIL` 20 → 17, imed relay/send 15+16 → 14+11, `settingsd QueueFull` 2 → 1, the board
  selftests without RTC/RNG/keystore as every cycle, `gpu gate scene FAIL` allow-listed); ONE new
  line, `WINDOWD: FAIL surface present rejected id=14 seq=27 status=4` right after `app window
  closed slot=0` — the calculator's last present after its close button, refused by the
  compositor, harmless (the process exited cleanly a line later). No `virtio-blk: timeout`.

**Verdict:** H1, H2, H3, H5 hold on the board; H4's hidden half holds, its close-and-reopen half
is the QEMU lane's until the keyboard package. 6e closes (operator 2026-10-27).

**Found, not 6e (open):** the board's keyboard path loses letters — the arrow chords reach the
compositor (`wm chord (zone=left-half`, `zone=return`), the letter chords Super+Ctrl+M / W never
do (no `wm chord`, no `wm tile deny`: the fact never leaves inputd), and typing has been red on
the board since cycle 64 (`inputd: imed send FAIL`, `WINDOWD: FAIL imed relay send`). The host rig
(the boot parser + inputd, `de` keymap) parks M and W as codes 14 and 15 (test extended), the QEMU
usb-visible lane types and closes with the same table. Measure on the board — the HID report under
Super+Ctrl, where the imed send fails — in the package after 6f; the operator asked for a Settings
page listing every keyboard shortcut in the same package.
