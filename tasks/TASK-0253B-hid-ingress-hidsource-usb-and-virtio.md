---
title: TASK-0253B HID ingress v2: hidrawd's sources behind one trait — virtio-input and USB HID (xhcid's client) — onto the one `WireHidBatch` contract
status: Done (P4 ✅ 2026-10-06 — the board: `inputd: live pointer route on` / `… keyboard route on` from the operator's USB mouse and keyboard, `[PASS] board-visible` in TASK-0328 U3 cycle 20; P1–P3 ✅ 2026-10-05 — TASK-0328 U2: hidrawd is one loop over its sources, xhcid serves the HID boot class to it, and the `usb-visible` lane drives the desktop over USB alone (`SELFTEST: ui v2 input ok`); P4 rides on TASK-0328 U3 (the board); seeded 2026-10-04 at TASK-0328 U0 — the parsers it relies on were fixed to the measured reports there)
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

- **P1 — the trait and the virtio source** ✅ 2026-10-05: `src/source.rs` (`HidSource`: the
  endpoint that wakes it, drain into the one batch path, live; `Emit` + `DeviceFrame`: the batch
  header), `src/virtio_source.rs` (today's open/ack/drain behind it, opened ONCE — every grant is in
  place before hidrawd runs — its lines bound to a declared notify endpoint instead of the control
  endpoint; what a device is — announced, then settled by what it sends — is the host-tested
  `src/virtio_class.rs`), `src/batch.rs` (the one path to inputd: HID events → `WireHidBatch`
  chunks, I1/I2), and
  `src/os_lite.rs` one loop over both sources on one waitset. No timer is left: the 20 Hz re-probe
  of a device-less lane and its `TIMER` pair are gone. The old loop's names are retired
  (`scripts/check-retired-names.sh`); `os_lite.rs` 605 → 187 LOC (off the LOC baseline).
- **P2 — the USB source** ✅ 2026-10-05:
  - xhcid serves the class (RFC-0099 §5, amended): its spec exposes a server (`slots::xhcid::SERVER`)
    and a policyd route; it announces `@ready` at once (`xhcid: serving (class=hid-boot)`), with or
    without a controller — so it gates `DisplayReady` and can never hold it up. A SUBSCRIBE moves
    the client's push-channel SEND half along; policyd must grant the kernel-attributed sender
    `usb.hid` (`policies/base.toml`: hidrawd alone; xhcid `policy.delegate`). `src/hid_class.rs` is
    the pure class server: device names never reused while the client could hold them, the answer
    + attaches + detaches owed in order (a full channel retried on the next wake or after 4 ms),
    reports fire-and-forget per drain (≤ 16 per frame, a fuller drain spills), a new subscriber
    told every interface attached so far, a dead channel ends the subscription. The core's notes
    gained the device identity (`HidInterface`) and `HidLost` (a HID pipe given up).
  - The wire: `nexus_wire::usb` (SUBSCRIBE, the answer, ATTACHED, REPORTS with a checked list
    iterator and an allocation-free `ReportList` builder, DETACHED).
  - init: the push pair is declared (`NamedSlot::UsbHidRecv/UsbHidSend`, minted by
    `pin_declared_waits` with depth 32 — the pairs carry their depth now), xhcid's server pair is
    minted (`Endpoints::usb_req/usb_rsp`), hidrawd's arm provisions its declared legs (the
    SUBSCRIBE route to xhcid).
  - hidrawd: `src/usb_source.rs` (pure: frames from xhcid's identity only, attach/reports/detach
    through the ONE device table — `HidrawdService` became the table of every registered device's
    parser, `ingest_report_into` allocation-free, `release_into` on detach — a refused frame
    counted whole, a refused report never half-parsed) and its OS side in `os_lite.rs` (the
    SUBSCRIBE, the push channel drained with the kernel's sender id). USB devices are
    `0x100 + n` on the wire, apart from the virtio ones.
  - `userspace/hid`: `parse_report_into` (events appended to a reused buffer; a refused report
    leaves it untouched) and `release_all_into` for both parsers; `parse_report` is the thin
    allocating form over it.
- **P3 — the QEMU proof** ✅ 2026-10-05: `[profile.usb-visible]` (the visible lane, launcher
  `QEMU_INPUT_TRANSPORT=usb` — no virtio input device — the `usb` lane's controller, hub,
  `usb-kbd` and `usb-mouse`, the injector's QMP input reaching them), `just ci-os-usb-visible` in
  `test-all`; the ladder (`USB_INPUT_MARKERS`) requires the chain end to end, the visible-input
  guard is transport-aware, the chain-marker contract checks `input-live` there at every run.
  Every full-ladder lane requires the class served and the subscription admitted
  (`USB_CLASS_MARKERS`).
- **P4 — the board** (with TASK-0328 U3): `inputd: live pointer route on` — a real report inside
  a bounded wait — and `board-visual: typed`. **Block 2 gate.**

## Proof (2026-10-05)

- Host: `cargo test -p hidrawd` (contract 14 + `virtio_class` 5 — the virtio source's decisions
  against fake batches — + `usb_source` 9: the desk's report formats as
  goldens, detach releases what was held, a re-attached name released first, `test_reject_*` for a
  foreign sender, frames that do not add up, unknown devices, roles, the table bound, a report
  outside the boot formats refused alone); `cargo test -p xhcid` (24: + 10 class tests over the
  real core and the machine — replay, unparsed reports one frame per drain, detach + a new name on
  re-plug, a full client keeps attaches/detaches owed and drops reports, a withdrawn attach, a dead
  client, the refusal, spilling and a lost pipe); `cargo test -p nexus-wire usb` (7, golden bytes +
  reject matrices); `cargo test -p input_v1_0_host --test hid_contract` (13: + the reused buffer,
  release-all, a refusal leaving the buffer untouched); `cargo test -p policy --test
  committed_policy` (+ `test_reject_a_second_subscriber_of_usb_hid`).
- QEMU `usb-visible` [PASS] (`build/logs/usb-visible--2026-10-05T10-55-08/`): `xhcid: serving`,
  `init: up xhcid` before `stage: display-ready`, `xhcid: hid class subscribed (interfaces=0)` —
  hidrawd subscribed before enumeration, the attaches followed — `hidrawd: usb hid device
  (vid=0627 pid=0001 role=keyboard)` and `(… role=mouse)`, `hidrawd: ready` 315 ms after entry, I1,
  `hidrawd: usb hid report seen`, I2, inputd I3, `inputd: live pointer route on`, `windowd: cursor
  move visible`, `SELFTEST: ui v2 input ok`, `inputd: live keyboard route on`, `windowd: keyboard
  visible`; no report refused, no frame rejected, the client never full; verify-uart clean,
  chain-marker contract 15/15 (incl. `input-live`), pixel proof ok.

## Findings (not this task's; recorded)

- **inputd takes `OP_PUSH_HID_BATCH` from any sender** holding a SEND on its request endpoint
  (hidrawd and the proof harness have one): it discards `sender_service_id` on receive and no test
  covers a foreign batch. A follow-up: admit batches from hidrawd's identity only, with a
  `test_reject_*` (the imed pattern, `SELFTEST: imed reject foreign ok`).
- **The QMP injector's greeter step never runs**: it waits for `windowd: greeter visible`, windowd
  says `windowd: greeter on (dsl)` — so every injected proof (this lane, the input-flood lane) runs
  on the greeter screen. The proofs it gates (a press, a key) do not depend on it; the login does.
- **The injector's relative-mouse dead reckoning is unverified**: it assumes inputd's start cell
  (489, 208 on 1280x800), the first frame shows the cursor elsewhere; the press it needs lands
  anyway (any press counts). Measure before the injector targets anything with a relative mouse.
- **The settings and session watch channels** are push channels minted by bespoke init code; the
  declared push pair (`pin_declared_waits`) could carry them (two arms, a follow-up of their own).

## Constraints / invariants (hard requirements)

- **One contract**: `WireHidBatch` to inputd unchanged; no second ingress loop, no second parser.
- **Untrusted input**: report lengths bounded by the parsers (`test_reject_*`); a refused report is
  counted, never parsed partially.
- **Identity**: xhcid admits hidrawd by `sender_service_id` + policyd (`usb.hid`).
- **No stuck input**: a detached or dropped device releases what it held.

## Definition of Done

1. ✅ hidrawd's ingress is one loop over sources; the virtio-only loop is gone (retired-name gate).
2. ✅ Host tests: both sources against fakes (`virtio_class` against fake event batches,
   `usb_source` against xhcid's frames), detach releases held keys, the measured report formats as
   goldens.
3. ✅ QEMU: the `usb-visible` lane in `test-all` with `SELFTEST: ui v2 input ok` over USB.
4. ✅ 2026-10-06 Board: `inputd: live pointer route on` / `… keyboard route on` + `board-visual: typed` + `pointer` (TASK-0328 U3 cycle 20, `[PASS] board-visible`).
5. ✅ Docs sweep: RFC-0099 Phase 2 (§5 amended, §7 markers, the checklist), CHANGELOG,
   IMPLEMENTATION-ORDER, `docs/testing/README.md`, `docs/dev/ui/input/input.md`.
