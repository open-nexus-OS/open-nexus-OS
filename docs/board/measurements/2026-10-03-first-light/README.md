# 2026-10-03 — first light: gpud's boot splash through the display controller and the HDMI encoder

TASK-0251 P2a step 1 (D4 of the hardware fast track). Two board cycles: the first scanned
nothing and the gate named it; the measurements between them — the stock system's live pad
words, the vendor boot loader's own console, the stock dump read for the composer — turned
the guess into gates; the second cycle passed every gate and the monitor showed the splash.

## Question

What does our chain need, beyond D3's glue (domain 7, `hdmi_reset`, `hmclk`), for the display
controller to scan a framebuffer of ours through the HDMI encoder onto the monitor — and how do
we know each part works before we look at the monitor?

## Instrument

- **Our chain** (build dev-f330 / dev-029f): gpud's display path (`source/drivers/gpud/src/
  backend/dc/`) prints a line per step; on failure the controller's live words (`gpud: dc census
  controller+…`), always the encoder's (`gpud: dc census encoder+0x0: …`); every controller word
  it wrote, read back (`gpud: dc readback …`); socd's bring-up lines name every register they
  touched before and after (`pinctrl+1ec:d040>d041`).
- **M1 — the stock system's pads**, read live through its pinctrl debugfs (`stock-pinctrl-pins.txt`,
  read-only, over adb): the pad register and word of every pin the encoder, uart0, mmc1 and the
  status LED use, and the device that claims it.
- **M2 — the operator**: the stock boot shows its logo on this monitor (the boot loader's splash
  `bianbu.bmp`, then the kernel's); our chain had shown nothing.
- **M3 — the vendor boot loader's own console**, `fastboot oem log` in the flash session
  (`vendor-bootloader-oem-log.txt`, `scripts/board-flash.sh` captures it every session now;
  the board's serial number redacted).
- **References** (names and order only, never ported): the boot loader's display drivers and
  the vendor kernel's HDMI driver (its video path writes nothing beyond the boot loader's
  sequence; its infoframe and timing functions are empty; its DDC uses 0x0..0x18).

## Results

### Cycle 1 (dev-f330, `board-boot-2026-10-03-cycle1-layer-reads-rdma3.txt`) — no signal

```
socd: bring-up /soc/multimedia-bus/display@c0440000 ok (… writes=5) apmu+0x3f4:0x0>0x11 …
gpud: dc glue ok (controller + encoder up through socd)
gpud: dc controller ok (version=0x03001030)
gpud: dc encoder ok (hpd=0 pll=locked)
gpud: FAIL dc scanout (line counter stuck at 0)
```

The monitor said "no signal". **Root cause, from the stock dump:** the composer's layer word
names the RDMA channel it reads (bit 0 enable, the bits above the channel): the stock kernel's
layer 7 holds `0x3` at 0x4d18 (channel 1), the boot loader's layer 0 `7` at 0x4c38 (channel 3,
the one it feeds). Our model wrote the boot loader's `7` while feeding the plane through channel
1 (the stock kernel's) — the composer waited on a channel nobody ran. Fixed: the layer word is
computed from the channel; `test_reject_a_layer_reading_a_channel_the_control_word_does_not_enable`
(red on the old constant). `hpd=0`: nobody muxed the encoder's pads.

### M1 — the pads (live, stock system)

| register | pin index | word | claimed by |
|---|---|---|---|
| 0xd401e1ec, 0xd401e1f0 | 123, 124 | 0xd041 (function 1, pull-up) | the HDMI encoder, `hdmi_0_grp` |
| 0xd401e1f4, 0xd401e1f8 | 125, 126 | 0xb041 (function 1, pull-down) | the HDMI encoder |
| 0xd401e1e0 | 120 | 0xb041 | GPIO 96, `sys-led` |
| 0xd401e114 / 0xd401e118 | 69 / 70 | 0xd042 | uart0 |
| 0xd401e1b8 / 0xd401e1bc | 110 / 111 | 0xc440 | mmc1 |

With the stock GPIO controller's `gpio-ranges` this fixes the pad map (pin = GPIO number):
0..85 → index pin+1, 86..92 → +37, 93..97 → +24, 104..110 → +6, 111..127 → +20 (98..103 not
measured). The encoder's pads are pins 86..89. **The tree's status-LED pad was wrong**: it
named `0xd401e180` (pinctrl-single's pin INDEX 96, an unclaimed pad) instead of GPIO 96's
`0xd401e1e0` — corrected (the LED ladder ran on the corrected pad in cycle 2).

### M3 — what the boot loader finds (download-mode session, 2026-10-03)

```
Found device 'hdmi@c0400500', …
hdmi_phy_wait_for_hpd() hdmi get hpd signal
fb=7f700000, size=1920x1080
```

With its pads muxed it sees the hot-plug and brings the display up with a framebuffer at CPU
0x7f700000 in bank 0 — its bus address, since bank 0 is identical on the controller's bus (the
corrected reading of `dma-ranges`, `../2026-09-29-display-regs/` "Reach").

### Cycle 2 (dev-029f, `board-boot-2026-10-03-cycle2-first-light.txt`) — the splash on the monitor

```
socd: bring-up /soc/hdmi@c0400500 ok (… pads=4 writes=4) … pinctrl+1ec:d040>d041 pinctrl+1f0:d040>d041 pinctrl+1f4:b040>b041 pinctrl+1f8:b040>b041
gpud: dc controller ok (version=0x03001030)
gpud: display mode 1920x1080 (maximum)
gpud: dc encoder ok (hpd=1 pll=locked)
gpud: dc readback differs (1 of 28): 4c10=ff0000/0
gpud: dc scanout ok (1920x1080@60 cea bus=0x8fd2000 vsync)
board-visual: splash
```

| gate | result |
|---|---|
| pads = the live words | the reset left function 0 (`d040`/`b040`); socd set function 1: `d041`, `d041`, `b041`, `b041` — the stock words exactly |
| hot-plug, PLL | `hpd=1`, locked |
| read-back | 27 of 28 words; `0x4c10` reads 0 — the boot loader's `0xff0000` lies outside the composer's 12-bit blue field (the vendor header: R/G/B 12 bits at 0x08/0x0c/0x10, A 8 bits at 0x14); the model no longer writes it |
| scan | raw vsync raised; the post-processing line counter did NOT move — it counts only with the post-processing in use (the stock kernel dithers, the first-light path does not); the second witness was needed |
| the monitor | gpud's boot splash (the background and the wordmark), acknowledged by the operator |
| the ladder | `[PASS] board-headless`, 20 rungs |

The framebuffer: one contiguous block for the controller at bus/CPU 0x8000000 (bank 0); the
display plane 0x8fd2000 = base + 2·1080 rows · 7680 bytes.

## Verdict

1. **First light is proven on the board** — the measured minimal sequence (the boot loader's
   order; every final word a measured one), our pads, our framebuffer, our controller model.
2. **The gates stay, sharpened by what they showed:** pads = live words (via socd's words), `hpd=1`
   (a ladder rung), the scan by EITHER witness (the counter alone was a false negative), the
   read-back (observed; it found a real model error — a rung once a cycle shows `readback ok`).
3. **Next (P2a step 2): windowd's desktop on this path** — the framebuffer grant on the board is
   this contiguous block; one backend abstraction in gpud's service loop; the CPU command
   executor shared with the virtio 2D path; present = Zicbom clean of the damage.
4. **Open measurements:** the pad drive-strength tables per IO domain (the pad step leaves the
   field as found: the encoder's reset value `4` matched the stock word); pins 98..103's order.
