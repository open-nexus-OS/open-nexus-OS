---
title: TASK-0328 USB host stack v1: `nexus-usb` + `xhcid` (xHCI rings, hub, enumeration) with device-class services as clients
status: Done (U3 ✅ 2026-10-06 — `[PASS] board-visible` 46 rungs + `board-headless` 38 on the desk board in cycle 20: socd releases the SuperSpeed PHY's inverted-polarity reset, xhcid runs both PHYs' PLL sequences, the DWC3 host mode and the xHCI at ~1 kHz with eight TRBs per pipe, the hub + keyboard + receiver enumerated, inputd's live routes from the operator's mouse and keys, the pointer on the display controller's own layer (ADR-0034 Tier 1b); 20 board cycles, `docs/board/measurements/2026-10-05-usb-cycle1/`; U2 ✅ 2026-10-05 — TASK-0253B P1–P3: xhcid serves the HID boot class to hidrawd (policyd `usb.hid`), the `usb-visible` lane drives the desktop over USB alone; U1 ✅ 2026-10-05 — xhcid + nexus-usb on QEMU: the `usb` lane enumerates a hub, a keyboard and a mouse driven by the interrupt alone; U0 ✅ 2026-10-04 — measured on the stock system (topology, registers, endpoints, the hub, what the devices send in the boot protocol), RFC-0099 seeded, the HID boot parsers fixed to the measured reports, TASK-0253B seeded; U1 next; rewritten to end state at U0; seeded 2026-09-21 by the hardware fast track)
owner: @runtime
created: 2026-09-21
depends-on: []
follow-up-tasks:
  - tasks/TASK-0253B-hid-ingress-hidsource-usb-and-virtio.md
links:
  - Execution order (Block 2): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md, .claude/skills/driver-bringup/
  - Contract: docs/rfcs/RFC-0099-usb-host-contract-xhcid-reactive-event-rings.md
  - Layering: docs/adr/0039-device-class-driver-architecture.md (amended: USB as a bus service)
  - Board truth: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md, docs/rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md
  - MMIO access model: docs/rfcs/RFC-0017-device-mmio-access-model-v1.md
  - Measurements: docs/board/measurements/2026-10-04-usb-topology/, docs/board/measurements/2026-10-04-usb-boot-protocol/
  - HID contract: userspace/hid (TASK-0252), source/services/hidrawd
  - Extracted from: tasks/TRACK-REMOVABLE-STORAGE.md (CAND-REM-030)
---

## Origin

Extracted from `tasks/TRACK-REMOVABLE-STORAGE.md` CAND-REM-030 ("USB mass storage device-class
services (xHCI + MSC + SCSI)") for Block 2 of the hardware fast track: the board's only input is
USB. The HID class client is `TASK-0253B`; mass storage returns to the track's later phases.

## Goal

The desk's USB keyboard and mouse drive the desktop on the board: one service (`xhcid`) owns the
USB host controller and drives it reactively (RFC-0099 — no polling in steady state), enumerates
through hubs, and hands HID boot-protocol reports to `hidrawd` as its IPC client; proven on QEMU
first, then on the board. **Block 2 gate:** `inputd: live pointer route on` / `… keyboard route on` + `board-visual: typed`.

## Non-Goals

USB gadget/device mode (`TASK-0261`, parked), mass storage + SCSI (track phases), isochronous
transfers, SuperSpeed link features (v1 serves HS/FS/LS through the USB 2.0 root port), MSI,
report-protocol HID (a follow-up behind a descriptor parser).

## Context (measured 2026-10-04)

- **The device** (stock tree, `2026-10-04-usb-topology/stock-usb-nodes.dts`): `usb3@0`
  (`spacemit,k1-x-dwc3`, the glue: one APMU word at `0xd4282bc8`, reset `ctl_rst`, clock
  `usbdrd30`, power domain 0, `phys = <&combphy 4>`, `usb-phy = <&usb2phy>`) with its child
  `dwc3@c0a00000` (`snps,dwc3`, `reg 0xc0a00000 + 0x10000`, IRQ 125, `dr_mode = "host"`); the USB
  2.0 PHY at `0xc0a30000`; the combo PHY (SuperSpeed lane) at `0xc0b10000`; `usb3hub@0`
  (`spacemit,usb3-hub`): hub lines GPIO 123/124, VBUS GPIO 97, `vbus_delay_ms = 200`, domain 8.
  Our tree already places `usb@c0a00000` on the `storage-bus` (DMA reach: the first 2 GiB,
  identity — the stock `dram_range@0`); the SoC bus is not coherent.
- **The controller** (`stock-usb-regs.txt`): xHCI 1.10, CAPLENGTH 0x20, 64 slots, 1 interrupter, 2
  root ports (1 = USB 2.0, 2 = USB 3.0 per the Supported Protocol capabilities at 0x890/0x8a0),
  1 scratchpad buffer, **64-byte contexts** (CSZ = 1), 64-bit addressing, port power control,
  extended capabilities at 0x880, DBOFF 0x480, RTSOFF 0x440; the dual-role controller's port
  capability reads host; the glue word `0x0b008000`.
- **The topology** (`lsusb -t`): root port 1 → the on-board hub `2109:2817` (HS, 5 ports) → port
  2 the keyboard `3434:0123`, port 3 the mouse's radio receiver `046d:c53f`, **both full speed** →
  their slot contexts need the **transaction-translator fields**; the hub: TT think time 32 FS bit
  times (TTT = 3), per-port power, bPwrOn2PwrGood 350 ms, alternate setting 0 single TT / 1 multi TT.
- **The boot protocol** (`2026-10-04-usb-boot-protocol/`): all boot interfaces accept
  SET_PROTOCOL(0); the mouse interface STALLs SET_IDLE; mouse reports are **4 bytes** (wheel in
  byte 3, thumb buttons on bits 3–4) at ~1000/s; the first report after the switch is still in the
  report format (9 bytes); the keyboard sends 8 bytes, reserved byte 0, drops keys past six;
  EP0 max packet 32 on the receiver, 64 on the keyboard and the hub; HID interrupt endpoints 1 ms.
- **What our chain does today**: no USB code; init grants no USB device; hidrawd reads only
  virtio-input (QEMU). On the board the desktop shows and nothing moves it.
- **What must not change**: hidrawd's `WireHidBatch` to inputd, the input ladder markers, the
  board tree golden except the U3 nodes, the slot declarations outside the new service's.

## Packages

- **U0 — Paper + measurement ✅ 2026-10-04.** Two measurement folders (above), RFC-0099 (the
  contract: ownership, the reactive loop, the DMA discipline, enumeration through hubs, the HID v1
  class contract, markers), ADR-0039 amended (USB as a bus service), this ledger at end state,
  `TASK-0253B` seeded. **The HID boot parsers fixed to the measured reports** (`userspace/hid`):
  mouse reports of 3..=8 bytes with the wheel in byte 3 and eight buttons (side/extra =
  0x113/0x114); a longer frame (the stale report-format frame) refused; the keyboard's reserved
  byte ignored; the error usages 0x01..=0x03 hold the keys and move the modifiers. The old parser
  would have refused every report of the desk's mouse. Proof: `hid_contract` 10 tests (5 new, the
  measured reports as goldens, `test_reject_mouse_report_protocol_frame_after_the_switch`,
  `test_reject_keyboard_overlong_report`), hidrawd `contract` updated, 7 mutations each killed.
- **U1 — Host stack on QEMU ✅ 2026-10-05** (`[profile.usb]`, `just ci-os-usb` in `test-all`).
  - `source/libs/nexus-usb`: the USB protocol only (descriptors bounded before use — bLength,
    `wTotalLength` ≤ 1024, ≤ 8 interfaces / 16 interface descriptors, ≤ 4 endpoints, EP0 max packet
    8/16/32/64 (512 on USB 3), endpoint max packet 1..=1024 —, setup packets, the hub class,
    speeds, route strings ≤ 5 tiers). 19 tests: the desk's three devices and both hub
    descriptors (the board's and QEMU's) as goldens from the measurement, every refusal by code.
    The xHCI encodings live with the driver (they are the controller's, not USB's).
  - nexus-driverkit `DmaShared`: memory a controller and the CPU share for its lifetime,
    `publish`/`observe` of block-rounded entry ranges (3 unit tests); `dma-model` — the
    non-coherent DRAM model extracted from the SDHCI model (no second copy), now with sub-range
    clean/flush and memory reuse (the newest region at an address wins); sdhci, storage and
    nxboot run on it unchanged.
  - `source/drivers/usb/xhcid`: the core is a pure event-driven state machine (`forbid(unsafe)`,
    no heap): controller bring-up paced by the one-shot (CNR/halt/HCRST/run, re-read at 1 ms,
    bounded), the event ring drained per interrupt (IMAN.IP, ERDP+EHB), one command in flight,
    ONE port reset/addressing at a time (two devices never share the default address), the
    enumeration stages, hubs (descriptor, hub slot fields, port power + power-good, the
    status-change pipe, GET_PORT_STATUS work, hub-port resets re-read every 10 ms — the board's hub
    reports changes only every 256 ms), HID boot interfaces (SET_PROTOCOL, best-effort SET_IDLE,
    four TRBs queued and requeued per report), halted-endpoint recovery (Reset Endpoint + Set TR
    Dequeue Pointer), detach keeping the memory until Disable Slot completes. `os_lite`: one
    waitset (line + one-shot), contiguous DMA objects, stack-formatted markers, an honest park
    without a controller.
  - `xhcid-model`: a behavioural xHCI (registers, the three ring kinds in the non-coherent DRAM
    model, port changes), a hub and HID devices; it refuses what hardware refuses — an unpublished
    DCBAA entry, a speed or TT field that does not match the device, an endpoint the device lacks,
    a wrong interval exponent, a data stage at the wrong EP0 max packet (babble), a full event
    ring. 14 tests: both machines (QEMU's shape; the board's with the measured descriptors, TT,
    the STALLed SET_IDLE), reports end to end (1000 reports wrap every ring), an idle bus costs
    not one register access, detach + re-attach, four `test_reject_*`.
  - init: the PCI plan's xHCI (class 0c03/30) → `device.mmio.usb` to xhcid, bus mastering after
    the grant (`pci::enable_bus_master` generic over any function now); the declared wait
    endpoints are pinned from the spec for every service (`pin_declared_waits`: timers AND the
    interrupt notify endpoint — blkd's bespoke mint deleted); the device grants split out of the
    orchestrator (`bootstrap/device_planes.rs`, orchestrator 803 → 735 LOC, baseline tightened).
    `ServiceId::Xhcid` (32), `slots::xhcid` (windowd's table split out of `slots/mod.rs` at its
    600-line limit), the spec, the supervision tier (standard, restart on failure), policy
    `"xhcid" = ["device.mmio.usb", "ipc.core"]` with `test_reject_a_second_holder_of_the_usb_host`,
    `config/os-services.txt`, the volume list.
  - **The board's tree node is NOT granted in U1**: its window is gated (clocks/resets/power)
    until socd brings it up, and a read of a gated block can stall the bus — the board says
    `usb plane none` until U3 adds the tree path together with the glue.
  - **Measured in the lane** (the gates are the measured values): `qemu-xhci` is xHCI 1.00, 8
    ports with the four USB 3 ports first (the hub on root port 5), 64 slots, 32-byte contexts, no
    scratchpad, its 64-bit BAR at 0x4_0000_0000, INTA on line 33; QEMU's hub `0409:55aa` sends a
    10-byte hub descriptor for 8 ports (`PortPwrCtrlMask` ⌈ports/8⌉, not ⌈(ports+1)/8⌉ — the first
    lane run refused it; fixed in nexus-usb, golden added); its keyboard and mouse are `0627:0001`,
    max packet 8 and 4, bInterval 10.
  - **Found by the mutation run** (16 mutants, 14 killed; the 2 survivors are the electrical waits
    the model cannot express — attach debounce, hub power-good): without the 100 ms debounce the
    board scenario re-enumerated its hub (the scan and the port-power change event queued the same
    root port twice; the debounce had hidden it) → a port is queued once, never while active or
    occupied; and a configuration shorter than it claims was parsed with the buffer's older bytes
    behind it → only the bytes the data stage moved are parsed
    (`test_reject_a_configuration_shorter_than_it_claims`).
  - Gate: `[PASS]` of `just ci-os-usb` with `init: usb host from pci (00:01.0 bar=0x400000000
    irq=33)`, `xhcid: controller ok (version=1.00 ports=8 slots=64 csz=32 scratch=0 irq=33)`,
    `xhcid: ready (ports=8 connected=1)`, the hub (`slot=1 port=5 … ports=8 ttt=0`), both devices
    enumerated and both HID boot interfaces; every other full-ladder lane (smp1, sdhci, reset,
    headless) requires `usb plane none`; a `xhcid: FAIL` fails any lane.
- **U2 — HID ingress** (`TASK-0253B`) ✅ 2026-10-05: hidrawd's sources behind one trait, the USB
  source as xhcid's client; QMP input to the USB devices moves the desktop (`SELFTEST: ui v2 input
  ok` over USB, the `usb-visible` lane). xhcid's side (RFC-0099 §5, amended): a server endpoint in
  its waitset and `@ready` at once (a DisplayReady server on every lane), the pure class server
  `src/hid_class.rs` (owed attaches/detaches, fire-and-forget reports, replay, device names never
  reused; 10 tests over the real core), admission by policyd (`usb.hid` of the kernel-attributed
  sender); the core's notes carry the device identity and a lost HID pipe. Details, proofs and
  findings in `TASK-0253B`.
- **U3 — The board** (built 2026-10-05; the cycle decides). init grants the tree's host-mode node
  (`snps,dwc3` with `dr_mode = "host"`, the board's `usb@c0a00000`) as the USB plane and the
  USB 2.0 PHY's window beside it (`slots::xhcid::PHY`); xhcid reads both nodes from its own tree
  slot and has socd bring them up BEFORE its first register read (a refusal leaves the window
  untouched): the host node — bus domain, the three resets, `usbdrd30`, then the APMU glue word
  (`nexus,glue-words = <&syscon_apmu 0x3c8 0x0b008000>`, a new RFC-0106 step) — and the hub
  node (`/usb-hub`, the stock `usb3hub@0` binding's shape): its three pads on the GPIO function,
  GPIO 123 and 124 driven high as outputs, the measured 200 ms settle on socd's new one-shot,
  then VBUS (GPIO 97) — the GPIO block is a provider kind now, the lines go through the bank's
  masked set registers and read back (RFC-0106 amendment: `GpioOut`, `Settle`, `SetWord`;
  nexus-soc 29 tests incl. the stock board writing nothing, cold driving every line in order,
  `test_reject_*` for a bank/line off the block, a settle too long, a settle without a pause).
  Then xhcid puts the DWC3 in host mode (`dwc3.rs`: PRTCAPDIR, the PHY suspend bits — the stock
  words; refused if the window is no DWC3) and prints the USB 2.0 PHY's words against the stock
  twelve (`phy.rs`; nothing written). Decision rule for the cycle: `usb2 phy (… 12/12 match)` →
  no PHY step exists; fewer → cycle 2 writes the differing stock words (a PHY step, RFC-0099 §6).
  Gates (`scripts/board-test.sh`, from this ledger's measurements and the board rig's values):
  `init: usb host from tree (`, `socd: bring-up /soc/storage-bus/usb@c0a00000 ok (`, `socd:
  bring-up /usb-hub ok (`, `xhcid: soc glue ok`, `xhcid: dwc3 host (`, `xhcid: usb2 phy (`,
  `xhcid: controller ok (version=1.10 ports=2 slots=64 csz=64 scratch=1 irq=125)`, `xhcid: ready
  (ports=2 connected=`, `xhcid: hub (slot=1 port=1 speed=high ports=5 ttt=3)`, the keyboard
  `(slot=2 route=0x2 speed=full vid=3434 pid=0123 class=3)` + its boot interface `(slot=2 if=0
  role=keyboard ep=0x81 mps=8 interval=1)`, the receiver `(slot=3 route=0x3 … vid=046d pid=c53f
  class=3)` + `(slot=3 if=1 role=mouse ep=0x82 mps=32 interval=1)`, `hidrawd: usb hid device
  (vid=3434 pid=0123 role=keyboard)` and `(vid=046d pid=c53f role=mouse)`; board-visible adds
  `inputd: live pointer route on` / `inputd: live keyboard route on` — inputd's own word that a
  real event reached its live routes — and the operator's `board-visual: typed`. **Block 2
  gate.** (A selftest-client probe with a bounded wait was built for this and retired in cycle
  16: it polled windowd's visible state over a route the topology does not declare —
  `route_matches(SelftestClient, Windowd)` is false by test — so it observed nothing on any
  machine; the proof is the service's own marker, as everywhere else on the ladder.)
  Found on the way (the `usb` lane, 2026-10-05): xhcid's state lives on its stack (no heap) —
  21 696 bytes measured (`tests/budget.rs`, now a budget test) — and the move from the bring-up's
  frame into the loop's holds two copies for a moment; 8 stack pages overflowed (`[USER-PF]
  STORE` below the stack, `init: service exit name=xhcid reason=fault`) once the glue's frames
  grew. `stack_pages = 16`, with the measurement beside it (24 since cycle 12: eight TRBs per
  pipe grew the state to 25 816 bytes; the budget test encodes the rule — two copies + 16 KiB
  of frames within the pages).
  **Cycle 1** (2026-10-05, `docs/board/measurements/2026-10-05-usb-cycle1/`): the loader leaves
  the USB clock/reset word at 0 (socd set it to `0xf00`); the glue word reads `0x08008000` and
  bits 24..25 of the stock `0x0b008000` ignore a write (status) → the bring-up failed its
  read-back, xhcid refused the controller, the keyboard did nothing. Fix: the glue word carries
  a mask (`<&apmu 0x3c8 0x8000 0x8000>`, RFC-0106 amended) — bit 15 is the glue and is up after
  the loader. **Cycle 2** repeated cycle 1 (flashed without `just build-os-workspace` — `board-
  image` packs the last OS build, it does not build; trap recorded). **Cycle 3**: glue ok (the
  clock word was 0 after the loader), the hub up (pads 97/124 changed, bank 3 lines 1/27/28
  driven high, read back), the DWC3 in host mode (it came up in device mode), the USB 2.0 PHY
  5/12 words off the stock values (`0x04 = 0x60e9` for `0x60ef`), then `xhcid: FAIL
  (step=controller cc=0)` — a controller wait ran out. **Cycle 4**: four PHY words took the
  stock values, `0x38` is a status word (written `0x8011`, read `0x0`); the controller sticks in
  its reset (`phase=3 usbcmd=0x2 usbsts=0x11`: HCRST set, HCH | CNR) — the xHCI waits on a PHY
  clock. Cycle 5: the PHY words given 20 ms to lock (status read again), the SuperSpeed PHY
  measured against its stock words (`xhcid: ss phy`), the dwc3 driver's core soft reset (core
  + both PHYs, two 100 ms holds on the one-shot) before host mode, `GUSB3PIPECTL` bit 18
  cleared as the stock quirk does. **Cycle 5**: the hub bring-up faulted on a read-back — the
  bank's level word follows the pin late (`0x28294000` right after the set, `0x38294000` a
  moment later); nothing of the controller ran. Cycle 6: the GPIO read-back polls the level
  (bounded), a hub failure no longer stops the controller's bring-up. **Cycle 6**: hub up, PHY
  words set (its status word stays 0), the DWC3 soft reset and host mode as stock — the
  SuperSpeed PHY block reads all zeros (unclocked/in reset) and HCRST still hangs. Cycle 7: the
  USB AXI clock + reset on the host node (stock `0x5c = 0x0f33`), the APMU lane select
  (`0x110 = 0x8`) as the SuperSpeed PHY node's glue word, both through socd. **Cycle 7**: both
  set (0x5c → 0xf03; 0x110 0 → 8), both PHYs still dead (SS block all zeros, USB 2.0 PLL
  status 0), HCRST still hangs — a gate outside the tables. Cycle 8 measures: socd prints every
  APMU/MPMU word that differs from the stock dumps before any bring-up. **Cycle 8** (40 APMU, 21
  MPMU words differ): the USB candidates — MPMU `0x14` (`0x1fbd0600` vs stock `0x007d0018`),
  APMU `0x144`/`0x14c` (the `phy_sel` block), `0x3c0` bit 8, `0x3c4` bit 21. Cycle 9 sets them as
  glue words on the PHY nodes (before/after in the marker) and measures the PHYs again. **Cycle
  9**: MPMU `0x14` took the stock value (no effect on the PHYs); `0x3c0` bit 8 is status (the
  plan stopped there, the other three words unmeasured); HCRST still hangs. Next: the vendor
  PHY/glue drivers as a reference for the sequence (register names only). **Reading** (cycle 10's
  decision, `docs/board/measurements/2026-10-05-usb-cycle1/README.md`): the SuperSpeed PHY
  shares PCIe port A's global reset — bit 8 of APMU `0x3cc`, SET = held, the one APMU reset of
  inverted polarity — and our loader leaves it set with the port's hold-PHY-reset bit (bit 30);
  a block in reset reads zeros, and the xHCI's reset waits on its PIPE clock. Cycle 10: the
  combo PHY node names that reset (`RESET_PCIE0_GLOBAL`, the table knows the polarity) and the
  two real glue words (bit 30 → 0, the lane select); the guessed words go; xhcid runs the PHY's
  USB-mode PLL sequence (`ss_phy.rs`: test word, timer, 24 MHz reference, init-done, the lock
  polled ≤ 500 ms, calibration measured) and the USB 2.0 PHY's words in the vendor init's order
  (divider `0x98` first, the lock waited for). Gates in the README; `xhcid: ss phy pll ready (`
  and the controller's `ok` decide. **Cycle 10: USB works on the board** — the PHY released
  (`0x3cc 0x40000780>0x680`), `ss phy pll ready (after 5 ms calibrated=1)`, `controller ok`,
  hub + receiver + keyboard enumerated, hidrawd names three HID devices, the operator moved the
  mouse and typed. Two findings: the rungs pinned the enumeration order (the receiver answered
  first; the markers now lead with `vid= pid=`, the rungs stop before slot/route), and reports
  are lost before hidrawd (≤ 325/s against the stock capture's ~1000/s; nothing drops between
  hidrawd and windowd — the pointer position is absolute from inputd on) — the mouse "moves
  very slowly". Cycle 11 measures with xhcid's one-second counters (`xhcid: irq hz= … dry=`,
  RFC-0099 Phase 3). The screen's part (every pointer move a CPU present, `windowd: loop hz=14`)
  is the controller path's software cursor — the cursor fast path (`OP_MOVE_CURSOR` on an
  overlay) bypassed in D4 step 2: a display package of its own (TASK-0251 P2a step 3c, the
  controller's cursor layer) — pulled INTO this lane by the operator ("no parallel
  implementations; the fast path by the end of the USB lane"). **Cycle 11** (the counters):
  `irq hz=162 events hz=401 reports hz=401 per_irq_max=4 dry=44` during movement — the driver
  wakes every ~6 ms with events pending and the four TRBs run dry 44 times a second (reports
  lost at the device); hidrawd/inputd/windowd take everything that arrives. Cycle 12: eight TRBs
  per pipe (`stack_pages` 16 → 24, the budget test follows), `IMODI = 0`, the drain's cost in the
  line (`drain_us avg= max=`) — `irq hz` against `reports hz` says whether the moderation or the
  kernel's wake path (an S-mode interrupt is stashed until the idle loop or a user-context trap)
  sets the interval; and the pointer's layer on the controller (3c) in the same image. Gates in
  the README (cycle 12). **Cycle 12**: the loss is gone (`dry` 0–1, `reports hz` up to 395);
  `IMODI = 0` did not raise the interrupt rate to the event rate — the ~5 ms wake interval is
  the kernel's (a kernel measurement is its own package); the pointer vanished: the first
  arming failed on two word groups that are channel state, not glue, windowd blended its
  software pointer and the controller path refused those presents whole; once armed the layer
  showed nothing (packing/latch/channel words open). The pointer's whole way is read and
  written down in the README (device → xhcid → hidrawd → inputd's absolute position → windowd →
  `OP_MOVE_CURSOR` / software blend): nothing scales or drops a delta. Cycle 13: the state
  words dropped, the arming latched like the plane switch, a `BlendCursor` skipped not fatal,
  a pointer census after the arming, and inputd's per-second travel (`travel dx= dy=`) as the
  distance witness. **Cycle 13**: USB at the controller's full rate (`irq hz=915 reports hz=953
  dry=1`, `IMODI = 0` kept), travel = the device's whole counts (the operator: "very wild" — a
  scaling question for the pointer package, no loss); the layer armed at the first upload and
  the census showed why it stayed invisible: the desktop on composer layer 0 above the pointer's
  layer 6 (a lower index composites above; the stock desktop is layer 7). Cycle 14: the desktop
  on layer 7 with the stock's words, layer 0 off — **the pointer follows at once ("works really
  well")**; two forbidden markers left (the probe's minute ran out during the SD swap → 180 s;
  the first present's software pointer named as a refusal → named only once the layer is
  armed). Cycle 15: 44 rungs, no forbidden marker, and no probe line at all: the kernel's
  `debug_putc` writes the raw UART only, `debug_write` the console funnel the eMMC trace records
  — the probe's `ok` line was emitted byte by byte. **Follow-up (board proof hygiene, TASK-0327B
  P4's successor)**: 118 more `emit_bytes(` sites in the selftest-client emit markers byte-wise
  and are invisible on a desk without a UART adapter; the board profile needs them whole.
  **Cycle 16**: still no probe line, and the root cause was the probe itself — the topology
  declares no selftest-client → windowd route, so it could never observe anything; retired, the
  ladder takes inputd's own `live … route on` markers. **Cycle 17: `[PASS] board-headless`**
  (38 rungs). **Cycle 18**: both routes live; a USB 3 root port that trained before the run
  phase was enumerated as a USB 2 device (the revisions were read only in `run`; the
  SuperSpeed hub STALLed the hub-descriptor request → `FAIL (step=control cc=6)`) — the
  revisions are read at `start`, the leave-alone said once per link, modelled. Cycle 19
  decides `[PASS] board-visible`.

## Constraints / invariants (hard requirements)

- **No fake success**: markers only after the behaviour (RFC-0099 §Constraints); a human-visible
  board check is an operator-acked `board-visual:` marker, never prose.
- **No polling in steady state**: one waitset; bounded waits as kernel timers (`check-wait-not-poll.sh`).
- **The FDT/PCI plan is the one hardware truth**: no address, IRQ or size of a controller in code.
- **Untrusted input bounded before parsing**: descriptors, report lengths, residuals (`test_reject_*`).
- **Privacy**: a measurement never archives what a person typed (the boot-protocol folder keeps
  aggregates only); device strings are never read in v1.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` except the one
  MMIO/DMA seam (nexus-driverkit).

## Red flags / decision points

- **RED**: TT on the board cannot be proven on QEMU (its hub is full speed) — the U3 cycle's gates
  name the TT fields explicitly so a wrong field shows as a missing `device enumerated` with the
  completion code, not as silence.
- **YELLOW**: the hub's power lines need a GPIO step in socd (none exists) — RFC-0106 amendment at U3.
- **YELLOW**: the receiver STALLs SET_IDLE — the EP0 stall recovery must be in U1's model tests.
- **GREEN**: DMA reach and alignment come from the kernel's contiguous objects (≤ 64 KiB blocks are
  aligned to their size; no ring crosses 64 KiB).

## Definition of Done

1. The measurement folders exist and answer their questions with archived raw data
   (`2026-10-04-usb-topology/`, `2026-10-04-usb-boot-protocol/`). ✅
2. The host model reproduces the measured behaviour: the xHCI model enumerates a hub with two
   full-speed HID devices (TT fields as measured), recovers a STALL on EP0, and the HID parsers take
   the measured reports (goldens).
3. The lanes are green on their REQUIRED ladders: QEMU `usb` (in `test-all`) and `board-visible`
   with `inputd: live pointer route on` + `board-visual: typed`, no tolerated red without a reference.
4. The gate has been shown red once on a real failure (e.g. a TT field withheld on the board, or
   the old 3-byte mouse parser against the measured reports), with the transcript archived.
5. Docs sweep: RFC-0099 status, ADR-0039, CHANGELOG, `docs/board/bpi-f3.md` (USB section),
   IMPLEMENTATION-ORDER.
