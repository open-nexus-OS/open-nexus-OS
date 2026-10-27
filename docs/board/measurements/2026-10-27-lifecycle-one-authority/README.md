<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-27 — a hidden app is frozen, and one authority holds every app grant (TASK-0333 6d, ADR-0082/0083/0084)

## Question

A hidden app kept running at full price and nothing could stop it short of killing it; an app's
permissions lived in two build-time tables (abilitymgr's and execd's) that drift the day one is
rebuilt alone. With 6d the parent orders its child whole (`task_suspend` / `task_resume` /
`task_kill`, a leader's exit takes its threads, a frozen receiver refuses the park), the
compositor states what is seen, the lifecycle authority runs one grace clock, the parent issues
the orders — and the bundle manager publishes every installed app's manifest grants to policyd,
which every launcher asks. Does a hidden app freeze and come back, does every launch ride the
ask, and does nothing regress on four harts?

## Instrument

- kernel: `KSELFTEST: process ctl ok (reject= frozen= frozen_send= killed=)` — the refusal matrix
  and the orders at boot; `KSELFTEST: task freeze ok (pid= resumed= frozen_ns= ran=0)` — no hart
  ran the frozen probe child; `KSELFTEST: process exit takes threads ok (pid= threads= space=freed)`
  — the compute broker's crash takes its worker threads and the reap frees the space;
- execd's probe: `SELFTEST: exec child freeze thaw ok` (a call to the frozen child refused with
  `Frozen`, the queued ping delivered on the thaw), `SELFTEST: task kill ok` (EOF, reaped as killed);
- the authority: `policyd: app grants (app= n=)` per installed app, `bundlemgrd: app grants
  published (n=k/k)`, `SELFTEST: app grants ok (app=calculator n= background=0)`, `SELFTEST: app
  grants put deny ok` (a publish from a stranger refused: `policyd: app grants denied (op=10
  sender=…)`); every launch: `abilitymgr: launch (app=, inst=)` with no `abilitymgr: launch denied
  (grants` and no `execd: FAIL app grants`;
- the lifecycle (visible / the operator): `windowd: minimize id=app` → `execd: freeze (pid=` →
  `abilitymgr: suspend (inst=` → `execd: thaw (pid=` → (QEMU) `SELFTEST: ability lifecycle ok
  (fg->hidden->frozen->fg)`;
- as before: `KSELFTEST: mm frames (… free= …)`, `KINIT: milestone 11`, the FAIL kinds, the UX
  windows (`gpud: ux frame`, `windowd: ux input`).

## Before (board cycle 71, `board--2026-10-26T17-05-01`, dev-64f3fda1)

- no process orders; a hidden app runs on; abilitymgr and execd each compiled the manifests'
  `caps` into their binary (`caps ok` self-check at boot);
- `KINIT: milestone 11` 1 743 ms; frame census `free=1012851` (two banks, `excluded=4554`);
- FAIL kinds: `WINDOWD: FAIL imed relay send` 14× (the typed-input defect, UX lane), `init: FAIL
  declared slot route send slot=…8` 1×, `route-deny: bundlemgrd -> execd` 1×, `gpud: FAIL gpu gate
  scene` 1× (allow-listed, layer 8), `FAIL internal` 1×; acks desktop, pointer, tile, modal,
  clipboard, gpu-desktop.

## Hypotheses

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the kernel half holds on four harts | `KSELFTEST: process ctl ok (reject=4 frozen=1 frozen_send=1 killed=1)`, `KSELFTEST: task freeze ok (… ran=0)`, `SELFTEST: exec child freeze thaw ok`, `SELFTEST: task kill ok`; the broker's three crashes each `process exit takes threads ok (… space=freed)` and its respawns `mappers=1` |
| H2 | one authority, every launch rides the ask | `policyd: app grants (app=` for every installed UI app and `bundlemgrd: app grants published (n=k/k)` with k = the installed count; `SELFTEST: app grants ok (app=calculator n=1 background=0)` + `put deny ok`; every app the operator opens has its `abilitymgr: launch (app=` and no `launch denied` / `execd: FAIL app grants` |
| H3 | a hidden app is frozen and comes back | the operator hides the focused app (Super+Ctrl+M), waits past the 5 s grace, restores it: `windowd: minimize id=app` → `execd: freeze (pid=` + `abilitymgr: suspend (inst=` → `execd: thaw (pid=`; the app returns where it was; the shell smooth meanwhile (no `gpud: FAIL dc present`, the UX windows not worse than cycle 71) — ack `lifecycle` only with these act lines |
| H4 | nothing regresses | the ladder complete (cycle 71's 79 rungs + 7 new), every service `ready`, the operator's series (apps, tiling, modal, clipboard, screenshot), FAIL kinds ⊆ cycle 71's set, `free=` and `KINIT: milestone 11` within noise of cycle 71 |

## Results

### QEMU headless (`headless--2026-10-27T10-48-36` vs cycle 70's `headless--2026-10-26T16-16-36`)

- **H1** ✅: `KSELFTEST: process ctl ok (reject=4 frozen=1 frozen_send=1 killed=1)` (line 183);
  `KSELFTEST: task freeze ok (pid=66 resumed=1 frozen_ns=30465200 ran=0)`; `SELFTEST: exec child
  freeze thaw ok`; `SELFTEST: task kill ok`; `process exit takes threads ok` 3/3 (pids 63, 94, 97,
  `threads=2 space=freed`).
- **H2** ✅: `policyd: app grants (app=calculator n=1) … (chat n=2) (desktop-shell n=7) (greeter
  n=3) (ime-ui n=3) (settings n=3) (stash n=3)`, `bundlemgrd: app grants published (n=7/7)` —
  seven installed UI apps, seven published, before the first volume answer; the greeter's launch
  (`abilitymgr: launch (app=greeter, inst=1)`) routed on the asked set, no denial, no FAIL;
  `SELFTEST: app grants ok (app=calculator n=1 background=0)`; the harness's publish refused
  (`policyd: app grants denied (op=10 sender=0x52c6…)`) → `SELFTEST: app grants put deny ok`.
- **H3**: the visible lane's rung (below).
- **H4** ✅ (QEMU): ladder complete (`SELFTEST: Completed`, chain markers 13/13, evidence bundle
  assembled); FAIL kinds the same four as cycle 70's headless (`settingsd: event send FAIL
  chan=11 err=QueueFull` 4× vs 3×, `init: FAIL declared slot route send slot=8` 1/1, `route-deny:
  bundlemgrd -> execd` 1/1, `apphost: ux launch FAIL (first_frame_ms=…)` 1/1 under TCG — all
  pre-existing, none new); census `free=` 43 399 → 43 383 (−16 frames, noise), `excluded=`
  4 552 → 4 563 (the bootfs grew by the policy and the grants code); `KINIT: milestone 11`
  929 → 935 ms.
- Found on the way (fixed before the green run): the ladder listed the kernel's `task freeze ok`
  after the exec-elf chain — the probe runs at execd's start; the proof manifest did not know
  `SELFTEST: exec child freeze thaw ok` / `SELFTEST: task kill ok` (evidence `unknown_marker`).

### QEMU visible (`just ci-os-usb-visible`, `usb-visible--2026-10-27T11-04-46`)

- **H3** ✅ (QEMU): the injector's chord on the focused app — `windowd: minimize id=app0` (line
  2424) → `execd: freeze (pid=104)` + `abilitymgr: suspend (inst=3)` (2429/2430, after the 5 s
  grace) → the chord again → `execd: thaw (pid=104)` (2444) → `SELFTEST: ability lifecycle ok
  (fg->hidden->frozen->fg)`; `SELFTEST: ui v7 screenshot ok` after it, the ladder complete
  (exit 0). The run before (`usb-visible--2026-10-27T10-51-48`) ran the same chain and missed the
  rung: its literal carried `→`, and the UART path drops non-ASCII bytes (`(fghiddenfrozenfg)`) —
  the marker is ASCII now, in the emitter, the manifest, the ladder and the docs.
- **H2** ✅ (visible too): `SELFTEST: app grants ok (app=calculator n=1 background=0)`, `put deny
  ok`; every app the injector launched rode the ask (no `launch denied`, no `execd: FAIL app
  grants`).
- FAIL kinds: `dsoftbus os connect FAIL` / `dsoftbus ping FAIL` (single-VM proof boot, held —
  pre-existing), no `virtio-blk: timeout` (the lane ran alone).

### Board cycle 72 (`board--2026-10-27T11-18-29`, build dev-f054fd80, vs cycle 71)

Judge: `board-test --profile=board-visible --log` **[FAIL] — rung missing: `inputd: live keyboard
route on` (82 of 86)**: the operator's series had no typing this run (the chord is a hotkey,
not a dispatched key; cycle 71 lit the rung at line 3038 when text was typed). Every other rung
present, the seven new ones included. **Verdict: not a pass — cycle 72b with the typed step.**

| # | Verdict | Evidence |
|---|---|---|
| H1 | ✅ | `KSELFTEST: process ctl ok (reject=4 frozen=1 frozen_send=1 killed=1)` (line 128); `KSELFTEST: task freeze ok (pid=66 resumed=1 frozen_ns=30512625 ran=0)`; `SELFTEST: exec child freeze thaw ok`; `SELFTEST: task kill ok`; the broker's three crashes `process exit takes threads ok (pid=63/96/101 threads=2 space=freed)`, its image `vmo=108 … mappers=1` after each respawn (cycle 70 climbed 1→4) |
| H2 | ✅ | `bundlemgrd: app grants published (n=7/7)` (559); `SELFTEST: app grants ok (app=calculator n=1 background=0)`, `put deny ok`; six launches (`greeter`, `desktop-shell`, `calculator`, `chat`, `settings`, `stash`) each `abilitymgr: launch (app=`, 13 `execd: app route granted`, no `launch denied`, no `execd: FAIL app grants` |
| H3 | ✅ act lines · ack open | `windowd: minimize id=app2` (2437) → `abilitymgr: bg (inst=5)` → `execd: freeze (pid=99)` + `abilitymgr: suspend (inst=4)` (2440/2441) → `windowd: transition restore` → `abilitymgr: fg (inst=4)` → `execd: thaw (pid=99)` → `abilitymgr: thaw (inst=4)` → `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)`; UX windows around it `windowd: ux input ok (to_submit_us=30739 of=16 budget_us=50000)` — the ack `lifecycle` waits for the operator's word on the return |
| H4 | ❌ (one new FAIL kind) | ladder: 85/86 (the typed rung, operator); `KINIT: milestone 11` 1 824 ms (1 743; noise band of a cold eMMC boot); census `free=1014047` (1 012 851), `excluded=4565` (4 554); FAIL kinds: `imed relay send` 0× (14× — no typing), `gpu gate scene` 1/1, `declared slot route` 1/1, `route-deny bundlemgrd -> execd` 1/1, `internal` 1/1, `settingsd QueueFull` 1 (0 — seen in every QEMU boot), `hidrawd: chain I2 wire send FAIL (inputd route)` 1 (0), and **NEW: `WINDOWD: FAIL desktop event send` 1× + `WINDOWD: FAIL desktop input send` 11× — the desktop shell died** (below) |

**The shell's death (open, not a 6d code path on the evidence).** `[USER-PF] STORE @
sepc=0x1fc9d2 stval=0x1ffee1a8 pid=0x5e sp=0x1ffee1a0` → `execd: exit pid=94 reason=fault code=-22`
→ `abilitymgr: stop (inst=2 app=desktop-shell pid=94 code=-22 live=4)`; no relaunch; every later
desktop tap is `FAIL desktop input send`. The faulting instruction is `sd ra, 8(sp)` in a 16-byte
leaf frame (a byte compare, `.text+0x7f9ce`, called from `.text+0x505e6`) with `sp` 73 304 B below
the user stack top (`0x2000_0000`): the app-host's stack (16 pages) was already exhausted — a
**stack overflow** of the shell, ~7.7 KiB past the bottom. Operator: "played with the no-blur mode,
then tried to take screenshots" — the trace shows the tray taps `(1734,13) hit=Some(34)
dmg=Some(Layout)` ×2 (3163, 3181; cycle 71 never tapped items 34/37/40), `windowd: raise
id=desktop z=39`, then the fault; no screencapd line reached before it. Nothing of 6d runs in the
shell (no 6d message reaches an app; inst 2 was never hidden or frozen); the shell's binary is
cycle 71's (`target/…/release/app-host` 10:34, unchanged by 6d). Follow-up task: the shell's
deep frame under no-blur + screenshot (symbols are stripped in the OS build — build the shell
with symbols and walk `.text+0x505e6`), and a guard page + a measured stack size for app-hosts.

### Board cycle 72b (`board--2026-10-27T11-31-07`, the same image dev-f054fd80)

The series with typing, the chord cycle on three apps, then the no-blur toggle and two screenshots.
Operator: "hat jetzt alles geklappt". Ladder **86/86 present** (judge: only operator acks pending);
acks given with their act lines: desktop, typed (`inputd: live keyboard route on`, 3120), pointer,
modal (17 lines), clipboard (25), screenshot (`screencapd: saved (kind=window … bytes=63042)`,
`(kind=screen w=1920 h=1080 bytes=1036045)`), gpu-desktop, **lifecycle** (below). `tile`: the four
quadrant tiles are in the log (`windowd: wm tile (zone=top-left id=app3)` …), no `wm chord` line —
the ack waits for the operator's word on the chord.

| # | Verdict | Evidence |
|---|---|---|
| H1 | ✅ | `process ctl ok (reject=4 frozen=1 frozen_send=1 killed=1)`; `task freeze ok (pid=66 … ran=0)`; `exec child freeze thaw ok`; `task kill ok`; the broker's image `mappers=1` ×4 |
| H2 | ✅ | `app grants published (n=7/7)` (556); `SELFTEST: app grants ok (app=calculator n=1 background=0)`, `put deny ok`; six launches, each asked, 13 routes granted, no denial, no FAIL |
| H3 | ✅ | three apps hidden: `execd: freeze (pid=86)` + `abilitymgr: suspend (inst=4)`, `(pid=87)`/`(inst=5)`, `(pid=98)`/`(inst=6)` (2475–2482) → `execd: thaw (pid=86)` → `SELFTEST: ability lifecycle ok (fg->hidden->frozen->fg)` (2533) → `thaw (pid=87)`, `thaw (pid=98)`; the operator: the apps came back where they were, the shell smooth — ack `lifecycle` given |
| H4 | ✅ (the shell lived) | **no `FAIL desktop input send`, no shell fault** — the no-blur + screenshot sequence did not reproduce cycle 72's stack overflow (it stays an open follow-up, not a 6d path); FAIL kinds = cycle 71's set (`imed relay send` 15 vs 14 and `inputd: imed send FAIL` 16 vs 3 — the typed-input defect of the UX lane, grows with typing; `gpud: ux frame FAIL` 26 vs 27, `windowd: ux input FAIL` 20 vs 20; the board selftests without RTC/RNG/keystore as every cycle), `apphost: ux launch` warn 803/739 ms, ok 291/283 ms (cycle 71: warn 671–893, ok 218/285); `KINIT: milestone 11` 1 806 ms (1 743); census `free=996710` at the census (cycle 71 `1012851` — more apps open at the census this run, `allocs=16493` vs 6 909) |

**Verdict:** 6d holds on the board: ADR-0082/0083/0084 proven on four harts; the lifecycle chain
and the one authority live. Two open items, not 6d's: the shell's stack overflow of cycle 72
(no-blur + screenshot, not reproduced in 72b) and the tile chord ack.
