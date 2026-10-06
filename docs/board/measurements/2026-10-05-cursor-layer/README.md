# 2026-10-05 — the pointer is a layer of the display controller (measured on the stock system)

Why: on our controller path (`gpud` `backend/dc`, D4 step 2) the pointer is a software sprite
blended into every present (`CURSOR_REPLY_SW`): every pointer move is a present through the CPU
executor and the cache clean — `windowd: loop hz=14 apply=9 present=8` on the board (TASK-0328
U3 cycle 10). On the virtio path the pointer is a hardware overlay (`OP_MOVE_CURSOR` on the
cursor queue, no present). Two implementations of one thing, and the slow one runs on the
hardware. The end state is one path: the controller's own layer. This measurement says how the
stock system does it.

## Instrument

Read-only, over adb as root, while the stock desktop showed its pointer on the HDMI monitor:
`regdump.py` (one 32-bit load per word, `../2026-10-04-usb-topology/regdump.py`) over the
controller window `0xc0440000 + 0x2a000` (`dpu-regs-pointer-on.txt`), and the DRM state and
framebuffer lists from debugfs (`/sys/kernel/debug/dri/1/{state,framebuffer}`:
`drm-state-pointer-on.txt`, `drm-framebuffers-pointer-on.txt`). `dpu-diff-pointer-on-vs-d0.txt`
is every word that differs from the D0 dump (`../2026-09-29-display-regs/dpu-regs-on.bin`, the
same desktop without a pointer plane), annotated with D0's field names where it had them.

## Results

**DRM**: `plane-1` (zpos 1) carries framebuffer 137: `AR24` (ARGB8888, little-endian), 64×64,
pitch 256, 16384 bytes, allocated by the KMS thread, `crtc-pos=64x64+1725+971`. `plane-0` is
the desktop (`XR24` 1920×1080, pitch 7680). Planes 2..7 idle.

**The controller** (the diff, read with D0's field names):

| Block | Word | D0 → pointer on | Reading |
|---|---|---|---|
| `DPU_CTL` | `0x560` | `0x00040002` → `0x00040006` | `ctl2_nml_rch_en` 2 → 6: RDMA channels 1 **and 2** run |
| `DPU_CTL` | `0x658` | `0x00020002` → `0x00060006` | the same pair in the control's second word |
| `RDMA_PATH2` (`0xc80`) | `0xcb8` | `0x05a005a0` → `0x00000100` | stride 256 (`rdma_stride0_layer0`) |
| | `0xcbc` | `0x0a0005a0` → `0x00400040` | 64 × 64 (`img_width_ly0`, `img_height_ly0`) |
| | `0xcc4` | `0x09ff059f` → `0x003f003f` | bbox end 63, 63 |
| | `0xca0`, `0xca4` | 0 → `0x10000000`, `0x00000001` | the buffer's bus address, low/high (`0x1_1000_0000`: bank 1) |
| | `0xcf0` | 0 → `0x4` | the pixel format (`+0x70`, where the desktop's channel reads 8 = XRGB8888): **4 = ARGB8888** |
| | `0xcf8` | `0x00000440` → `0x10000008` | the channel's memory word after the format (`fbc_mem_size` in the dump's names; the desktop's holds 0x570): the 64×64 buffer's with bit 28 |
| | `0xc80` | `0x381e00fc` → `0x001e203c` | the channel's burst/outstanding word for a small layer |
| | `0xc84` | 0 → `0x3cb` | 971 (the layer's y on screen, repeated here) |
| | `0xd18`, `0xd1c` | `0x00550000`, `0x00ff00aa` → 0 | the per-layer alpha ramps of D0 cleared |
| | `0xd4c` | `0x00200000` → `0x0020400c` | unknown (channel enable/mode bits) |
| | `0xd60`..`0xd7c` | 0 → 1 ×8 | unknown (eight words set to 1) |
| `CMPS2` (`0x4c00`) | `0x4cf8` | 0 → `0x5` | layer 6 enabled, `layer_id` 2 (= `1 | 2 << 1`: reads RDMA channel 2 — the same encoding as layer 7's `0x3` for RDMA1) |
| | `0x4d08` | 0 → `0x0006bd00` | the rectangle's left: 1725 = `0x6bd`, packed at bits 23:8 (hypothesis — a second position pins it) |
| | `0x4d0c` | 0 → `0x06fc03cb` | right 1788 (bits 31:16), top 971 (bits 15:0) — right = left + 63 |
| | `0x4d10` | 0 → `0x0005040a` | bottom 1034 (bits 15:0) = top + 63, `blend_mode` 5 (the desktop layer 7 runs 1) |
| | `0x4d14` | 0 → `0x00ff000a` | `layer_alpha` 255, `alpha_factor` 10 (layer 7: 55) |
| `OUTCTRL_TOP2` | `0x180c8`, `0x180cc` | line/pixel counters | moving (the scanout runs) |
| `MMU` | `0x169c`, `0x16c0`, `0x16c8` | counters | moving |
| `DPU_INTP` | `0x950`.. | interrupt raw bits | moving |
| | `0x808`, `0x840`, `0x1880`..`0x18b0` | 0 → `0x2ff47000`, `0x3cb00`, … | the command-list / reserved-region words the stock driver uses (`dpu_reserved@2ff40000`); not needed for a direct write |

**A second sample** (later the same day, the pointer at `+1725+970` per DRM): `0x4cf8 = 0x5`,
`0x4d08 = 0x0006bd00` (left 1725 again — `left << 8` holds, though the same x), `0x4d0c =
0x06fc03ca` (top 970 in the low half: confirmed), `0x4d10 = 0x00050409` (bottom 1033:
confirmed), `0x4d14 = 0x00ff0009` — the low byte read 10 before and 9 now: not a fixed
"alpha factor" but a changing value (a sequence or fence by the look of it); the layer alpha
255 stays. The driver writes 10 (the first sample) until a cycle says otherwise.

**The vendor header as a name reference** (the BSP kernel's composer and channel register
headers, read 2026-10-05 for names and bit positions only, nothing ported): the composer layer
block holds `en` [0], `layer_id` [4:1], `solid_en` [5]; `rect_ltopx` [23:8] of the word at
+0x10 (so `left << 8` is the field, not a quirk); `rect_ltopy` [15:0] and `rect_rbotx` [31:16]
of the next word; `rect_rboty` [15:0], `blend_mode` [17:16], `alpha_sel` [18] of the one after;
`alpha_factor` [7:0] and `layer_alpha` [23:16] of the last. The channel's word at +0x04 is
`compsr_y_offset` — the composer row the channel's first line lands on: the measured `0xc84 =
0x3cb` (971) is the layer's top, and board cycle 12 (which left it at 0) armed the layer and
showed nothing. The control block's `0x658` is `cmdlist_rch_act`, the active channels — a
status mirror of the enable word, not written.

**Z-order** (board cycle 13): our first light put the desktop on composer layer 0 (the boot
loader's splash layer) and the pointer on layer 6 — armed, running, read back, and invisible:
a lower layer index composites ABOVE a higher one (the stock desktop is layer 7 under the
pointer's layer 6). The desktop now runs on layer 7 with the stock's words, layer 0 is switched
off at the bring-up.

**Reading**: a pointer is one more RDMA channel (2) feeding one more composer layer (6) with a
64×64 ARGB8888 buffer, positioned by the layer's rectangle; blend mode 5 with an alpha factor
of 10 composites it over layer 7. A move is the rectangle's three words (left, right/top,
bottom) — and whatever latch the stock driver uses to commit a frame (our `dc::flip` latches
the address; the rectangle's latch is the open question for the first cycle).

## Decisions (TASK-0251 P2a step 3c — the controller's cursor layer)

1. `gpud`'s controller path arms the pointer as a hardware overlay: `upload_cursor` stores the
   sprite (ARGB8888, 64×64, premultiplied as the proto says) in a device buffer of its own,
   programs RDMA channel 2 (stride 256, 64×64, format 8 + bit 28, the address), enables composer
   layer 6 on channel 2 with blend mode 5 / alpha factor 10 / layer alpha 255, replies
   `CURSOR_REPLY_HW`; `move_cursor` writes the rectangle (left, right/top, bottom) and the
   latch — no present. Shape changes swap the buffer's contents (the shape cache) and latch.
2. The software cursor on the controller path (`CURSOR_REPLY_SW` from `dc`, `BlendCursor` in its
   presents) is deleted in the same package, with a retired-name gate; the CPU executor keeps
   `BlendCursor` only for the virtio 2D path, if that path still answers SW (to be checked —
   one path per backend, no fallback that hides a dead overlay).
3. Gates for the first cycle, measured: the stock words above read back from our registers
   (`gpud: dc cursor layer ok (rdma=2 layer=6 fmt=0x10000008 blend=5)`), `windowd: cursor
   move visible` with `windowd: loop hz` under mouse movement **not** rising with the pointer
   rate (a move is no present), and the operator's `board-visual: pointer` (the arrow follows
   the mouse). A second stock sample at another pointer position pins the left word's packing
   before the first write.
