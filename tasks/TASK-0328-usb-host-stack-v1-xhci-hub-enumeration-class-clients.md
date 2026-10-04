---
title: TASK-0328 USB host stack v1: `nexus-usb` + `xhcid` (xHCI rings, hub, enumeration) with device-class services as clients
status: In Progress (U0 ✅ 2026-10-04 — measured on the stock system (topology, registers, endpoints, the hub, what the devices send in the boot protocol), RFC-0099 seeded, the HID boot parsers fixed to the measured reports, TASK-0253B seeded; U1 next; rewritten to end state at U0; seeded 2026-09-21 by the hardware fast track)
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
first, then on the board. **Block 2 gate:** `SELFTEST: input usb hid ok` + `board-visual: typed`.

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
- **U1 — Host stack on QEMU.**
  - `source/libs/nexus-usb` (no_std, `forbid(unsafe_code)`): descriptor parsing with bounds
    (`test_reject_*` per RFC-0099 §Security), setup packets, the hub class (descriptor, port
    status/feature requests, TT parameters), route strings, the xHCI context/TRB encodings, the
    enumeration and HID-setup state machines — driven by events, host-tested.
  - nexus-driverkit: the shared DMA region (`publish`/`observe` of entry ranges) beside
    `DmaBuffer`, host-tested against the non-coherent cache model (TASK-0246 P2).
  - `source/drivers/usb/xhcid`: the controller (init, rings, interrupter, doorbells), the reactive
    loop (one waitset: IRQ, clients, one-shot timer), root ports, the class dispatch; tested
    against an xHCI behavioural model in the sdhci-model style (commands, events, transfers, port
    changes, a hub, a STALL on EP0).
  - init: the PCI plan's class `0x0c03`/prog-if `0x30` function and the `snps,dwc3` node with
    `dr_mode = "host"` become one `device.mmio.usb` grant (bus mastering after the grant);
    `ServiceId::Xhcid`, its spec and slots in `nexus-service-topology` (`check-slot-ssot.sh`), the
    policy class `device.mmio.usb` and the client class `usb.hid` in `policies/base.toml`.
  - Lane: harness profile `usb` (`qemu-xhci`, a `usb-hub`, `usb-kbd` and `usb-mouse` behind it —
    QEMU's hub is full speed, so TT is the board's to prove), `just ci-os-usb` in `test-all`.
  - Gate: `xhcid: controller ok (…)`, `xhcid: ready (…)`, `xhcid: hub (…)`, two `xhcid: device
    enumerated (…)`, two `xhcid: hid boot interface (…)`; no `xhcid: FAIL`.
- **U2 — HID ingress** (`TASK-0253B`): hidrawd's sources behind one trait, the USB source as
  xhcid's client; QMP input to the USB devices moves the desktop (`SELFTEST: ui v2 input ok` over USB).
- **U3 — The board.** socd brings `usb@c0a00000` up (domain 0, `usbdrd30`, the resets, the glue
  word; the USB 2.0 PHY diffed against the stock words on the first cycle); the tree names the hub's
  supply and reset lines as fixed regulators with enable GPIOs and startup delays, and socd gains
  the GPIO output step that powers them (RFC-0106 amendment); xhcid on the board's window with TT.
  Gates (set before the cycle from this ledger's measurements): `xhcid: hub (… ports=5 ttt=32)`,
  `device enumerated (… vid=3434 pid=0123 …)` and `(… vid=046d pid=c53f …)`, `hidrawd: usb hid
  device (…)`, `SELFTEST: input usb hid ok (…)` — a real report inside a bounded wait, else `FAIL
  (no event in 30s)` — and `board-visual: typed`. **Block 2 gate.**

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
   with `SELFTEST: input usb hid ok` + `board-visual: typed`, no tolerated red without a reference.
4. The gate has been shown red once on a real failure (e.g. a TT field withheld on the board, or
   the old 3-byte mouse parser against the measured reports), with the transcript archived.
5. Docs sweep: RFC-0099 status, ADR-0039, CHANGELOG, `docs/board/bpi-f3.md` (USB section),
   IMPLEMENTATION-ORDER.
