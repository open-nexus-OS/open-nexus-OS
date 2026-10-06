# RFC-0099: USB host contract — `xhcid` owns the controller, a reactive event-ring driver, class services as its clients

- Status: Draft (seed 2026-10-04 — TASK-0328 U0; the measurements it rests on are archived)
- Owners: @runtime
- Created: 2026-10-04
- Last Updated: 2026-10-05
- Links:
  - Tasks: `tasks/TASK-0328-usb-host-stack-v1-xhci-hub-enumeration-class-clients.md` (execution + proof,
    U0–U3), `tasks/TASK-0253B-hid-ingress-hidsource-usb-and-virtio.md` (the HID class client, U2)
  - ADRs: `docs/adr/0039-device-class-driver-architecture.md` (layering; amended for USB as a bus service)
  - Related RFCs: `RFC-0017` (device MMIO access model), `RFC-0098` (the FDT is the one hardware truth;
    DMA reach C4), `RFC-0106` (socd — the one owner of SoC glue), `RFC-0096` (IPC contract v2),
    `RFC-0052` (input v1.0a host HID contract)
  - Measurements: `docs/board/measurements/2026-10-04-usb-topology/`,
    `docs/board/measurements/2026-10-04-usb-boot-protocol/`

## Status at a Glance

- **Phase 0 (contract seed + measurements + HID boot parser fixed to the measured reports)**: ✅ 2026-10-04
- **Phase 1 (`xhcid` + `nexus-usb` on QEMU: controller, hub, enumeration, HID boot interfaces)**: ✅ 2026-10-05 (the `usb` lane, `test-all`)
- **Phase 2 (the HID class client: `hidrawd` sources over one contract)**: ✅ 2026-10-05 (the `usb-visible` lane, `test-all`; §5 amended)
- **Phase 3 (the board: socd glue + hub power + TT; keyboard and mouse drive the desktop)**: ✅ 2026-10-06 — `[PASS] board-visible` (46 rungs) + `board-headless` (38) on the desk board, cycle 20 of 20 (`docs/board/measurements/2026-10-05-usb-cycle1/`): the SuperSpeed PHY released from its inverted-polarity reset, both PHYs' PLL sequences, DWC3 host mode, the xHCI at ~1 kHz with eight TRBs per pipe, the hub + keyboard + receiver, inputd's live routes from the operator's hand, the pointer on the controller's own layer

Definition: "Complete" means the contract is defined and the proof gates are green (tests/markers).

## Scope boundaries (anti-drift)

- **This RFC owns**:
  - who owns a USB host controller (one service, `xhcid`) and how class services reach devices
    (IPC clients, never MMIO);
  - the driver's execution model: interrupt-driven event rings, no polling in steady state, bounded
    waits as kernel timers on a waitset;
  - the DMA discipline for memory the CPU and the controller share for the controller's lifetime,
    on coherent and non-coherent machines;
  - enumeration through hubs, including the transaction-translator fields of full/low-speed devices
    behind a high-speed hub;
  - the HID boot-protocol class contract v1 (what xhcid hands to the HID client) and its markers.
- **This RFC does NOT own**:
  - the HID event model, keymaps, repeat, pointer acceleration (RFC-0052; `userspace/hid`);
  - SoC clocks/resets/power-domain/pad writes (RFC-0106 — socd), only what xhcid asks socd for;
  - mass storage, isochronous classes, USB gadget mode, SuperSpeed link features (non-goals below);
  - the report-protocol HID descriptor parser (a follow-up, see Open questions).

### Relationship to tasks (single execution truth)

TASK-0328 carries the stop conditions and proofs of Phases 0, 1 and 3; TASK-0253B those of Phase 2.

## Context

The OS has no USB code. Input on the board is USB only: the desk's keyboard and mouse sit on the
USB-A ports. Measured on the stock system (`2026-10-04-usb-topology/`, `2026-10-04-usb-boot-protocol/`):

- **One xHCI 1.10 controller** at `0xc0a00000` (the `snps,dwc3` node with `dr_mode = "host"`, IRQ
  125), 64 slots, 1 interrupter, 2 root ports (1: USB 2.0, 2: USB 3.0), **64-byte contexts**
  (HCCPARAMS1.CSZ = 1), **1 scratchpad buffer**, 64-bit addressing, port power control.
- **The USB-A ports are behind an on-board high-speed hub** (`2109:2817`, 5 ports, per-port power,
  TT think time 32 FS bit times, bPwrOn2PwrGood 350 ms) whose supply and reset are GPIO lines (97,
  123, 124; `vbus_delay_ms = 200`).
- **Both HID devices are full speed one hub deep** — their slot contexts need the
  transaction-translator fields; QEMU's `usb-hub` is itself full speed, so QEMU cannot exercise TT.
- **DMA reach**: the controller's bus maps only the first 2 GiB identity (stock `dram_range@0`, our
  `storage-bus`); the SoC bus is **not coherent** (the tree's `dma-noncoherent`).
- **Boot protocol, as the devices really answer**: every boot interface accepts SET_PROTOCOL(0);
  the mouse interface STALLs SET_IDLE; the mouse sends **4-byte** reports (wheel in byte 3, thumb
  buttons on bits 3–4) at **~1000 reports/s**; the first report after the protocol switch is still in
  the report format (9 bytes); EP0's max packet is 32 on one device and 64 on the others.

## Goals

- One service owns the controller; every USB class reaches its devices through it.
- Zero CPU time when no device produces data: the driver sleeps on its waitset; the controller
  schedules the bus by itself (the endpoints' intervals) and interrupts only when transfers complete
  or ports change.
- The same driver on QEMU (`qemu-xhci` over PCI, coherent) and the board (platform node, not coherent).
- Enumeration through hubs (route strings, hub class, TT) from the first version.
- Keyboard and mouse in the boot protocol as the first class (HID v1).

## Non-Goals

- Mass storage, isochronous transfers (audio/video), USB gadget/device mode (`TASK-0261`, parked).
- SuperSpeed link management beyond reporting it: v1 serves HS/FS/LS through the USB 2.0 root port;
  a USB 3.0 port that trains is logged, never fatal.
- MSI/MSI-X (QEMU's `virt` PCI host offers INTx; the board has a platform interrupt).
- Report-protocol HID (touchpads, gamepads, consumer keys) — a follow-up behind the descriptor parser.

## Constraints / invariants (hard requirements)

- **No fake success**: `xhcid: ready` only after the controller runs (USBCMD.R/S = 1 and
  USBSTS.HCH = 0) and every root port is powered; `device enumerated` only after a successful
  Address Device and the device descriptor read back; `inputd: live pointer route on` only for a real
  report inside a bounded wait.
- **No polling in steady state**: the service blocks on ONE waitset (IRQ endpoint, client endpoint,
  one-shot timer). Waits the specification mandates during reset (HCRST, CNR, HCH) are bounded by
  the kernel timer on that waitset — never a spin against the clock (`scripts/check-wait-not-poll.sh`).
- **Bounded resources**: slots enabled ≤ 16 (`CONFIG.MaxSlotsEn`), hub depth ≤ 5 (route string),
  hub ports ≤ 15, configuration descriptor ≤ 1024 bytes, interfaces per configuration ≤ 8,
  endpoints per interface ≤ 4, interrupt-IN TRBs queued per endpoint = 8 (4 until board cycle 11
  measured them running dry at the board's ~6 ms wake interval), one event-ring segment of
  256 TRBs, one command-ring segment of 64 TRBs, report frames to a client ≤ 16 reports.
- **The FDT/PCI plan is the one hardware truth**: the controller's window, IRQ, reach and
  coherence arrive inside the device capability init grants (RFC-0098); no address in code.
- **Security floor**: descriptors and reports are untrusted input, bounded before parsing; a
  device's strings are never read in v1 and never logged; class clients are identified by
  `sender_service_id` and admitted by policyd (deny by default).

## Proposed design

### 1. Ownership and layering (normative)

```
inputd ── hidrawd (HID class client: boot parsers → WireHidBatch)
              │  IPC: attach / reports / detach  (identity = sender_service_id, policy-gated)
              ▼
           xhcid  (the bus service: controller, rings, hubs, enumeration, class dispatch)
              │  nexus-usb (descriptors, setup packets, hub class, TT, route strings — host-tested)
              │  nexus-driverkit (MMIO, DMA regions + buffers, cache maintenance)
              ▼
kernel ── DeviceMmio cap (window + IRQ + reach + coherence), irq_bind/irq_complete, timer, waitset
```

`xhcid` is the only holder of the controller's MMIO window and IRQ. A USB class service (HID now,
storage later) is an IPC client that is told about devices of its class and receives their data;
it never sees an MMIO window or a TRB. This is ADR-0039's layering with USB as a **bus service**
between the device-class service and the hardware (ADR-0039 amendment).

**Grants** (init): QEMU — the PCI function of class `0x0c03`, prog-if `0x30` (xHCI) from init's PCI
plan, bus mastering enabled after the grant (the boot-disk pattern); board — the `snps,dwc3` node
with `dr_mode = "host"`. Either becomes ONE `device.mmio.usb` capability for xhcid (a new policy
class). On the board xhcid asks socd to bring its node up (RFC-0106) before touching the window;
the dual-role controller's global registers (port capability = host) are xhcid's, the APMU glue
word, clocks, resets and power domain are socd's.

### 2. The reactive loop (normative)

xhcid's main loop waits on one waitset with three members and does work only when one fires:

1. **IRQ endpoint** (the controller's line, `irq_bind`): clear USBSTS.EINT and IMAN.IP (both
   write-1-to-clear), then **drain the event ring**: while the TRB at the dequeue pointer carries
   the consumer cycle state, dispatch it and advance (toggling the cycle state at the segment end).
   After the drain write ERDP with the new dequeue pointer and EHB set (clearing it), then
   `irq_complete`. Events that arrive after the drain set IP again and re-assert the line once the
   moderation interval has passed, so no wakeup is lost.
2. **Client endpoint**: class clients' requests (subscribe, a control transfer on behalf of a
   class, ack/credit).
3. **One-shot timer** (a kernel timer on a declared notify pair): the deadline of the earliest
   pending wait — a command's timeout, a port's power-good or reset recovery time, a reset step of
   the controller. Armed only while such a wait exists; disarmed otherwise.

Event dispatch:

| event | owner | effect |
|---|---|---|
| Command Completion | the command queue | completes the head command (one in flight; the ring is serial) and advances the device state machine that issued it |
| Transfer Event (EP0) | the device's control pipe | completes the control transfer's TD; a STALL completion is recovered (Reset Endpoint, Set TR Dequeue Pointer) and reported to the transfer's issuer |
| Transfer Event (interrupt IN) | the endpoint's report queue | report length = requested − residual; the report is appended to the client's frame; the TRB is requeued at once |
| Port Status Change | the root-port state machine | connect → reset → enabled (speed) → enumerate; disconnect → Disable Slot → detach to clients |
| Host Controller | the controller | HSE/HCE → bounded controller reset + `xhcid: FAIL (…)` |

Every state machine (controller init, root port, hub port, device enumeration, HID setup) is a
plain enum advanced by completions and timer deadlines — no thread, no future executor, no busy
wait. The controller schedules interrupt endpoints by their intervals: with four Normal TRBs queued
per HID endpoint (IOC set), the CPU sleeps between reports.

**Interrupt moderation**: IMOD bounds the interrupt rate the controller can produce. One measured
mouse completes ~1000 transfers/s; v1 set IMODI = 4000 (1 ms), which keeps the added latency under
a millisecond and caps the rate at 1 kHz however many devices report. Phase 3 measures the board's
interrupt rate and input latency (the `xhcid: irq hz=…` counters) and records the value it keeps.
**Measured (board cycle 11, 2026-10-05)**: with IMODI = 4000 and four TRBs per pipe the board
woke the driver ~160 times a second with events pending (`irq hz=162 events hz=401 per_irq_max=4
dry=44`) — the pipes ran dry 44 times a second, reports lost at the device. Cycle 12 runs
IMODI = 0 (no moderation) and eight TRBs per pipe (§Bounded resources), with the drain's own cost
in the line (`drain_us avg= max=`): the interrupt rate against the report rate says whether the
moderation or the kernel's wake path set the interval; the value kept is the one that measurement
names.

### 3. Rings, contexts and DMA (normative)

Memory the controller reads or writes:

| structure | size / alignment | written by | read by | non-coherent maintenance |
|---|---|---|---|---|
| DCBAA (+ scratchpad array) | (MaxSlotsEn + 1) × 8 B, 64 B | CPU | controller | clean after write |
| scratchpad buffers | PAGESIZE each, page | controller | controller | flush once at setup; the CPU never touches them |
| command ring | 64 TRBs (1 KiB), 64 B, one segment | CPU | controller | clean each TRB before the doorbell |
| event ring + ERST | 256 TRBs (4 KiB) + 1 entry, 64 B | controller | CPU | flush (invalidate) the line before reading a TRB; the CPU never writes it after setup |
| input context | 33 × CSZ, 64 B | CPU | controller | clean before the command |
| output device context | 32 × CSZ, 64 B | controller | CPU | flush before reading |
| transfer rings (EP0, interrupt IN) | 64 / 16 TRBs, 64 B, one segment | CPU | controller | clean each TRB before the doorbell |
| report buffers | max packet each | controller | CPU | flush before requeue and before reading |

- Every structure lives in a **contiguous** VMO made for the controller's device capability
  (`vmo_create_contiguous`): a block of ≤ 64 KiB is aligned to its own power-of-two size, so no
  ring segment crosses a 64 KiB boundary and every structure is 64-byte aligned (kernel buddy
  invariant; asserted by xhcid at setup from the runs the kernel reports). The bus address comes
  from the kernel (`vmo_runs`) — xhcid never computes one; the reach (the first 2 GiB on the board)
  is the kernel's to honour.
- **TRB publication order** (non-coherent): write the TRB's words except the cycle bit, clean the
  line, fence; write the cycle bit, clean, fence; then the doorbell (an MMIO write after the
  fence). The controller never sees a TRB whose cycle bit is valid before its fields.
- **Two DMA shapes in nexus-driverkit**: per-transfer buffers keep `DmaBuffer`'s ownership
  typestate (CPU XOR device); memory both sides use for the controller's lifetime (rings, contexts,
  DCBAA, ERST) is a **shared DMA region** with explicit `publish(range)` (clean) and
  `observe(range)` (flush) of entry ranges — the one seam where a driver maps such memory, host-tested
  against a model of a non-coherent cache (the TASK-0246 P2 model). On a coherent device both are
  no-ops.

### 4. Enumeration (normative)

- **Controller init** (xHCI §4.2): wait CNR = 0; stop (R/S = 0, wait HCH = 1); HCRST, wait HCRST = 0
  and CNR = 0; MaxSlotsEn; DCBAAP (+ scratchpad array from HCSPARAMS2); CRCR with RCS = 1;
  interrupter 0 (ERSTSZ, ERDP, ERSTBA last, IMOD, IMAN.IE); USBCMD.INTE + R/S; wait HCH = 0; power
  every root port (PORTSC.PP when HCCPARAMS1.PPC = 1). Context size from HCCPARAMS1.CSZ; the
  Supported Protocol capabilities name each root port's USB revision.
- **A device** (root or hub port): port reset → speed → Enable Slot → Address Device (slot
  context: route string, speed, root-hub port number, and for a full/low-speed device behind a
  high-speed hub the **TT hub slot ID, TT port number and MTT** of that hub; EP0 context with the
  speed's default max packet — 8 for full speed until the first eight bytes of the device descriptor
  name `bMaxPacketSize0`, then Evaluate Context) → device descriptor → configuration descriptor
  (bounded) → Configure Endpoint for the chosen interfaces' endpoints (interval exponent from
  `bInterval` per speed: full/low speed `3 + floor(log2(bInterval))`, high speed `bInterval − 1`)
  → SET_CONFIGURATION.
- **A hub** (class 9): GET_DESCRIPTOR(hub) → its slot context becomes a hub (Hub = 1, number of
  ports, TTT from `wHubCharacteristics`, MTT = 0: v1 keeps alternate setting 0, single TT) →
  SET_PORT_FEATURE(PORT_POWER) on every port → wait `bPwrOn2PwrGood × 2 ms` (the timer) → the
  hub's status-change endpoint is queued like a HID endpoint; a change bit → GET_PORT_STATUS →
  clear the change → PORT_RESET → wait the reset change → speed → enumerate the child with the
  route string extended by the port.
- **HID boot interfaces** (class 3, subclass 1, protocol 1 keyboard / 2 mouse): SET_PROTOCOL(0),
  SET_IDLE(0) best effort (a STALL is recovered and ignored — measured on a mouse), then four
  Normal TRBs per interrupt-IN endpoint with buffers of its max packet, and the device is announced
  to the HID client.

### 5. The class-client contract — HID v1 (normative, wire `nexus_wire::usb`)

Amended at Phase 2 (TASK-0253B, 2026-10-05) with what the implementation had to decide: the push
channel, the replay, the delivery classes, the device names, the client's refusals, readiness.

- **Subscribe**: the client sends `SUBSCRIBE { class = HID_BOOT }` (`[U, B, 1, OP_SUBSCRIBE, class]`)
  on xhcid's server endpoint, moving the SEND half of its own push channel along. Init mints that
  channel for the client from its declaration (`NamedSlot::UsbHidRecv`/`UsbHidSend`, queue depth
  32); the client keeps the RECV half as a waitset member, and after the move xhcid is the
  channel's only sender. xhcid admits the subscriber if policyd grants its kernel-attributed
  `sender_service_id` the class (`usb.hid`; only hidrawd holds it) — never a payload string. One
  subscriber per class: an admitted one replaces the one before (whose channel is closed).
- **The answer** is the first frame on the push channel: `[U, B, 1, OP_SUBSCRIBE|0x80, status]`
  (OK, MALFORMED, DENIED, UNSUPPORTED). A refused subscriber hears its status and nothing else; the
  subscription it tried to take is untouched. A SUBSCRIBE that came without a channel is answered
  MALFORMED on xhcid's shared response endpoint.
- **Replay**: an admitted subscriber is told every interface attached so far (an attach each,
  after the answer). The client may subscribe before, during or after enumeration; nothing depends
  on the order.
- **Attach**: `OP_DEVICE_ATTACHED { device: u16, vendor: u16, product: u16, interface: u8, role:
  keyboard|mouse, max_packet: u16 }` — sent once the interface's TRBs are queued. `device` names
  the attachment and is never reused while the client could still hold it: a re-plugged keyboard
  is a new device, so a stale report can never be read as the new one's.
- **Reports**: `OP_HID_REPORTS { device: u16, count: u8, len: u8, list }` — the reports one drain
  produced for that interface, each `len: u8` + 1..=64 bytes, at most 16 reports and 240 list
  bytes per frame (a fuller drain sends more frames). A report is the controller's bytes, unparsed
  — the ONE parser is `userspace/hid` in the client (it refuses lengths outside the boot formats,
  which also drops the stale report-format frame right after the protocol switch). A list that does
  not add up to `count` refuses the whole frame.
- **Detach**: `OP_DEVICE_DETACHED { device }` when the port disconnects, the device is disabled or
  the interface's pipe is given up after repeated errors. The interface's last reports go out
  before its detach; the client releases what the device held (key-ups, button-ups) — nothing
  stays pressed.
- **Delivery**: the answer, attaches and detaches are OWED — kept in order until the channel takes
  them; a full channel is tried again on the next wake or after 4 ms (a one-shot on xhcid's
  timer). An attach not yet delivered when its interface goes away is withdrawn (the client never
  hears of it). Reports are not owed: a frame the full channel refuses is dropped and counted
  (`xhcid: hid client full (reports dropped)`, said once). A boot keyboard report carries the whole
  key state, so the next report heals a dropped one; a dropped mouse frame loses its deltas only. A
  report never overtakes its interface's attach. A dead channel ends the subscription; the
  interfaces stay attached for the next subscriber.
- **The client's side** (hidrawd's USB source): frames are taken from xhcid's kernel identity
  only; anything else — a frame that does not decode or add up, a device never attached, a role
  outside the boot protocol, an attach past the client's bound (16) — is refused whole and
  counted, and a report its parser refuses is counted, never half-parsed.
- **Readiness**: serving the class makes xhcid a member of the DisplayReady barrier (RFC-0093): it
  announces `@ready` as soon as its endpoints are set up, with a controller or without one.

### 6. Board specifics (normative for Phase 3; built 2026-10-05, TASK-0328 U3)

- **Order on the board**: xhcid reads its tree slot, finds the host node (`snps,dwc3` in host
  mode) and the hub node (`spacemit,usb3-hub`) and has socd bring both up (`BRING_UP` by path,
  RFC-0106) BEFORE it maps or reads the controller — a read of a gated block can stall the bus.
  socd's refusal or fault leaves the controller untouched (`xhcid: FAIL (step=glue-host|glue-hub
  cc=<status>)`); a tree without the host node (QEMU's PCI controller) needs no glue.
- **The host node** (`/soc/storage-bus/usb@c0a00000`): bus domain, the three resets (`ahb`,
  `vcc`, `phy`), the `usbdrd30` clock — our loader leaves the word at 0 (cycle 1: `apmu+5c:
  0>f00`) — then the APMU glue word's bit 15 (`nexus,glue-words = <&syscon_apmu 0x3c8 0x8000
  0x8000>`): the stock word is `0x0b008000`, but bits 24..25 ignore a write (status bits the
  stock PHY drivers raise; cycle 1 read `0x08008000` back), so only bit 15 is glue.
- **The DWC3 around the xHCI** (same window, `0xc100` on): xhcid identifies it (`GSNPSID`
  `0x5533330a`) and makes the three measured fields so — `GCTL.PRTCAPDIR` = host, the PHY
  suspend bits the stock tree disables by quirk clear (`GUSB2PHYCFG` bits 6 and 8,
  `GUSB3PIPECTL` bit 17) — read back, every other bit left; a window that is no DWC3 is refused.
- **The USB 2.0 PHY** (`/soc/storage-bus/phy@c0a30000`, its window a second `device.mmio.usb`
  grant): socd gates its clock; xhcid reads its words against the stock system's (`xhcid: usb2
  phy (…)`: how many match, which differ) and writes the differing writable ones to the stock
  values in the order the vendor PHY driver's init runs them (reference only: the PLL divider
  word `0x98` first, the PLL's lock — `0x04` bit 0 — waited for on the one-shot, bounded, then
  the reset/mode word and the rest), each read back (`xhcid: usb2 phy set (…)`); `0x38` is
  status, the dump's words past `0x4c` are compared, never written.
- **The SuperSpeed (combo) PHY** (`/soc/storage-bus/phy@c0b10000`, a third grant): the xHCI's
  reset inside the DWC3 waits on this PHY's PIPE clock, so it is brought up, not only measured
  (board cycles 4–9, 2026-10-05: HCRST never completed with the block reading zeros). socd
  releases its global reset — PCIe port A's, bit 8 of APMU `0x3cc`, SET = held (RFC-0106's one
  inverted-polarity APMU reset) — clears the port's hold-PHY-reset bit (bit 30) and selects the
  lane for USB (`0x110` bit 3); xhcid then compares the block's words with the stock twenty-three
  (`xhcid: ss phy (…)`) and runs the USB-mode PLL sequence as the vendor PHY driver does it
  (reference only; `ss_phy.rs`): the test word zeroed, the internal timer for USB, the 24 MHz
  reference and the 5000 ppm spread-spectrum depth, the software init-done bit, then the lock
  polled up to 500 ms on the one-shot — `xhcid: ss phy pll ready (after … ms calibrated=…)`, or
  `… pll NOT ready (…)` (said, never fatal: the controller's reset that follows measures it).
  Calibration (`0x84` bit 10) is measured; running it (PCIe port A's application clocks and
  resets, in the table) is a cycle's step only if a board reads `calibrated=0`.
- **Hub power** (`/usb-hub`, the stock binding's shape): socd puts the three pads on the GPIO
  function (97 mux 1, 123/124 mux 0, the stock words' pulls), drives the hub's two lines (GPIO
  123, 124) high as outputs, settles the measured 200 ms on its one-shot, then VBUS (GPIO 97) —
  each through the bank's masked set registers, read back (RFC-0106 amendment). No driver
  drives a GPIO.
- The USB 3.0 root port (the hub's SuperSpeed twin) may train; xhcid logs it and leaves it alone
  in v1.

### 7. Markers (normative)

- `init: usb host from pci (<bdf> bar=0x… irq=…)` / `init: usb plane none (no host controller)`
- `xhcid: controller ok (version=… ports=… slots=… csz=… scratch=… irq=…)`
- `xhcid: ready (ports=… connected=…)` — every root port powered and scanned
- `xhcid: superspeed port left alone (port=…)` — a USB 3 root port with a link (v1 leaves it)
- `xhcid: hub (slot=… port=… speed=… ports=… ttt=…)` — `port` is the hub's root port
- `xhcid: device enumerated (vid=… pid=… class=… speed=… slot=… route=0x…)` — the identity
  leads, the slot and route (the order the ports answered in) trail; a lane's rung stops before
  them
- `xhcid: hid boot interface (vid=… pid=… role=… if=… ep=0x… mps=… interval=… slot=…)`
- `xhcid: irq hz=… events hz=… reports hz=… per_irq_max=… dry=… dropped=… refused=…` — the
  one-second counters under traffic (Phase 3): `dry` counts drains in which a pipe's every TRB
  had completed (the device went unpolled — reports lost at the device), `dropped`/`refused`
  the class server's running totals
- `xhcid: device detached (slot=…)`
- `xhcid: no host controller (usb plane none)` — xhcid holds no window and parks
- `xhcid: FAIL (step=… cc=…)` — the failing step and its completion code (for a refused
  descriptor: the `nexus_usb::UsbError` code); the harness FAIL gate fails any lane on it
- Phase 2 (the class): `xhcid: serving (class=hid-boot)` (the `@ready` line, every lane),
  `xhcid: hid class subscribed (interfaces=…)` / `xhcid: hid class subscriber refused (status=…)`,
  `xhcid: hid interface lost (slot=… if=…)`, `xhcid: hid client full (reports dropped)`
- Phase 2 (the client): `hidrawd: usb hid subscribed` / `hidrawd: usb hid subscribe refused
  (status=…)`, `hidrawd: usb hid device (vid=… pid=… role=…)`, `hidrawd: usb hid device gone
  (vid=… pid=… role=…)`, `hidrawd: usb hid report seen`, `hidrawd: usb hid report refused (not a
  boot report)`, `hidrawd: usb hid frame rejected (…)`
- `inputd: live pointer route on` / `inputd: live keyboard route on` — inputd's own word that a
  real report reached its live routes (the board's input proof; a selftest-client probe that
  polled windowd for it was retired in board cycle 16 — the topology declares no
  selftest-client → windowd route, so it observed nothing)

### Phases / milestones (contract-level)

- **Phase 0**: this contract + the two measurements + the HID boot parsers taking the measured
  reports (3..=8-byte mouse reports with the wheel, eight buttons; the keyboard's reserved byte and
  error usages) — proof: `cargo test -p input_v1_0_host --test hid_contract`, `-p hidrawd --test contract`.
- **Phase 1** (TASK-0328 U1): `nexus-usb` + `xhcid` host-tested against an xHCI behavioural model
  (command/event/transfer rings, port changes, a hub); QEMU profile `usb` (`qemu-xhci`, a `usb-hub`,
  `usb-kbd` and `usb-mouse` behind it) — `xhcid: ready`, `xhcid: hub`, two `device enumerated`, two
  `hid boot interface`; in `test-all`.
- **Phase 2** (TASK-0253B): hidrawd's ingress generic over sources (virtio-input, USB HID); QMP
  input to the USB devices moves the desktop — `SELFTEST: ui v2 input ok` over USB (profile
  `usb-visible`: the visible lane without a virtio input device); on every lane the class is served
  and hidrawd's subscription admitted.
- **Phase 3** (TASK-0328 U3): the board — socd glue, hub power, TT; `[PASS] board-visible` with
  `inputd: live pointer route on` / `… keyboard route on` and `board-visual: typed`. **Block 2 gate.**

## Security considerations

- **Threat model**: a malicious or broken USB device (descriptor overruns, absurd sizes, endless
  hubs, report floods, stalls); a confused-deputy client asking xhcid to act on a device of another
  class; a spoofed client identity.
- **The class boundary** (Phase 2): keystrokes reach exactly one client — the subscriber policyd
  admitted by its kernel identity (`usb.hid`, held by hidrawd alone: `test_reject_a_second_
  subscriber_of_usb_hid`); a refused subscriber hears its status and nothing else
  (`test_reject_a_refused_subscriber_hears_its_status_and_nothing_else`); the push channel's SEND
  half is moved, so xhcid is its only sender, and the client still takes frames from xhcid's
  identity only (`test_reject_frames_not_from_xhcid`); frames are bounded and checked whole on
  both sides (`nexus_wire::usb` `test_reject_*`, hidrawd's `test_reject_frames_that_do_not_add_up`).
- **Mitigations**: descriptor parsing in `nexus-usb` bounded before use — `test_reject_*` for a
  `bLength` below 2 or past the buffer, `wTotalLength` past 1024, more interfaces or endpoints than
  the bounds, a `bMaxPacketSize0` outside {8, 16, 32, 64}, a max packet of 0 or past 1024, a hub with
  0 or more than 15 ports, a route deeper than 5 hubs; a transfer's residual larger than its request
  refused; report frames bounded; class admission by `sender_service_id` + policyd; MMIO `USER|RW`,
  never exec; DMA only inside the capability's reach (kernel-checked); device strings never read.
- **Open risks**: a device that floods reports at its interval is bounded by IMOD and the frame
  drop counter, not refused; a hub that lies about TT parameters breaks only its own children.

## Failure model (normative)

- A command that does not complete before its deadline is aborted (CRCR.CA) → `xhcid: FAIL
  (step=<command> cc=timeout)`; the owning device is disabled, the controller keeps running.
- A STALL on EP0 → Reset Endpoint + Set TR Dequeue Pointer; the control transfer returns the stall
  to its issuer (SET_IDLE: ignored; anything else: the interface is not used, with a marker).
- A transaction error on an interrupt endpoint → Reset Endpoint + requeue, at most 3 in a row,
  then the device is disabled with a marker.
- Host System Error / Host Controller Error → one bounded controller reset (the init sequence);
  a second failure stops xhcid with `xhcid: FAIL (step=controller …)`.
- No silent fallback: no device is ever reported attached that did not complete enumeration.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p input_v1_0_host --test hid_contract && cargo test -p hidrawd --test contract
cd /home/jenning/open-nexus-OS && cargo test -p nexus-usb && cargo test -p xhcid   # Phase 1
cd /home/jenning/open-nexus-OS && cargo test -p nexus-wire usb && cargo test -p hidrawd --test usb_source   # Phase 2
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just test-os usb          # Phase 1 (profile `usb`)
cd /home/jenning/open-nexus-OS && just test-os usb-visible  # Phase 2 (the desktop over USB alone)
cd /home/jenning/open-nexus-OS && just test-all             # both lanes included
```

### Proof (board)

```bash
cd /home/jenning/open-nexus-OS && bash scripts/board-test.sh --profile=board-visible --log=build/logs/latest-board/uart.log
```

## Alternatives considered

- **A timer that polls the event ring**: wakes an idle system for nothing and trades latency for
  CPU; the controller already schedules the bus and interrupts on completion. Rejected.
- **Each class driver owns the controller**: the controller, its rings and its slots are shared by
  every device on the bus. Rejected (ADR-0039: one owner per device, classes as clients).
- **A USB stack in the kernel**: drivers live in userspace (microkernel). Rejected.
- **An async executor (futures) inside xhcid**: the state machines are few and small; plain enums
  advanced by events are deterministic, testable on the host and need no runtime. Rejected for v1.
- **Report protocol with a full HID descriptor parser in v1**: the boot protocol covers keyboards
  and mice with a fixed, measured format. Deferred.
- **The board's separate EHCI controller**: it does not serve the USB-A ports. Not used.

**Prior art** (described generically): a production-grade capability OS drives xHCI from a
userspace driver with per-interrupter event rings and completion-driven asynchronous transfers,
choosing its interrupt moderation from a scheduler trace and polling only during reset; a research
Rust microkernel runs xHCI as a userspace daemon that receives the IRQ as an event and wakes the
waiters of specific TRBs, with ring buffers to its class drivers; classic boot-protocol mouse
drivers read the fourth byte as the wheel — as the measured mouse sends it.

## Open questions

- **The report-protocol parser** (touchpads, consumer keys, gamepads): a follow-up task after
  Block 2, behind a bounded HID descriptor parser in `userspace/hid`.
- **Multi-TT**: v1 runs the hub single-TT (alternate setting 0); multi-TT once two full-speed
  devices on one hub contend for bandwidth (measure first).
- **IMOD**: 1 ms in v1; the board's interrupt-rate and latency counters decide (Phase 3).
- **A shared-memory report ring to the client** (instead of IPC frames) when a bulk class (mass
  storage) arrives; HID's reports are a few bytes.
- **The other push channels** (the settings and session watches) predate the declared push pair
  (`pin_declared_waits`) and are minted by bespoke init code; moving them onto the declaration is
  a follow-up of their own, not this contract's.

---

## Implementation Checklist

- [x] **Phase 0**: contract seed, measurements, HID boot parsers on the measured reports — proof:
  `cargo test -p input_v1_0_host --test hid_contract`, `cargo test -p hidrawd --test contract`
- [x] **Phase 1**: xhcid + nexus-usb on QEMU — proof: `just test-os usb`, `just test-all`
  (2026-10-05: QEMU's xHCI 1.00, a full-speed hub on root port 5, a boot keyboard and mouse;
  host: nexus-usb 19 tests, xhcid 14 against the model, 14 of 16 mutants killed — the two
  survivors are the electrical waits the model cannot express)
- [x] **Phase 2**: hidrawd sources, USB input moves the desktop — proof: `just test-os usb-visible`
  (2026-10-05: the desktop over USB alone — no virtio input device; `SELFTEST: ui v2 input ok`,
  the keyboard route, chain contract incl. `input-live` 15/15, pixel proof; host: xhcid's class
  server 10 tests over the real core, hidrawd's USB source 9, `nexus_wire::usb` 7)
- [x] **Phase 3** ✅ 2026-10-06: the board — `[PASS] board-visible` with `inputd: live pointer route on` / `… keyboard route on` + `board-visual: typed` / `pointer` (cycle 20)
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass (`USB_MARKERS`,
  `USB_CLASS_MARKERS`, `USB_INPUT_MARKERS`; the board's in Phase 3).
- [x] Security-relevant negative tests exist (`test_reject_*`) — Phase 0's parsers; Phase 1 adds the descriptor rejects.
