# 2026-10-04 — what the desk's keyboard and mouse send in the boot protocol (TASK-0328 U0)

Read on the stock system over adb as root, after `../2026-10-04-usb-topology/` had placed the
devices: the keyboard `3434:0123` on port 2 and the mouse's radio receiver `046d:c53f` on port 3
of the on-board hub `2109:2817`, both full speed.

## Question

Our host stack will drive HID devices in the **boot protocol** (SET_PROTOCOL(0)), so the parser in
`userspace/hid` sees boot reports — but the stock system drives them in the report protocol, so
nothing the stock system shows says what these devices send in boot mode. What are the report
lengths, the button bits, the wheel, the keyboard's reserved byte and its behaviour past six keys;
does each boot interface accept SET_PROTOCOL and SET_IDLE; and what do the endpoints and the hub
look like to a host stack (max packet, interval, TT)?

## Instrument

1. `/sys/bus/usb/devices/*/ep_*` and the devices' `bMaxPacketSize0` (endpoint parameters).
2. `lsusb -v -d 2109:2817` — the hub's class descriptor and alternate settings.
3. `/sys/bus/hid/devices/*/report_descriptor` — the report-protocol layout of every HID interface.
4. `boot_capture.py` (this folder) over usbfs: detach the stock HID driver from ONE interface,
   SET_PROTOCOL(0) and SET_IDLE(0) exactly as our stack will, GET_PROTOCOL, read the interrupt-IN
   endpoint for a bounded time, then SET_PROTOCOL(1) and hand the interface back. Dry runs first
   (no input), then one 25 s capture of the receiver's mouse interface and the keyboard's boot
   interface in parallel while the operator moved, clicked, scrolled and typed.

**Privacy:** what the operator typed is not archived. The keyboard capture was analysed and
deleted (on the board and on the host); only the aggregate facts below remain. While an interface
is claimed the stock system receives nothing from it, so input outside the 25 s window never
reached the script.

## Results

**Endpoints** (sysfs):

| interface | role | endpoint | max packet | bInterval | period |
|---|---|---|---|---|---|
| hub 2-1:1.0 | status change | 0x81 IN | 1 | 12 (HS) | 256 ms |
| keyboard 2-1.2:1.0 | boot keyboard (3/1/1) | 0x81 IN | 8 | 1 | 1 ms |
| receiver 2-1.3:1.0 | boot keyboard (3/1/1) | 0x81 IN | **12** | 1 | 1 ms |
| receiver 2-1.3:1.1 | boot mouse (3/1/2) | 0x82 IN | **32** | 1 | 1 ms |

EP0 max packet: hub 64, keyboard 64, **receiver 32** (a host must read it before trusting 64).

**The hub** (class descriptor): 5 ports, per-port power switching and over-current protection,
**TT think time 32 FS bit times** (the slot context's TTT = 3), port indicators,
**bPwrOn2PwrGood 175 × 2 ms = 350 ms**, DeviceRemovable 0x20. Device protocol 2 ("TT per port");
alternate setting 0 = single TT, 1 = multi TT (the stock system selects 1).

**Report descriptors** (report protocol, for contrast): the receiver's mouse interface sends
report id 2 with 16 button bits, 16-bit X/Y, an 8-bit wheel and an 8-bit pan (nine bytes with the
id), plus report ids 3, 4 and 8 on the same interface; its keyboard interface has ten key slots
(12 bytes). The keyboard's boot interface is the standard 8-byte layout without report ids.

**Boot protocol** (`boot_capture.py`):
- Every boot interface accepted SET_PROTOCOL(0); GET_PROTOCOL read 0 afterwards.
- SET_IDLE(0): accepted by both keyboard interfaces, **refused with a STALL by the receiver's
  mouse interface** (SET_IDLE is optional for a mouse) — EP0 stalls and must be recovered.
- **Mouse: 4170 reports in 25 s; 4169 are four bytes** — buttons, dx, dy, wheel. Button bits seen:
  bit 0 (left), bit 1 (right), **bit 3 and bit 4 (the thumb buttons)**; the wheel byte read ±1
  per notch; dx/dy within −86..127. The median gap between reports while moving was 1.0 ms
  (minimum 0.5 ms): **~1000 reports/s from one mouse**.
- **The first report after the switch was still in the report format**: nine bytes,
  `02 00 00 03 00 00 00 00 00` (report id 2, x = 3) — queued while the stock driver polled. As a
  boot report it would read as a right click and a scroll of 3.
- Keyboard: 114 reports, **all eight bytes, the reserved byte 0 in every one**; with more than six
  keys held the keyboard reported six and dropped the rest — **no ErrorRollOver** was sent; with
  SET_IDLE(0) a held key sends no repeats.

## Verdict (decisions for U1–U3)

1. **The boot mouse parser takes 3..=8 bytes**: buttons, dx, dy, and the wheel in byte 3 when
   present; all eight button bits are buttons (left, right, middle, side, extra, forward, back,
   task). A frame longer than eight bytes is another format and is refused — which also refuses the
   stale report-format frame after the protocol switch. The old parser (exactly three bytes, bits
   3..7 rejected) would have dropped **every** report of this mouse.
2. **The keyboard parser keeps eight bytes**, ignores the reserved byte (the device's own) and
   holds the key state on the error usages 0x01..=0x03 (another keyboard's answer to too many keys;
   this one drops them instead).
3. **SET_IDLE is best effort**: a STALL on EP0 is recovered (Reset Endpoint, then the next
   control transfer) and the interface is used anyway.
4. **EP0's max packet is read from the first eight bytes of the device descriptor** before the
   rest (32 on the receiver).
5. **TT**: both HID devices are full speed behind a high-speed hub — their slot contexts carry
   the hub's slot and port; the hub's slot context carries TTT = 3 (32 FS bit times) and five
   ports; v1 leaves the hub at alternate setting 0 (single TT, MTT = 0). Port power needs 350 ms
   (bPwrOn2PwrGood) before a port is reset.
6. **Interrupt pacing**: one mouse alone completes ~1000 transfers/s; the controller's interrupt
   moderation bounds the interrupt rate, and four TRBs queued per HID endpoint cover several
   milliseconds of scheduling delay (RFC-0099).
