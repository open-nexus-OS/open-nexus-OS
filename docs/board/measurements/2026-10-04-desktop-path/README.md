# 2026-10-04 — windowd's desktop on the display controller (TASK-0251 P2a step 2)

The board-cycle protocol of D4 step 2 (`.claude/skills/driver-bringup`): what was measured
before the cycle, which hypotheses the cycle decides and by which observation, and the gates
the trace is judged against. Written before the cycle; the verdict is added after it.

## Question

Does the desktop path the controller will run work at the board's resolution through the same
CPU executor, which line proves each hop on the board, and what can go wrong only on the
hardware (the switch from the splash to windowd's framebuffer, the caches, the cost on the
board's harts)?

## Instrument

1. **QEMU's 2D scanout at the board's mode** — the `smp1` lane with
   `QEMU_GPU_XRES=1920 QEMU_GPU_YRES=1080 QEMU_MEM=512M`. The 2D virtio scanout runs exactly the
   CPU executor the controller runs (`gpud backend::cpu_frame`) over the same layout; only the
   last hop differs (transfer + flush there; a cache clean, and once the switch, here). Run
   before the change (M1) and on the new request loop (M2).
2. **The board's vocabulary** — `nexus-proof-manifest verify-uart --profile=board-visible` on
   the M1 trace: every line the desktop chain prints is declared for the board profiles (no
   surprise lines on the board).
3. **Step 1's board trace** (`../2026-10-03-first-light/`): the controller scans a contiguous
   block in bank 0 (`bus=0x8fd2000`), the harts clean in 64-byte blocks
   (`KINIT: user cache maintenance zicbom block=64`), the 99.5 MB layout block was made for the
   controller at bring-up without complaint; sessiond never started there
   (`FAIL stage wait svc=sessiond stage=session-start`) because windowd never reported
   `DisplayReady` — its framebuffer request was refused.

## Results

- **M1** (`build/logs/smp1--2026-10-04T11-40-59`, the code before the change): the whole chain
  at 1920x1080 — `gpud: framebuffer granted (1920x1080)` → `windowd: display mode from gpud
  (1920x1080)` → `gpud: handoff attach ack` → `gpud: chain G1…G4` → `gpud: cursor uploaded`
  (the software cursor: windowd blends it) → `sessiond: ready` → `APPHOST: mounted hash=…` →
  `WINDOWD: desktop surface created id=1 1920x1080` → `gpud: desktop reveal (handshake seq=4)`
  → `windowd: desktop revealed (seq=4)`; lane PASS, chain-marker contract 13/13.
- **Vocabulary**: `verify-uart --profile=board-visible` on the M1 trace: ok.
- **M2** (`build/logs/smp1--2026-10-04T12-08-45`, the new request loop): the same chain at
  1920x1080, `windowd: desktop revealed (seq=4)`, lane PASS, chain-marker contract 13/13; the
  virgl `visible` lane's pixel statistics equal every run since 2026-09-30 (desktop mean luma
  11.97, non-black 25.79 %) — the GL path is unchanged.
- **M3** (`build/logs/visible-fhd--2026-10-04T14-38-36`, a one-off: the `visible-fhd` lane with
  `GPU_MODE=mmio`, the 2D scanout captured over VNC at 1920x1080 — the picture the board's CPU
  executor composes, 1:1): the greeter with the wallpaper over the whole screen and crisp text;
  against the GL path's capture (`visible-fhd--2026-10-04T12-50-31`) the avatar circle's top is
  cut flat (the CPU path composites the layer without the GPU's rounded mask and scaling) and the
  glass reads as flat blue (the CPU blur is the box blur, no tint) — the next step's work.

## Hypotheses the board cycle decides

- **H1 — the switch latches with the bring-up's two latch words.** The reveal writes the
  display plane's address and stride into RDMA1, then config-ready and software start (the
  writes that brought the splash on screen from a stopped pipeline). Decided by
  `gpud: dc reveal flip ok (… cfg-ready cleared after N reads)` and the operator seeing the
  greeter. Alternatives: **H1'** software start on a running pipeline restarts the scan (a
  flicker at the switch, the greeter after it); **H1''** the configuration does not latch
  (`cfg-ready still set`, the splash stays on the monitor).
- **H2 — a full-screen present costs well under 100 ms on the board's harts.** Measured by
  `gpud: dc first present (exec_us=… clean_us=… damage=1920x1080)`. Seconds would delay the
  display stage past sessiond's 2 s liveness bound (`FAIL stage wait svc=sessiond`, a witness,
  not a FAIL rung) — a measurement for the next step, not a gate of this one.
- **H3 — the session runs on the board once windowd reports `DisplayReady`.** Decided by
  `sessiond: ready`, `APPHOST: mounted hash=`, `WINDOWD: desktop surface created id=1
  1920x1080` in the trace (the hops M1 shows on QEMU, in that order).
- **H4 — the cleaned framebuffer shows exactly what windowd composed.** Decided by the
  operator: the greeter without stale rectangles or garbage (a missed clean shows as a stale
  rectangle, never as a red line).

## Gates (`scripts/board-test.sh --profile=board-visible`)

1. The 20 rungs of `board-headless`, unchanged — the desktop path costs no rung.
2. `gpud: framebuffer granted (` — the layout block exists for the controller and windowd holds
   a clone.
3. `windowd: display mode from gpud (` — windowd composes at the controller's mode.
4. `gpud: dc reveal flip ok (` — the switch's words read back.
5. `windowd: desktop revealed` — the reveal ack, sent only after the switch took (RFC-0093 §5,
   `splash_hold`).
6. No `gpud: FAIL` / `(K)SELFTEST: … FAIL` line outside the allow lists.
7. Operator: `board-visual: desktop` — the greeter of THIS boot on the monitor.

What a red gate means, decided in advance:

| red | with | means | next measurement |
|---|---|---|---|
| 2 | `gpud: FAIL dc framebuffer (no contiguous block …)` | the kernel found no 99.5 MB run for the controller | the kernel's free contiguous ranges after boot |
| 4 | `gpud: FAIL dc reveal flip (readback …)` | the RDMA words do not take a write while scanning | the words before/after the write, with the pipeline stopped |
| 5 | 4 green | the hold or the ack (host-tested: `splash_hold`, `reply::present_status`) | the gpud lines between `OP_REVEAL` and the next present |
| 7 | splash still on the monitor, `cfg-ready still set` | H1'': the configuration never latched | the raw interrupt word (`cfg_eof`) around the switch |
| 7 | black or garbage | the bus address/stride of the switch, or a missed clean | the switch's `bus=` against the grant's run |
| 7 | the desktop without the greeter | the reveal came before the session's surface | windowd's reveal gate lines |

## Verdict (cycle 1, 2026-10-04 — `board-boot-2026-10-04-desktop.txt`)

**Every gate green, the desktop on the monitor in the first cycle.** `[PASS] board-visible: 24
rungs, 1 operator marker(s), no FAIL marker, manifest clean`; `[PASS] board-headless` with its
20 rungs on the same trace. The chain, in the trace's order: `gpud: dc scanout ok (1920x1080@60
cea bus=0x5000000 vsync)` (the splash plane) → `gpud: framebuffer granted (1920x1080)` →
`windowd: display mode from gpud (1920x1080)` → `gpud: handoff attach ack` (12 ms) →
`gpud: dc first present (exec_us=14044 clean_us=824 damage=1920x1080)` → `sessiond: ready` →
`APPHOST: mounted hash=…` → `WINDOWD: desktop surface created id=1 1920x1080` →
`gpud: dc reveal flip ok (bus=0x8fd2000 stride=7680 readback 3/3 cfg-ready cleared after 0
reads)` → `gpud: desktop reveal (handshake seq=4)` → `windowd: desktop revealed (seq=4)`. The
operator saw the greeter (`board-visual: desktop`).

- **H1** decided: the switch takes with address + stride + config-ready + software start on the
  running pipeline — the greeter replaced the splash. Config-ready reads 0 on the first read
  after the write: the latch is not observable through that word (it clears at once or reads as
  zero); a frame-exact flip needs the vsync interrupt (tear-free step).
- **H2** decided: a full-screen present costs 14.0 ms on the board's harts plus 0.8 ms to clean
  8.3 MB out of the caches.
- **H3** decided: once windowd reports `DisplayReady` the session runs on the board (step 1's
  `FAIL stage wait svc=sessiond` is gone).
- **H4**: the operator saw the greeter; what they described is the next step's input:
  1. *long black before the logo* — the kernel's LED milestones alone run to 23.4 s
     (`KINIT: milestone 11 at 23432 ms`), the splash comes with gpud's bring-up after that;
  2. *a very short flicker as the logo appears* — the monitor locking onto the new signal (the
     encoder's PLL and the first frame), not yet separated from the switch;
  3. *the wallpaper not over the whole screen*, 4. *vector graphics look chopped*, 5. *text not
     sharp* — M3 shows the CPU executor's picture at 1:1: the wallpaper covers the screen, the
     text is crisp, the avatar circle's top is cut flat (the CPU layer composite lacks the GPU
     path's rounded mask and content scaling). The monitor's own mode is 2560x1440 (its EDID's
     preferred timing, `../2026-09-29-display-regs/`): a 1920x1080 signal is scaled by 4/3 in
     the monitor, which softens text — to be checked against the stock system at 1080p on the
     same monitor before anything is changed for it.
- Unchanged reds, present in step 1's trace too: `KSELFTEST: bkl budget FAIL` (max hold 23 ms),
  `netstackd: net FAIL internal`, the eight tolerated board reds.
