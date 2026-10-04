# 2026-10-04 — USB on the board, measured on the stock system (TASK-0328 U0, R8)

Read-only, over adb from the stock system (the live tree, the probe log, the device tree of
attached devices, and the registers through `regdump.py` — one 32-bit load per word, nothing
written).

## Question

What must our USB host stack do on this board that QEMU's `qemu-xhci` does not show: which
controller, which PHYs, which glue, what sits between the root port and a keyboard, what powers
it — and which register state does the stock system leave behind?

## Instrument

`/sys/firmware/fdt` → `stock-usb-nodes.dts`; `dmesg` (`stock-dmesg-usb.txt`); `lsusb -t` and
`/sys/bus/usb/devices` (`stock-lsusb-tree.txt`, `stock-usb-devices.txt`); `regdump.py` on the
xHCI capability/operational/port/extended-capability words, the DWC3 globals, the APMU glue
word, the USB2 PHY, the combo PHY and the GPIO bank of the hub's lines (`stock-usb-regs.txt`).

## Results

**The tree** (`stock-usb-nodes.dts`):
- `usb3@0` `spacemit,k1-x-dwc3` — the glue: `reg = <0xd4282bc8 4>` (one APMU word), reset
  `ctl_rst` (0x4d), clock `usbdrd30` (0x8e), power domain 0, `phys = <&combphy 4>`
  (`usb3-phy`), `usb-phy = <&usb2phy>`, glue interrupt 149; its child `dwc3@c0a00000`
  `snps,dwc3`: `reg 0xc0a00000+0x10000`, interrupt 125, `dr_mode = "host"`, `phy_type = "utmi"`.
- `usb2phy@0xc0a30000` `spacemit,usb2-phy` (`reg +0x200`, clock `usbdrd30`).
- `phy@c0b10000` `spacemit,k1x-combphy`: `puphy` 0xc0b10000+0x800 and `phy_sel`
  0xd4282910+0x400, reset `phy_rst` (0x4c) — the SuperSpeed lane.
- `usb3hub@0` `spacemit,usb3-hub`: `hub-gpios = <&gpio 123 0>, <&gpio 124 0>`,
  `vbus-gpios = <&gpio 97 0>`, `vbus_delay_ms = <200>`, power domain 8 — **the USB-A ports sit
  behind an on-board hub whose power and reset are GPIOs.**

**The topology** (`lsusb -t`): one xHCI with TWO root ports — port 1 USB 2.0, port 2 USB 3.0
(the Supported-Protocol capabilities at 0x890 / 0x8a0). Port 1: the on-board VIA hub
`2109:2817` (HS, 5 ports; its SS twin `2109:0817` on port 2) → port 4: a Genesys hub
`05e3:0608` (HS) → port 3: the attached device `28bd:092d` (full speed, interfaces 0 and 1 HID
boot-protocol mouse, interface 2 a generic HID digitizer). A keyboard and a mouse will sit at
least one hub deep, at full or low speed behind a high-speed hub.

**The registers** (`stock-usb-regs.txt`, stock Linux running, host mode):
- xHCI: version 1.10, CAPLENGTH 0x20, `HCSPARAMS1 = 0x02000140` (64 slots, 1 interrupter, 2
  ports), `HCSPARAMS2 = 0x0c0000f1` (**1 scratchpad buffer**, scratchpad restore), `HCCPARAMS1 =
  0x0220fe6d` (64-bit addresses, **64-byte contexts** (CSZ=1), port power control, extended
  capabilities at 0x880), DBOFF 0x480, RTSOFF 0x440; USBCMD running, PAGESIZE 4 KiB, CONFIG 64
  slots; PORTSC1 `0xe03` (connected, enabled, high speed), PORTSC2 `0x0c001263` (SuperSpeed hub
  connected, U3).
- DWC3 3.30a (`GSNPSID 0x5533330a`), `GCTL 0x32c91004` (port capability = host),
  `GUSB2PHYCFG 0x40102400`, `GUSB3PIPECTL 0x01080003`.
- APMU glue word `0xd4282bc8 = 0x0b008000`.
- GPIO bank 3 (`0xd4019100`): GPLR `0x38184002`, GPDR `0x981ac003` — lines 97, 123 and 124
  are outputs driven high; pads: 97 `0xa041` (mux 1), 123 `0xa040` and 124 `0xd040` (mux 0).

## Verdict (decisions for U1–U3)

1. **Hubs are not optional.** The board's only USB 2.0 root port carries the on-board hub; the
   host stack must enumerate through hubs (hub class: port power, reset, status) and program the
   transaction translator of a full/low-speed device behind a high-speed hub in its slot context
   (parent hub slot, parent port, multi-TT) — QEMU's `usb-kbd` on a root port would never show
   it, so the QEMU lane puts the HID devices behind a `usb-hub`.
2. **Context size from HCCPARAMS1**: 64-byte contexts here; the driver reads CSZ (QEMU's
   controller may report 32). **Scratchpad buffers** from HCSPARAMS2 (one here).
3. **Hub power is GPIO**: the `spacemit,usb3-hub` node's lines driven high after the pads are
   muxed (97 → 1, 123/124 → 0), VBUS after `vbus_delay_ms`. Who drives them is U3's design
   question (socd's glue steps, or a GPIO consumer of the tree binding).
4. **The glue**: `ctl_rst`, `usbdrd30`, domain 0 through socd (RFC-0106 — the USB set), the APMU
   glue word to `0x0b008000`; the USB2 PHY's tuning words as the stock driver leaves them
   (diffable against our chain's first board cycle, the D3 method); v1 serves HS/FS/LS through
   the USB 2.0 root port — the SuperSpeed lane (combo PHY) is not needed for HID.
5. **What to measure next, with a keyboard and a mouse attached**: their ids, speed, boot
   protocol support and position in the tree (the operator plugs them in for one more read).
