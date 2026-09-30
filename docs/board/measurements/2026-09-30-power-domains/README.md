# 2026-09-30 — the power domains' protocol, read from the stock system's own tree (D3)

The measurement half of TASK-0245B P3 (power domains, display set). D0
(`../2026-09-29-display-regs/`) found the HDMI pipeline in power domain 7 and could not watch the
domain switch: the stock kernel never drops it while it runs, and unbinding its display driver
oopsed. This folder answers the question from a source that needs no board cycle — the tree the
stock system boots with — and cross-checks it against the words D0 read live.

## Question

Which register switches domain 7 on, which bit reports it on, what does the stock system's
steady state look like — and what will the first cycle of our own chain decide that this
reading cannot?

## Instrument

- **The stock system's tree, as its boot partition ships it.** The pinned vendor archive
  (`resources/board/bpi-f3/PROVENANCE.md`, archive SHA-256 `36450812…9bed`) carries the boot
  partition `bootfs.ext4` (SHA-256 `eff9a32e…0bcf`); the board variant's tree in it is
  `k1-x_deb1.dtb` (kernel 6.6.63, SHA-256 `cc801ecc…7674`). It is the tree the stock system ran
  on 2026-09-22 and 2026-09-29: its `model` is the one the 2026-09-22 README records, and its
  power controller carries phandle `0x20` — the phandle D0 read live in both display nodes
  (`power-domains = <0x20 7>`, `../2026-09-29-display-regs/dt-stock-display-nodes.txt`). Read
  without mounting and without root:
  ```
  python3 -c "import zipfile,shutil,sys; z=zipfile.ZipFile(sys.argv[1]); \
    shutil.copyfileobj(z.open('bootfs.ext4'), open('bootfs.ext4','wb'))" <archive>.zip
  debugfs -R "ls -l /<dtb dir>" bootfs.ext4  # the variants
  debugfs -R "dump /<dtb dir>/k1-x_deb1.dtb k1-x_deb1.dtb" bootfs.ext4
  dtc -I dtb -O dts k1-x_deb1.dtb            # the power controller: /soc/power-management@0
  ```
  The tables below transcribe facts (offsets and bit numbers); no text of the tree is copied.
- **The live APMU words** (window `0xd428_2800`, 0x400): D0's capture at 1080p60
  (`../2026-09-29-display-regs/apmu-on.txt`), with the layer blanked (`apmu-off.txt`), and the
  2026-09-22 stock system (`../2026-09-22-stock-system/regmap-apmu.txt`).

## Results

### The power controller (compatible `spacemit,power-controller`, nine domains)

Every domain's control register is an APMU offset. "hw" = the tree marks the domain
hardware-sequenced (`use_hw`): a mode bit hands it to the power sequencer, a request bit asks
the sequencer to power it up; "sw" = the driver walks isolation and the two sleep bits itself.

| id | domain | control | mode | mode / request bits | sleep2 / sleep1 / isolation | status bits (sw / hw) | live control | live state |
|---|---|---|---|---|---|---|---|---|
| 0 | BUS | — | always on | — | — | — | — | on |
| 1 | VPU | 0x0a8 | sw | — | 3 / 2 / 1 | 1 / 9 | 0x0 | off |
| 2 | GPU | 0x0d0 | sw | — | 3 / 2 / 1 | 0 / — | 0xe | on |
| 3 | LCD | 0x380 | hw | 4 / 0 | 3 / 2 / 1 | 4 / 12 | 0x0 | off |
| 4 | ISP | 0x37c | sw (hw bits named, no `use_hw`) | 4 / 0 | 3 / 2 / 1 | 2 / 10 | 0x0 | off |
| 5 | AUDIO | 0x378 | hw | 4 / 0 | 3 / 2 / 1 | 3 / 11 | 0x11 | on |
| 6 | GNSS | 0x13c | sw (hw bits named, no `use_hw`) | 4 / 0 | 3 / 2 / 1 | 6 / 14 | 0x0 | off |
| **7** | **HDMI** | **0x3f4** | **hw** | **4 / 0** | 3 / 2 / 1 | **7 / 15** | **0x11** | **on** |
| 8 | (placeholder) | — | — | — | — | — | — | — |

Domain 7 in the stock steady state: control `0x3f4 = 0x11` — the mode bit (4) and the request
bit (0) set, nothing else. The same word in all three captures.

### The status word

The tree names the status bits, not the register that holds them. APMU `0x0f0` reads `0x8009`
in all three captures — bits 0, 3 and 15. Against the table: every domain that is on has one
of its status bits set (GPU sw bit 0, AUDIO sw bit 3, HDMI hw bit 15), every domain that is
off has neither (VPU 1/9, LCD 4/12, ISP 2/10, GNSS 6/14) — seven of seven consistent, and no
other APMU word is. **`0x0f0` is the power status register; domain 7 reports "on" in bit 15.**
This is an inference from one steady state, so hypothesis H2 below checks it on our chain.

### The rate the pipeline runs at

`hmclk` (APMU `0x1b8`) reads `0x01040321` with the picture on: gate bit 0, mux 1 (pll1_d5
491.52 MHz), divider 0, reset released (bit 9) — and bits 8, 18 and 24, which no table names.
The stock driver sets the rate itself; nothing in the stock tree states it. Our chain never ran
the stock display path, so `0x1b8` holds whatever the reset and the first-stage loader leave.

### The encoder's pads (for TASK-0251 P2b)

`hdmi_0_grp` (pinctrl-single cells: offset, function, configuration): `0x1ec` and `0x1f0`
function 1 configuration `0xd040`, `0x1f4` and `0x1f8` function 1 configuration `0xb040` — the
encoder's DDC and status lines. The first light does not need them (no EDID); the DDC does.
Our tree names pads by pad number, and the table maps a pad number to its offset: that mapping
for these four pads is measured with its consumer (D4 P2b), not guessed here.

## Hypotheses one cycle with the display nodes decides

The instrument is socd's bring-up marker: it names every register the bring-up touched with
its word before and after (`apmu+0x3f4:0x…>0x…`), so one cycle decides all four. The cycle ran
the same day with a board probe (the selftest client asks socd for every display node the tree
names; `SELFTEST: soc glue display ok` is a rung of the board ladder) — results below.

- **H1 — the sequence.** Domain 7 comes up when the mode bit is set with the request low, then
  the request raised, and `0x0f0` bit 15 follows within the poll bound. Decided by `socd:
  bring-up /soc/multimedia-bus/display@c0440000 ok (…)` with `0x3f4 …>0x00000011` — or by
  `FAIL (step=domain reg=… val=…)` naming the status word it read.
- **H2 — the status word.** `0x0f0` bit 15 is domain 7's "on". Refuted if the domain is
  demonstrably up (the controller's version register at `0xc044_0000` reads `0x03001030`, as
  D0 read it) while bit 15 stays clear.
- **H3 — the rate.** The tree demands `hmclk` at 491.52 MHz (`assigned-clock-rates`, the
  measured running rate); socd selects mux 1 / divider 0 and polls the frequency-change bit
  (29). Decided by the `0x1b8` before/after words and `CLOCK_RATE` reading 491 520 000.
- **H4 — the unnamed bits.** If the pipeline is up (domain on, rate right, the controller
  answers) and no picture appears, bits 8, 18 and 24 of `0x1b8` — set in the stock state, set
  or not in the before-word — are the first suspects.

## Results on our chain (2026-09-30, build dev-42cf, `board-boot-2026-09-30T17-14-32-display-glue.txt`)

One board cycle: flash, eMMC boot, the trace read back with `just board-log`,
`[PASS] board-headless` with 18 rungs (the display rung red on the 2026-09-29 trace, green here).

```
socd: bring-up /soc/multimedia-bus/display@c0440000 ok (domains=1 resets=1 clocks=1 rates=1 writes=5)
      apmu+0x3f4:0x0>0x11 apmu+0xf0:0x8>0x8008 apmu+0x1b8:0x1040304>0x1040321
socd: bring-up /soc/hdmi@c0400500 ok (domains=1 resets=1 clocks=1 rates=1 writes=0)
      apmu+0x3f4:0x11>0x11 apmu+0xf0:0x8008>0x8008 apmu+0x1b8:0x1040321>0x1040321
SELFTEST: soc glue display ok
```

- **H1 — confirmed.** Our chain finds domain 7 off (`0x3f4 = 0`); the mode bit with the
  request low, then the request raised (two writes, `0x3f4 = 0x11`) brings it up inside the
  poll bound. No hang, no fault.
- **H2 — confirmed.** `0x0f0` goes from `0x8` to `0x8008`: bit 15 and nothing else changes,
  right after the request. Our chain has the GPU off (bit 0 clear, stock: set) and the audio
  domain's software status on (bit 3, as on the stock system) — both consistent with the table.
- **H3 — confirmed.** `0x1b8` reads `0x01040304` at reset: mux 0 (pll1_d6) and divider 2 —
  136.5 MHz — with the gate off; socd leaves it at mux 1, divider 0, gate on: 491.52 MHz. The
  frequency change completed on the running clock (the executor polls it).
- **H4 — resolved, nothing missing.** Bits 8, 18 and 24 are the register's reset state (set
  before any write of ours), and `hdmi_reset` (bit 9) comes out of reset released (the reset
  step wrote nothing). The word socd leaves equals the stock system's word exactly
  (`0x01040321`).
- The encoder shares all three and wrote nothing. Five writes in all: two for the domain, one
  gate, two for the rate.
- Also in the words: the loader leaves the eMMC's `0x0e0 = 0x112` (pll1_d6 / 2 = 204.8 MHz; the
  stock kernel runs it at 375 MHz, `0x52`) — informational, the block owner reads the rate.

## Verdict (what D3 builds)

1. **The table** (`source/libs/nexus-soc/src/table/k1.rs`): domain 7 is hardware-sequenced —
   control `0x3f4` (mode bit 4, request bit 0), status `0x0f0` bit 15; BUS stays always on;
   every other domain stays refused (GPU and VPU are software-sequenced: their consumers
   measure them).
2. **The executor** (`ops.rs`): the status first — a domain already on is left alone (no
   write); else the mode bit with the request low, the request raised, the status polled with a
   bound; a domain that never reports on is a named fault with the status word.
3. **The tree** (`config/board/bpi-f3/board.dts`): the display controller names what the stock
   tree names — `hmclk`, `hdmi_reset`, domain 7 — instead of the five DSI clocks and three DSI
   resets; both display nodes demand `hmclk` at 491.52 MHz.
4. **The instrument** (`socd`): every bring-up marker carries the words of the registers it
   touched, before and after; a failure names the step, the register and the word.
5. **The gate** (`scripts/board-test.sh`): `SELFTEST: soc glue display ok` is a rung of
   `board-headless` — the display set is up on every board boot from now on, before any display
   driver runs. TASK-0251 P2 starts from a powered, clocked, released pipeline; its first read
   is the controller's version word.
