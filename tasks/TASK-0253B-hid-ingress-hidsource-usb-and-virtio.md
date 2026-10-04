---
title: TASK-0253B HID ingress v2: hidrawd's sources behind one trait — virtio-input and USB HID (xhcid's client) — onto the one `WireHidBatch` contract
status: Draft (seeded 2026-10-04 at TASK-0328 U0 — Block 2 U2 of the hardware fast track; the parsers it relies on were fixed to the measured reports at U0)
owner: @runtime
created: 2026-10-04
depends-on:
  - tasks/TASK-0328-usb-host-stack-v1-xhci-hub-enumeration-class-clients.md
follow-up-tasks: []
links:
  - Execution order (Block 2 U2): tasks/IMPLEMENTATION-ORDER.md
  - Contract: docs/rfcs/RFC-0099-usb-host-contract-xhcid-reactive-event-rings.md (§5 the HID class contract)
  - HID contract: docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md, userspace/hid (TASK-0252)
  - Parent: tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md (Done — the virtio-input ingress)
  - Measurement: docs/board/measurements/2026-10-04-usb-boot-protocol/
---

## Origin

B part of `TASK-0253`: hidrawd's ingress was built for virtio-input alone (QEMU). The board's
input is USB; xhcid (TASK-0328) delivers HID boot reports to one class client. This ledger turns
hidrawd's ingress into sources behind one trait so both transports feed the one contract to inputd.

## Context (measured / read 2026-10-04)

- hidrawd's loop (`src/os_lite.rs`) opens up to three virtio-input devices on fixed MMIO slots,
  binds their IRQs to its control endpoint, acks, drains the used rings into evdev-shaped
  `RawInputEvent`s and normalizes them (`adapter.rs`: evdev key codes → HID usages, REL 0/1/8,
  ABS 0/1, BTN ≥ 0x110) into `WireHidBatch` frames (≤ 15 events, 256 bytes) to inputd.
- `userspace/hid`'s boot parsers (`BootKeyboardParser`, `BootMouseParser`) exist and are used only
  by tests today; since U0 they take the reports the desk's devices really send (4-byte mouse
  reports with the wheel, eight buttons; the keyboard's reserved byte and error usages).
- The wire codes already match: HID usages for keys, REL X/Y/Wheel = 0/1/8, BTN 0x110 + bit.

## Goal

One ingress loop over `HidSource`s: the virtio-input source (today's code behind the trait) and
the USB source (xhcid's client per RFC-0099 §5: attach / reports / detach), each producing
`hid::HidEvent`s that the one batch path sends to inputd. The boot parsers are the one
normalization of USB reports; the hardwired virtio-only loop is deleted (no second loop).

## Packages

- **P1 — the trait and the virtio source** (host-first): `src/source.rs` (`HidSource`: wait
  member, drain into events, role, device identity), `src/virtio_source.rs` (today's open/ack/drain
  behind it); the loop generic over sources; the old loop deleted with a retired-name entry.
  Proof: hidrawd host tests; the visible lanes unchanged (input ladder, pixel proof).
- **P2 — the USB source**: subscribe to xhcid (`usb.hid`, policy-gated), a parser per attached
  interface (keyboard/mouse by protocol), reports → events → the batch path; detach drops the
  device's parser and releases its held keys/buttons (key-up/btn-up events, so nothing sticks).
  Marker `hidrawd: usb hid device (vid=… pid=… role=…)`.
- **P3 — the QEMU proof**: the `usb` visible profile (virtio-input absent, `usb-kbd`/`usb-mouse`
  behind a `usb-hub`), QMP `input-send-event` addressed to the USB devices → `SELFTEST: ui v2 input
  ok` over USB.
- **P4 — the board** (with TASK-0328 U3): `SELFTEST: input usb hid ok (…)` — a real report
  inside a bounded wait — and `board-visual: typed`. **Block 2 gate.**

## Constraints / invariants (hard requirements)

- **One contract**: `WireHidBatch` to inputd unchanged; no second ingress loop, no second parser.
- **Untrusted input**: report lengths bounded by the parsers (`test_reject_*`); a refused report is
  counted, never parsed partially.
- **Identity**: xhcid admits hidrawd by `sender_service_id` + policyd (`usb.hid`).
- **No stuck input**: a detached or dropped device releases what it held.

## Definition of Done

1. hidrawd's ingress is one loop over sources; the virtio-only loop is gone (retired-name gate).
2. Host tests: both sources against fakes, detach releases held keys, the measured reports as goldens.
3. QEMU: the `usb` visible lane in `test-all` with `SELFTEST: ui v2 input ok` over USB.
4. Board: `SELFTEST: input usb hid ok` + `board-visual: typed` (with TASK-0328 U3).
5. Docs sweep: RFC-0099 Phase 2, CHANGELOG, IMPLEMENTATION-ORDER.
