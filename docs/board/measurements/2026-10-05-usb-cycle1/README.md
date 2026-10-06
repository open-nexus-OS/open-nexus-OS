# 2026-10-05 — USB on the board, cycle 1 (TASK-0328 U3, RFC-0099 §6, RFC-0106 amended)

Our chain's first boot with the USB glue (`dev-3308`): socd brings the host node up before
xhcid touches the controller. The boot trace (`board-boot-2026-10-05-usb-cycle1.txt`, RFC-0107)
holds no typed text — nothing the operator pressed reached the system (see below), and the
chain never logs key codes.

## Question

What did our loader leave of the USB host's glue, and does the measured glue word
(`0xd4282bc8 = 0x0b008000`, stock) take a write?

## Results

- `init: usb host from tree (base=0xc0a00000 irq=125 phy=yes)` — the tree's host and PHY nodes
  were found and granted.
- `socd: bring-up /soc/storage-bus/usb@c0a00000 FAIL (step=read-back reg=apmu+0x3c8
  val=0x8008000) apmu+5c:0>f00 apmu+3c8:8008000>8008000`:
  - the USB clock/reset word (`apmu+0x5c`) was **0** after our loader — the DWC3's resets held
    and its clock gated — and the bring-up set it to `0xf00` (the three resets released, the
    clock on), read back;
  - the glue word (`apmu+0x3c8`) read `0x08008000` before the write; written as the stock
    `0x0b008000` it read back `0x08008000` — **bits 24..25 ignore a write** (status, not glue;
    the stock system shows them set while its driver runs the PHYs); bit 15 was up already.
- xhcid refused to touch the controller (`xhcid: FAIL (step=glue-host cc=4)`); the hub was not
  brought up, keyboard and mouse did nothing — as designed for a failed glue.
- The rest of the ladder as before (`stage: display-ready`, `windowd: desktop revealed`).

## Decision

The glue word's binding carries a mask (`nexus,glue-words = <&provider offset mask value>`,
`#nexus,glue-cells = <3>`): socd sets only the mask bits and leaves a word's status bits
alone. The USB host's mask is bit 15 (`0x8000`), already up after the loader — the step writes
nothing. Cycle 2 reaches the hub, the DWC3 and the PHY measurement.

## Cycle 3 (same day; `board-boot-2026-10-05-usb-cycle3.txt` — cycle 2 was cycle 1's OS volume again, flashed without a rebuild)

- `socd: bring-up /soc/storage-bus/usb@c0a00000 ok (domains=1 resets=3 clocks=1 rates=0 pads=0
  gpios=0 writes=4) apmu+5c:0>f00 apmu+3c8:8008000>8008000` — the masked glue word writes
  nothing (bit 15 up), the clock word is ours.
- `socd: bring-up /usb-hub ok (… pads=3 gpios=3 writes=8) pinctrl+1e4:b040>b041
  pinctrl+23c:b040>b040 pinctrl+240:b040>d040 gpio+100:20294000>38294002 gpio+10c:1>18000003`
  — the loader leaves the hub's pads at `0xb040` (function 0, pull-down) and the lines as
  inputs; after: 97 on function 1, 124 pulled up, bank 3 lines 1/27/28 outputs driven high
  (the level word read back), the 200 ms settle before VBUS.
- `xhcid: dwc3 host (id=0x5533330a gctl=0x32c92004>0x32c91004 usb2phycfg=0x40102400>…
  usb3pipectl=0x10c0003>… writes=1)` — the core came up in device mode (the strap); one write
  made it host. `GUSB3PIPECTL` differs from the stock `0x01080003` in bits 18..19 (left alone).
- `xhcid: usb2 phy (stock words 7/12 match, first differs at 0x4=0x60e9)` — **five of the
  twelve PHY words differ after our loader** (stock `0x60ef` at 0x04).
- `xhcid: FAIL (step=controller cc=0)` — a bounded controller wait ran out (which one, the
  line did not say yet).

## Decision (cycle 4)

The PHY step exists: xhcid writes the differing USB 2.0 PHY words to the stock values (read
back) BEFORE the DWC3 and the controller, whose reset waits on the PHY's clock; the controller
failure names its wait (`cc=` 1 ready, 2 halt, 3 reset, 4 start) and the command/status words
read last (`xhcid: controller stuck (…)`). If the reset still hangs with the USB 2.0 PHY at
stock, the SuperSpeed PHY (combo PHY, `phy@c0b10000` + `phy_sel`; its stock words are in
`2026-10-04-usb-topology/stock-usb-regs.txt`) is the next hypothesis.

## Cycle 4 (`board-boot-2026-10-05-usb-cycle4.txt`)

- `xhcid: usb2 phy (stock words 7/12 match 0x4=0x60e9 0x10=0x11 0x18=0x105 0x34=0x10
  0x38=0x0)` — the five words after our loader.
- `xhcid: FAIL (step=phy cc=1) 0x38=0x0 after 5 writes` — four words took the stock values;
  **`0x38` written as `0x8011` read `0x0` back**: a status word (bit 15 the PLL's lock by the
  look of it), compared from now on, never written.
- `xhcid: dwc3 host (… writes=1)` as in cycle 3; `GUSB3PIPECTL` `0x010c0003` keeps bit 18 the
  stock clears by quirk (`dis-del-phy-power-chg`).
- `xhcid: controller stuck (phase=3 usbcmd=0x2 usbsts=0x11)` → `FAIL (step=controller cc=3)`:
  **HCRST never completes** (`USBCMD.HCRST` stays set, `USBSTS` = HCH | CNR) — the xHCI's
  reset waits on a PHY clock it does not get.

## Decision (cycle 5)

Three things the stock chain does before its xHCI and ours did not, in the dwc3 driver's
order: the PHY words set and given time to lock (a 20 ms hold, the status word read again),
the SuperSpeed PHY's words compared with the stock's (a measurement: `xhcid: ss phy (…)` —
nothing written), and the DWC3 **core soft reset** (core and both PHYs held in soft reset,
released with two 100 ms holds) before the port capability is set — plus bit 18 of
`GUSB3PIPECTL` cleared as the stock quirk does. If HCRST still hangs, the SuperSpeed PHY's
diff names the next step.

## Cycle 5 (`board-boot-2026-10-05-usb-cycle5.txt`)

- `socd: bring-up /usb-hub FAIL (step=read-back reg=gpio+0x100 val=0x28294000) …
  gpio+100:20294000>38294000 gpio+10c:1>18000001` — the hub's second line (GPIO 124, bit 28):
  the level word read right after the set still showed the old level (`0x28294000`), the
  marker's later read the new one (`0x38294000`): **the level word follows the pin**, with a
  propagation the read-back did not allow for (cycles 3 and 4 happened to read it in time).
- The hub's failure stopped xhcid before the PHYs and the core — cycle 5 measured nothing of
  the controller.

## Decision (cycle 6)

The GPIO read-back reads the level word again, bounded (1000 bus reads), before it faults; a
hub failure no longer stops the controller's own bring-up (its PHYs, the core soft reset, the
reset) — the ports stay unpowered, the measurement goes on.

## Cycle 6 (`board-boot-2026-10-05-usb-cycle6.txt`)

- The hub up (the level read-back polled); the USB 2.0 PHY's four words set, read back; after
  20 ms `11/12 match 0x38=0x0` — **the status word stays 0** (stock `0x8011`: no lock).
- `xhcid: ss phy (stock words 0/23 match …)` — **every word of the SuperSpeed PHY block reads
  0**: the block is not clocked or still in reset (the stock tree gives it only `phy_rst`).
- `xhcid: dwc3 reset ok (core + phys, 2x100 ms)`, host mode with bit 18 cleared (`usb3pipectl
  … >0x1080003`, the stock word) — and the controller still stuck in its reset (`phase=3
  usbcmd=0x2 usbsts=0x11`).
- The stock APMU USB word (`0x5c`) is `0x0f33`; ours `0xf00`: the stock also runs the USB AXI
  clock and its reset (bits 0, 1) and the second EHCI's (bits 4, 5).

## Decision (cycle 7)

Two measured differences closed: the host node binds the USB AXI clock and reset too (bits 0
and 1 of `0x5c`, as the stock runs it), and the SuperSpeed PHY node carries the APMU lane
select as a glue word (`0x110`, stock `0x8`) brought up through socd before the controller;
the SuperSpeed PHY's words are measured again. If the block still reads 0 and HCRST still
hangs, the next hypothesis is the PHY's own clock gate outside the tables.

### Gates for cycle 7 (set before the cycle)

| Line | Expected (from the stock measurements) | Decides |
|---|---|---|
| `socd: bring-up /soc/storage-bus/usb@c0a00000 ok (… resets=4 clocks=2 …) apmu+5c:0>f03` | the USB word ends at `0xf03` (stock `0x0f33` minus the second EHCI's bits 4, 5) | the AXI clock and reset are ours now |
| `socd: bring-up /soc/storage-bus/phy@c0b10000 ok (… writes=N) apmu+110:X>8` | `X` = what the loader left; `8` after | whether the lane select had to be set |
| `xhcid: ss phy (stock words N/23 …)` | `N > 0` and not every word `0x0` | the block is clocked: the glue reached it; `0/23` all zero → a clock gate outside the tables, next hypothesis |
| `xhcid: usb2 phy after 20 ms (… 0x38=…)` | `0x8011` = the PLL locked as stock | whether the USB 2.0 PHY runs at all |
| `xhcid: controller ok (version=1.10 ports=2 slots=64 csz=64 scratch=1 irq=125)` | the ladder's rung | HCRST completed: the PHY clocks were the blocker |
| else `xhcid: controller stuck (phase=3 …)` | — | the SuperSpeed PHY's own init (the stock's 23 words) is the next step |

## Cycle 7 (`board-boot-2026-10-05-usb-cycle7.txt`) — against its gates

| Gate | Read | Verdict |
|---|---|---|
| `apmu+5c:0>f03` | `apmu+5c:0>f03` (resets=4 clocks=2 writes=6) | the AXI clock and reset are ours ✓ |
| `apmu+110:X>8` | `apmu+110:0>8` (writes=1) | the loader leaves the lane select 0; set now ✓ |
| `ss phy … N > 0` | `0/23 match`, every word `0x0` | **the block is still dead** — a gate outside the tables |
| `usb2 phy after 20 ms … 0x38` | `0x38=0x0` | **the USB 2.0 PHY's PLL never locks** |
| `controller ok` | `controller stuck (phase=3 usbcmd=0x2 usbsts=0x11)` | HCRST still waits on a PHY clock |

Both PHYs stay dead with every gate the tables know open: whatever feeds them (a PLL, a
reference clock, a power switch) is a word the tables do not cover.

## Decision (cycle 8 — a measurement)

socd compares, at its start and before any bring-up, every APMU and MPMU word the stock dumps
hold with what our loader left (`socd: apmu vs stock (N differ) off:ours/stock …`, `socd: mpmu
vs stock (…)`); the USB-relevant differences name the next gate. Nothing else changes.

## Cycle 8 (`board-boot-2026-10-05-usb-cycle8.txt`) — the measurement

`socd: apmu vs stock (40 differ)` and `socd: mpmu vs stock (21 differ)` at socd's start, before
any bring-up (ours/stock). The USB-relevant words:

| Window | Offset | Ours | Stock | Reading |
|---|---|---|---|---|
| APMU | `0x05c` | `0` | `0xf33` | the USB clocks/resets (ours since cycle 7, by the bring-up) |
| APMU | `0x07c` | `0x08008000` | `0x0b008000` | the shape of `0x3c8`: bits 24..25 status (a PHY ready) |
| APMU | `0x110` | `0` | `0x8` | the lane select (set by the glue since cycle 7) |
| APMU | `0x144` | `0x890789c0` | `0x894fffe8` | a `phy_sel` block word (`0x110`+`0x34`) the stock PHY driver writes |
| APMU | `0x14c` | `0xb8002804` | `0xb980380d` | a `phy_sel` block word (`0x110`+`0x3c`) |
| APMU | `0x3c0` | `0x3ef1` | `0x3ff1` | bit 8, beside the USB glue word |
| APMU | `0x3c4` | `0x8000` | `0x208000` | bit 21, beside the USB glue word |
| MPMU | `0x014` | `0x1fbd0600` | `0x007d0018` | the PMU's USB clock control word by its lineage; the stock sets bits 3 and 4 |
| MPMU | `0x030`, `0x048` | `0` | `0x180000f0`, `0x177000f0` | unknown (PLL control by the look); not touched |

The rest: EMAC/PCIe/display/i2s words the stock desktop runs and we do not need.

## Decision (cycle 9)

Each candidate becomes a masked glue word on the PHY node it belongs to — `0x14` of the MPMU
on the USB 2.0 PHY node (whole word, stock `0x007d0018`), `0x3c0` bit 8, `0x3c4` bit 21,
`0x144`, `0x14c` on the SuperSpeed PHY node — brought up through socd before xhcid touches
the PHYs (non-fatal: the marker's before/after words say which took, the PHY measurements and
the controller follow either way).

### Gates for cycle 9

| Line | Expected | Decides |
|---|---|---|
| `socd: bring-up /soc/storage-bus/phy@c0a30000 … mpmu+14:1fbd0600>7d0018` | the word takes the stock value (`FAIL (step=read-back …)` names the bits that do not) | whether `0x14` is a control word |
| `socd: bring-up /soc/storage-bus/phy@c0b10000 ok (… writes=4)` | `3c0`, `3c4`, `144`, `14c` read back | the `phy_sel` words are ours |
| `xhcid: usb2 phy after 20 ms (… 0x38=0x8011)` | the PLL locks | the USB 2.0 PHY runs |
| `xhcid: ss phy (stock words N/23 …)`, `N > 0` | the block answers | the SuperSpeed PHY is clocked |
| `xhcid: controller ok (version=1.10 …)` | HCRST completes | the ladder continues to the hub and the devices |

## Cycle 9 (`board-boot-2026-10-05-usb-cycle9.txt`) — against its gates

| Gate | Read | Verdict |
|---|---|---|
| `mpmu+14:1fbd0600>7d0018` | took, read back (`writes=1`) | a control word — but the PHYs stay dead with it |
| `phy@c0b10000 ok (writes=4)` | `FAIL (step=read-back reg=apmu+0x3c0 val=0x3ef1)` | **bit 8 of `0x3c0` is status** (a write does not take); the plan stopped there — `0x3c4`, `0x144`, `0x14c` were never written |
| `usb2 phy after 20 ms … 0x38=0x8011` | `0x38=0x0` | the PLL still does not lock |
| `ss phy N > 0` | `0/23`, all zero | the block still dead |
| `controller ok` | `stuck (phase=3 …)` | HCRST still waits |

A bring-up stops at its first fault (as it must), so a plan of unknown words measures one word
per cycle. The next step reads the vendor drivers as a reference for the register sequence
(RFC-0099: register names only, nothing ported) instead of guessing word by word.

## Decision (cycle 10 — from the vendor drivers as a reference, against the stock dumps)

The vendor USB 2.0 PHY, combo PHY, clock and reset drivers (reference only; register names
and bit positions transcribed, no line ported) read against our dumps:

- **The SuperSpeed (combo) PHY is held in reset by our loader.** The PHY shares PCIe port A's
  global reset: bit 8 of the APMU's `0x3cc`, and that reset's polarity is inverted against
  every other APMU reset (SET = asserted). The stock word is `0x480` (bit 8 clear — released);
  cycle 8 measured ours as `0x40000780`: bit 8 set (held) and bit 30 set (the port's "hold PHY
  reset", which the vendor PHY driver clears before calibrating). A block in reset reads all
  zeros — exactly cycles 6–9's `ss phy 0/23`. The DWC3's xHCI reset waits on this PHY's PIPE
  clock, so HCRST hangs. Fix: the combo PHY node carries `resets = <RESET_PCIE0_GLOBAL>` (the
  table's one inverted-polarity APMU reset) and the glue words `0x3cc` bit 30 → 0 and `0x110`
  bit 3 → 1 (the lane select, kept). The four guessed words (`0x3c0` bit 8 status, `0x3c4`,
  `0x144`, `0x14c`) go: the vendor drivers never touch them.
- **The combo PHY's USB-mode PLL sequence** (`ss_phy.rs`): the test word `0x68` zeroed, the
  internal timer field of `0x08` set to USB (bits 10:7 = 2), `0x48`'s spread-spectrum depth
  (bits 19:16 = 0xa), 24 MHz reference (bits 15:13 = 1) and no 100 MHz reference (bit 12
  clear), the software init-done bit (`0x08` bit 11), then the PLL's lock (`0x08` bit 0) polled
  up to 500 ms on xhcid's one-shot. The stock words agree with that sequence (`0x08 = 0x97d`,
  `0x48 = 0x603a2276`). Calibration (`0x84` bit 10) is measured, not run: it needs PCIe port
  A's application clocks (now in the table, not in the tree) — a `calibrated=0` in the marker
  names cycle 11's step.
- **The USB 2.0 PHY was never the blocker**: its PLL-ready bit (`0x04` bit 0) read set under
  our chain from cycle 3 on (`0x60e9`), and the words we set are the ones the vendor init
  writes (`0x04 = 0x60ee` + ready, `0x34 = 0x1c`, `0x10` bit 2). The vendor order is now
  followed — the PLL divider word `0x98` (`0xbec4`, the stock value) first, the lock waited
  for, then the rest — and the stock dump's remaining words (`0x84`…`0xa4`) are compared,
  never written. `0x38` stays a status word nobody writes.
- **MPMU `0x14`** stays as a stock word (it took, no harm); the vendor drivers do not name it.

### Gates for cycle 10

| Line | Expected | Decides |
|---|---|---|
| `socd: bring-up /soc/storage-bus/phy@c0b10000 ok (… resets=1 words=2 writes=3)` | bit 8 and bit 30 of `0x3cc` cleared, read back; `0x110` = 8 | the PHY is released |
| `xhcid: ss phy (stock words N/23 …)`, `N > 0` | the block answers (not all zeros) | the reset was the gate |
| `xhcid: ss phy pll ready (after T ms calibrated=C writes=W …)` | the PLL locks within 500 ms; `C` says whether the loader left the PHY calibrated | the PIPE clock runs; `C=0` → cycle 11 calibrates on the PCIe0 clocks |
| `xhcid: usb2 phy set (writes=N read back, pll wait 0 ms)` | the USB 2.0 PLL is locked before the mode word | the USB 2.0 PHY runs |
| `xhcid: controller ok (version=1.10 …)` | HCRST completes | the ladder continues to the hub and the devices |

## Cycle 10 (`board-boot-2026-10-05-usb-cycle10.txt`) — USB works on the board

| Gate | Read | Verdict |
|---|---|---|
| `phy@c0b10000 ok (… writes=3)` | `resets=1 words=2 writes=3 apmu+3cc:40000780>680 apmu+110:0>8` | the PHY released |
| `ss phy N/23 > 0` | `16/23 match` (`0x8=0x478 … 0x84=0x8437`) | the block answers: the reset was the gate |
| `ss phy pll ready` | `after 5 ms calibrated=1 writes=4 cfg=0x979` | the PIPE clock runs; the loader leaves the PHY calibrated |
| `usb2 phy set … pll wait 0 ms` | `writes=4 read back, pll wait 0 ms` | the USB 2.0 PLL was locked |
| `controller ok` | `version=1.10 ports=2 slots=64 csz=64 scratch=1 irq=125`, `ready (ports=2 connected=1)` | **HCRST completes** |
| the devices | hub `2109:2817` (5 ports, ttt=3), receiver `046d:c53f` (slot 2, route 0x3: keyboard if 0 mps 12, mouse if 1 mps 32), the hub's companion `2109:8817` (class 17, slot 3), keyboard `3434:0123` (slot 4, route 0x2); `hidrawd: usb hid device` ×3; `hidrawd: usb hid report seen` | the operator moved the mouse and typed: both work |

Two findings, measured in the same trace:

- **The ladder's rungs pinned the enumeration order** (`slot=2 route=0x2 … 3434:0123`): the
  receiver answered first this boot, so the keyboard took slot 4 — the judge failed at rung 33
  on an otherwise green boot. The marker lines now lead with the device's identity
  (`vid= pid= class= speed=`), slot and route trail, and the rungs stop before them.
- **Reports are lost between the device and hidrawd**: during a second of movement hidrawd
  counted `rx hz=169 ev hz=325` (at most ~325 reports/s), while the stock capture of the same
  mouse (`../2026-10-04-usb-boot-protocol/`) shows ~1000 reports/s with a 1.0 ms median gap.
  hidrawd's `send_fail=0`, inputd's `push hz=159`: nothing between hidrawd and windowd drops.
  The pointer position is absolute from inputd on, so lost reports are lost distance — the
  operator's "the mouse moves very slowly". Not measured yet: whether xhcid's four queued TRBs
  per pipe run dry before the requeue (the device goes unpolled), or the device reports less.
- The screen: `windowd: loop hz=14 apply=9 present=8`, `gpud: dc first present (exec_us=29690
  clean_us=871 damage=1920x1080)`. On the controller path the cursor is a software sprite
  blended in every present (`CURSOR_REPLY_SW`, D4 step 2), so every pointer move is a present
  through the CPU executor — the cursor fast path (`OP_MOVE_CURSOR` on a hardware overlay, as
  on the virtio cursor queue) is bypassed. A display package, not USB: the controller's
  composer has per-layer rectangles, blend mode and alpha (`../2026-09-29-display-regs/
  dpu-regs-on-annotated.txt`: `m_nl7_rect_*`, `m_nl7_blend_mode=1`, `m_nl7_layer_alpha`), i.e.
  a cursor layer.

## Decision (cycle 11 — measure where the reports go)

xhcid prints its one-second counters under traffic (RFC-0099 Phase 3: `xhcid: irq hz=
events hz= reports hz= per_irq_max= dry= dropped= refused=`); `dry` counts drains in which an
interrupt pipe's every queued TRB had completed (the ring ran dry: the device went unpolled
until the requeue). Nothing else about the pipes changes, so the line measures today's loss.

### Gates for cycle 11

| Line | Expected | Decides |
|---|---|---|
| `[PASS] board-visible` with the identity rungs | 43 rungs, whatever order the ports answer in | the ladder is order-free |
| `xhcid: irq hz=… reports hz=R dry=D` during movement | `R` against hidrawd's `ev hz` (the same second) | where the reports go: `R ≈ 1000` and hidrawd low → the class path; `R ≈ 300` and `D > 0` → the ring runs dry (deeper queue / the requeue latency); `R ≈ 300` and `D = 0` → the device reports less than the stock capture (the TT, the interval) |
| `per_irq_max` | ≤ 4 with `D = 0`; = 4 with `D > 0` | consistency of the two counters |

## Cycle 11 (`board-boot-2026-10-05-usb-cycle11.txt`) — the ring runs dry at a ~6 ms wake interval

| Gate | Read | Verdict |
|---|---|---|
| `[PASS] board-visible` with the identity rungs | the receiver again first (`046d:c53f` slot 2 route 0x3, `3434:0123` slot 4 route 0x2); every identity rung present | the ladder is order-free (the operator did not ack this cycle: judged on the rungs) |
| `xhcid: irq hz=… dry=D` during movement | `irq hz=162 events hz=401 reports hz=401 per_irq_max=4 dry=44 dropped=0 refused=0`; hidrawd the same second `rx hz=162 ev hz=401`; inputd `push hz=149`; windowd `loop hz=150 apply=149 present=8` | **the ring runs dry**: 162 drains a second — the driver wakes every ~6 ms although events wait — and in 44 of them the pipe's four TRBs had all completed (reports lost at the device). Nothing between xhcid and windowd drops. |
| `per_irq_max` | 4 = the pipe's depth | consistent |

Reading: the controller moderated at 1 ms (`IMODI = 4000`) cannot explain a 6 ms interval, and
the drain's own cost is unmeasured; either the moderation behaves otherwise on this controller
or the kernel's wake path (an interrupt that arrives in S-mode is stashed and delivered by the
idle loop or the next trap with a user context) takes that long. The idle windowd loop ran at
2 kHz in the same boot, so the harts are not busy.

## Decision (cycle 12 — the pointer's layer, and the wake interval measured)

- **USB**: eight TRBs per pipe (the ~6 ms interval covered: `dry` should read 0 and the report
  rate the device's), `IMODI = 0` (no moderation), and the drain's own cost in the line
  (`drain_us avg= max=`). `irq hz ≈ reports hz` → the moderation set the interval (keep 0 or a
  measured value); `irq hz` still ~160 with `drain_us` small → the kernel's wake path (a kernel
  measurement next: the stash-to-deliver latency).
- **The pointer** (TASK-0251 P2a step 3c, `../2026-10-05-cursor-layer/`): the controller path
  arms the pointer's layer at windowd's upload and answers `CURSOR_REPLY_HW`; a move is the
  layer's rectangle + config-ready; the software pointer on this path is deleted (a
  `BlendCursor` in a present is refused and named).

### Gates for cycle 12

| Line | Expected | Decides |
|---|---|---|
| `gpud: dc cursor layer ok (rdma=2 layer=6 fmt=0x4 blend=5 sprite=32x32 hot=… readback N/N)` | every word read back | the layer's words take |
| `windowd: hw cursor on` | windowd on the overlay path | no present per move |
| the arrow on the monitor follows the mouse at once; `board-visual: pointer` | the operator's eye | the rectangle's packing (`left << 8`) and the latch (config-ready alone) are right; else the arrow sits wrong or does not move |
| `windowd: loop hz=… present=P` under movement | `P` stays at the content's rate, not the pointer's | the fast path |
| `xhcid: irq hz=I reports hz=R dry=D drain_us avg=A max=M` | `D = 0`; `I` vs `R` as above | where the 6 ms come from |
| `gpud: dc refused BlendCursor` | absent | windowd sent no software pointer |

## Cycle 12 (`board-boot-2026-10-05-usb-cycle12.txt`) — the loss is gone, the pointer is not there

| Gate | Read | Verdict |
|---|---|---|
| `xhcid: irq hz=I reports hz=R dry=D drain_us` | `irq hz=193 events hz=395 reports hz=395 per_irq_max=8 dry=1 drain_us avg=126 max=18228`; also `irq hz=233 events hz=235` and `irq hz=100 events hz=172 dry=1` | **the loss is gone** (`dry` 0–1 with eight TRBs). `IMODI = 0` did not raise the interrupt rate to the event rate (193 drains for 395 events): the ~5 ms wake interval is the kernel's, not the moderation's. `drain_us max` ≈ 18 ms coincides with the line's own print (the console write); under movement the drain costs 15–130 µs. |
| the arrow follows at once | **the pointer vanished**; the mouse "minimally faster" | see below |
| `gpud: dc cursor layer ok (…)` | first: `FAIL dc cursor layer (readback 9 of 24: d4c=20400c/200000 d60=1/0 … d78=1/0)`; second (at the first shape change): `ok (rdma=2 layer=6 fmt=0x4 blend=5 sprite=32x32 hot=16,16 readback 24/24)`, `windowd: hw cursor on` | the two word groups beyond the desktop channel's set (`0xcc`, `0xe0..=0xfc`) did not take before the channel ran — state, not glue (the desktop's channel holds neither). After the failed first arming windowd blended its software pointer and **every present carrying it was refused whole** (`gpud: dc refused BlendCursor` ×40): the desktop stopped with the pointer. Once armed (24/24), the layer showed nothing: the rectangle's packing, the latch (config-ready alone, where the proven plane switch also issues the software start) or the channel's words are still open. |
| `windowd: loop hz=… present=P` | no `loop hz` line under movement | — |
| hidrawd `wire_skip`, `send_fail` | 0, 0 | no loss between xhcid and inputd |

Reading the pointer's whole way (the operator: "understand first"): a report leaves the device
every 1 ms (xhcid's TRB, `Note::Report`) → hidrawd parses the 4-byte boot report (`dx`, `dy`
as signed bytes) into `HidEvent`s and sends one wire batch per xhcid frame → inputd sums the
batch's `dx`/`dy`, runs the acceleration (identity below its threshold) and adds them to its
absolute display position (`pointer_state.apply_relative`: `x += dx`), then pushes the visible
state (absolute pointer) to windowd → windowd keeps the newest position per frame and, on the
overlay path, sends `OP_MOVE_CURSOR` (no present); on the software path it queues a cursor
damage rect and blends the sprite in the present. Nothing on this way scales or drops a delta
once the TRBs no longer run dry; what the operator still saw as slow is therefore either the
reports' counts (the device's own, unmeasured on this boot) or the eye without a pointer.

## Decision (cycle 13)

- The two state word groups are no longer written; the arming latches like the proven plane
  switch (config-ready + software start), a move keeps config-ready alone; after the arming
  gpud prints the pointer channel's, the composer's and the control words (`gpud: dc census
  pointer+0x…`) — the witness against the stock dump when the eye sees nothing.
- A `BlendCursor` in a present is skipped (said once, counted), the present's other commands
  land: a failed arming no longer stops the desktop.
- inputd prints the relative travel per second (`inputd: hid rx hz=… travel dx=Σ|dx| dy=Σ|dy|
  rel=batches`): the distance the pointer was given, against the operator's hand.

### Gates for cycle 13

| Line | Expected | Decides |
|---|---|---|
| `gpud: dc cursor layer ok (… readback N/N)` at the FIRST upload | no FAIL before it | the glue words take before the channel runs |
| the arrow on the monitor | follows at once | the rectangle/latch/blend hypotheses; else the census vs. the stock dump names the word |
| `gpud: dc census pointer+0xc80: …`, `+0x4c00: …`, `+0x500: …` | channel 2 words as the stock dump's (`cbc=400040 cf0=4 cf8=10000008 …`), layer 6 block (`4cf8=5 4d08=… 4d0c=… 4d10=… 4d14=…`), `560=40006` | which word the controller holds otherwise |
| `inputd: … travel dx= dy=` during a known hand movement (e.g. 10 cm left to right on the pad) | thousands of counts per second at a 1000-cpi mouse | whether the distance given to the pointer matches the hand |
| `gpud: dc refused BlendCursor` | absent | windowd on the overlay path from the first upload |

## Cycle 13 (`board-boot-2026-10-05-usb-cycle13.txt`) — USB at 1 kHz, the pointer under the desktop

| Gate | Read | Verdict |
|---|---|---|
| `gpud: dc cursor layer ok` at the first upload | `ok (rdma=2 layer=6 fmt=0x4 blend=5 sprite=32x32 hot=2,2 readback 16/16)`, `windowd: hw cursor on`; one `refused BlendCursor` before it (the first present, skipped) | the glue takes before the channel runs |
| the arrow | **no pointer** | see the census |
| `gpud: dc census pointer+0xc80` | `0=1e203c 20=4cb8000 38=100 3c=400040 44=3f003f 70=4 78=10000008 … cc=20400c e0..fc=1` | the channel runs (its state words appear as in the stock dump) |
| `gpud: dc census pointer+0x4c00` | `38=3 4c=77f0000 50=437 54=ff0000` (our desktop on **layer 0**), `f8=5 10c=3f0000 110=5003f 114=ff000a` (the pointer on layer 6) | **the desktop is on layer 0, the pointer on layer 6, and a lower index composites above**: the stock runs the desktop on layer 7 under the pointer's 6. Our pointer lies under the desktop. |
| `xhcid: irq hz=I reports hz=R dry=D` | `irq hz=915 events hz=953 reports hz=953 per_irq_max=8 dry=1 drain_us avg=14 max=26`; `563/563`, `546/748 dry=4` | **the controller's full rate reaches xhcid** (~1 kHz, one event per interrupt at `IMODI = 0`); cycle 12's ~5 ms interval is not a fixed property of the wake path (it had the console busy with the first long lines, by the look of it) — `IMODI = 0` stays. |
| `inputd: … travel dx= dy=` | `travel dx=1622 dy=1125 rel=903` (a second at 903 reports), `dx=536 dy=432 rel=592` | the pointer gets the device's counts whole (~1.8 per report, ~1600 px/s at a slow hand): what the operator called "very wild" is the native 1000-cpi distance without the stock's pointer acceleration — a scaling question, no loss |

## Decision (cycle 14)

The desktop moves to composer layer 7 as the stock kernel runs it (`0x4d18 = 0x3`, blend mode
1, `0x4d34 = 0x00ff0037`) and the loader's layer 0 is switched off at the bring-up; the pointer
stays on layer 6, above it. The first-light goldens follow the stock words.

### Gates for cycle 14

| Line | Expected | Decides |
|---|---|---|
| `gpud: dc scanout ok`, `gpud: dc reveal flip ok`, `windowd: desktop revealed` | as before | the desktop on layer 7 scans |
| the arrow on the monitor | follows the mouse at once | the z-order was the gate |
| `gpud: dc census pointer+0x4c00` | `38=0`, `118=3 … 134=ff0037`, `f8=5` | the layers as the stock's |
| the pointer's speed | the operator's judgement | the acceleration/scaling package that follows |

## Cycles 14 and 15 (`board-boot-2026-10-06-usb-cycle14.txt`, `…cycle15.txt`) — the pointer follows; the probe's line

Cycle 14: the operator — "the mouse works really well"; `gpud: dc cursor layer ok (… readback
16/16)` at the first upload, `windowd: hw cursor on`, the census with the desktop on layer 7
(`118=3 … 134=ff0037`) and the pointer on 6; USB at ~1 kHz (`irq hz=929 reports hz=939 dry=1`),
`travel dx=2083 dy=1162` in the fastest second. The ladder's 43 rungs and the three acks
(`desktop`, `typed`, `pointer`) present; two forbidden markers: `SELFTEST: input usb hid FAIL
(no event in 60s)` — the minute ran out during the SD swap — and one `gpud: dc refused
BlendCursor` from windowd's first present (before the overlay answered: the path's start, not
a second path). Two findings noted for the display track: typing presents the whole plane
10–16 times a second and starves xhcid's wakes meanwhile (`dry=29`), and the pointer looks odd
over a hover field (unmeasured).

Cycle 15 (the probe at 180 s, the refusal named only once the layer is armed): 44 rungs, no
forbidden marker — and **no probe line at all**, although `SELFTEST: ui v2 input ok` proved
the route live. Found in the kernel: `debug_putc` writes the raw UART only, `debug_write` the
console funnel the eMMC trace records; the `ok` line was emitted byte by byte (`emit_bytes`,
`emit_u64`) and so was on no trace, while cycle 14's `FAIL` line, emitted whole, was. The probe
builds its line whole now (and from the generated marker constants: the selftest-client's
architecture gate). Cycle 16 decides `[PASS] board-visible`.

Cycle 16 (`board-boot-2026-10-06-usb-cycle16.txt`): still no probe line — and the reason was
never the emission: the topology declares **no selftest-client → windowd route**
(`route_matches(SelftestClient, Windowd)` is false by the topology's own test), so the probe's
`interactive_live_tick()` returned nothing on every poll, on every machine; it waited its 180 s
until the operator reset. Meanwhile the trace held `inputd: live pointer route on` (1404) and
`inputd: live keyboard route on` (1564) — inputd's own word for exactly the contract's sentence
("a real event reached inputd's live routes"). The probe is retired; the ladder requires
inputd's two markers. Also in this trace: `SELFTEST: frame arena spill FAIL (a frame outgrew
its generation)` ×2 from app-host while typing — TASK-0251's finding 2 (1280x800-era sizes:
the frame arena's generation at 1080p), a known board red to tolerate with its reference.
Cycle 17 decides `[PASS] board-visible`.

Cycle 17 (`board-boot-2026-10-06-usb-cycle17.txt`): **`[PASS] board-headless` (38 rungs)** — the
package's first honest board PASS; `board-visible` short of its last rung only (no key was
pressed this cycle). The four new markers had been declared for `board-visible` alone and
counted as unexpected under `board-headless`: declared for headless now (visible inherits);
inputd's two route markers fire on every lane with live input, so they carry no profile.

Cycle 18 (`board-boot-2026-10-06-usb-cycle18.txt`): both routes live (`inputd: live pointer
route on`, `… keyboard route on`), the pointer's layer armed at the first upload — and a new
red: `xhcid: FAIL (step=control cc=6)`. This boot the USB 3 root port's link trained **before
the run phase** and the hub's SuperSpeed twin (`2109:0817`, `speed=super slot=1 route=0x0`) was
enumerated like a USB 2 device: the port's change arrived before `run` had read the ports'
revisions (the only place that read them), so the "leave the USB 3 port alone" branch did not
fire, and the SuperSpeed hub STALLed the USB 2 hub-descriptor request. The revisions are read
at `start` now (the capability is static), the leave-alone is said once per link, and the model
proves a port that trains before the run is left alone. Cycle 19 decides `[PASS] board-visible`.

Cycle 19 (`board-boot-2026-10-06-usb-cycle19.txt`): `[PASS] board-headless` again, `superspeed
port left alone (port=2)` with no FAIL (the fix holds), the pointer route live — one keyboard
batch arrived and dispatched nothing (no `imed forward`), so the keyboard rung stayed out; the
operator's one key is not logged (privacy) and not known.

**Cycle 20 (`board-boot-2026-10-06-usb-cycle20.txt`) — `[PASS] board-visible`: 46 rungs, the
operator's `desktop` / `typed` / `pointer` acks, no FAIL marker, manifest clean; `[PASS]
board-headless`: 38 rungs.** Block 2's gate. Twenty cycles from the first glue word to the
desk's keyboard and mouse driving the desktop with the pointer on the controller's own layer.
