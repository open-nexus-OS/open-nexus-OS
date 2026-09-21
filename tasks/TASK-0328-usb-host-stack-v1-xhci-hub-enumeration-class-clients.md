---
title: TASK-0328 USB host stack v1: `nexus-usb` + `xhcid` (xHCI rings, hub, enumeration) with device-class services as clients
status: Draft (seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; rewritten to end state at its block's P0)
owner: @runtime
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Extracted from: tasks/TRACK-REMOVABLE-STORAGE.md (CAND-REM-030)
  - Device-class layering: docs/adr/0039-device-class-driver-architecture.md
  - MMIO access model: docs/rfcs/RFC-0017-device-mmio-access-model-v1.md
  - Driver kit: source/libs/nexus-driverkit, source/libs/nexus-hal
  - HID contract: userspace/hid (TASK-0252), source/services/hidrawd
  - RFC seed (at P0): RFC-0099 USB host contract
---

## Origin

Extracted from `tasks/TRACK-REMOVABLE-STORAGE.md` CAND-REM-030 ("USB mass storage device-class services (xHCI + MSC + SCSI)") for Block 2 of the hardware fast track; the HID half is `TASK-0253B` (seeded at P0), mass storage returns to the track's later phases.

## Context

Input on the board is USB (keyboard/mouse on the DWC3/xHCI ports); the OS has no USB code at all. `userspace/hid` already holds the transport-neutral USB-HID boot parser (TASK-0252) and `hidrawd` produces `hid::HidEvent` from virtio-input — the contract exists, the transport does not. QEMU can prove the stack first (`-device qemu-xhci -device usb-kbd -device usb-mouse`).

## Goal

A driver-kit USB host service (`xhcid`) owning the controller (MMIO + IRQ from the FDT, DMA via `DmaBuffer`), a `nexus-usb` library (descriptors, control/interrupt pipes, hub class, class dispatch over IPC), and the layering of ADR-0039: device-class drivers are clients of the controller service. Gate: `xhcid: ready (ports=…)`, `xhcid: device enumerated (vid=… pid=… class=hid)` on the QEMU `ci-os-usb` profile and on the board.

## Non-Goals

USB gadget/device mode (the recovery fastboot gadget is `TASK-0261`, parked), mass storage + SCSI (track phases), isochronous transfers (audio/video), USB3 link features beyond enumeration.

## Packages (from the order file; the P0 rewrite fixes them)

- **U0 Paper** — this ledger + `TASK-0253B` to end state; RFC-0099; measure R8 (DWC3/xHCI PHY init, USB3 vs USB2 fallback).
- **U1 Host stack** — `source/libs/nexus-usb`, `source/drivers/usb/xhcid` (host-tested against a mock `Bus`), DWC3 host-mode glue via `nexus-soc`, policy class `device.mmio.usb`, slots + grants + `check-slot-ssot.sh`. Gate: QEMU `ci-os-usb` enumerates keyboard + mouse; board same ladder.
- **U2 HID ingress** (`TASK-0253B`) — `hidrawd` ingress becomes a `HidSource` (virtio-input on virt, USB HID via `xhcid`); one contract, two transports; the virtio-only ingress path deleted. Gate: `SELFTEST: ui v2 input ok` via USB on QEMU; board `SELFTEST: input usb hid ok` + `board-visual: typed`.

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite.
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
