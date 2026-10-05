# Measurement recipes (board-independent)

Every recipe below was used on the reference board between 2026-09-22 and 2026-09-30 and
is written up in `docs/board/measurements/`. The stock system is reached over adb as root
(`scripts/board-log.sh` and `scripts/board-test.sh` wait for it); nothing below writes to
the hardware except where the recipe says so. Archive the raw files next to the README of
the measurement folder — the annotated dump, not the interpretation, is the evidence.

## 1. A register window (`2026-09-29-display-regs/`)

`busybox devmem <addr> 32` reads one word; a window is read with `mmap` on `/dev/mem`
and **32-bit loads** (`ctypes.c_uint32.from_buffer(m, off).value`) — slicing the mmap
copies byte-wise and the bus answers `0xffffffff`, and `dd if=/dev/mem` faults with
"Bad address" on MMIO. Read the window twice (picture on / off) and diff the words; the
words that move are the ones the driver touches. Keep the raw `.bin` files.

```
python3 rd.py 0xc0440000 0x2a000 /tmp/dpu.bin      # the reader is 15 lines, in the README
```

Read the window while the stock driver shows a STILL picture at the mode you want, and
note the mode from `/sys/kernel/debug/dri/*/state` (mode line, framebuffer format,
pitch) in the same README.

## 2. Clock tree, regmap, power domain — on/off diff

`/sys/kernel/debug/clk/clk_summary` names every clock, its enable count, its rate and its
consumers: grep the block's clocks and read the ancestry (`2026-09-29-display-regs`: the
HDMI pipeline runs on one clock, the five DSI clocks are off — that trimmed the tree node).
`/sys/kernel/debug/regmap/<syscon>/registers` is the APMU as the vendor driver left it;
diff it against a changed state. Ways to change state, honest about what they are:
`fb0/blank` disables the layer only (nothing else moves — measured); a cable unplug flips
the HPD bit only (measured 2026-09-30); a driver unbind may oops the vendor driver (it did,
`2026-09-29-display-regs/oops-pinctrl-dram-fb.txt`) — so a transition the stock system never
makes is judged on OUR chain, with the words of every register before and after (socd's
bring-up marker, D3). Its register map usually needs no cycle at all: recipe 3b.

## 3. The tree: nodes, phandles, ranges

`/proc/device-tree/<path>/<prop>` are the raw cells (`xxd -p`); a phandle is resolved by
searching every `phandle` file for the value (`2026-09-29-display-regs/dt-phandles-drm-dmesg.txt`:
memory-region → the reserved pool, interconnect → the DRAM range with its `dma-ranges`,
pinctrl → the pad group with mux values). The `dma-ranges` of the bus node translate CPU
addresses to bus addresses. Read them with the cells of BOTH sides (`#address-cells` of the bus
and of its parent, then the size): `2026-09-29-display-regs` first read `<0x0 0x0 0x0 0x0 0x0
0x80000000>` as "bus 0 → CPU 0x8000_0000" and concluded bus = CPU − 0x8000_0000; the entry
says bus 0x0 → CPU 0x0 for 2 GiB (corrected 2026-10-03 from the stock tree in the pinned
archive). A captured address that is not in `iomem.txt`'s RAM is a BUS address.

## 3b. The stock tree from the pinned archive — no board cycle (`2026-09-30-power-domains/`)

The vendor's own tree often states what its driver does with a register: the power
controller lists every domain's control word, its mode/request/sleep/isolation bits and its
status bits. The pinned vendor archive (`resources/board/<board>/PROVENANCE.md`, cached by
`scripts/fetch-board-inputs.sh`) carries the boot partition; read the board variant's tree
without mounting and without root:

```
python3 -c "import zipfile,shutil,sys; z=zipfile.ZipFile(sys.argv[1]); \
  shutil.copyfileobj(z.open('bootfs.ext4'), open('bootfs.ext4','wb'))" <archive>.zip
debugfs -R "ls -l /<dtb dir>" bootfs.ext4
debugfs -R "dump /<dtb dir>/<variant>.dtb v.dtb" bootfs.ext4 && dtc -I dtb -O dts v.dtb
```

Prove it is the tree the stock system ran before trusting it: its `model` and a phandle
read live must match (`power-domains = <0x20 7>` live, phandle `0x20` = the power controller
in the file). Then check the tree's claims against the live words — the tree named the
status BITS but not the status REGISTER; the one APMU word whose bits matched all seven
switchable domains' states named it. Transcribe facts into a table; copy no tree text.

## 4. Display: DRM state and EDID

`/sys/kernel/debug/dri/<n>/state` gives the mode line (pixel clock, porches, sync widths,
polarity), the plane's format and pitch; `/sys/class/drm/<connector>/edid` the EDID,
decoded on the host with `edid-decode` (VICs, preferred mode, aspect). Keep both.

## 5. Annotating a dump with the vendor's field names

The vendor kernel's register headers are read as a NAME reference only, never ported
(the `driver-bringup` legal rule; GPL text stays out of the repo). Parse the bitfield
structs (`UINT32 name : bits;` per register word) into (block, word, lo, hi, name), map
the block bases from the vendor's register map, and annotate every live word of the dump:
`docs/board/measurements/2026-09-29-display-regs/dpu-regs-on-annotated.txt` is the
result (317 live words, timing generator, composer, DMA channels, MMU). The boot loader's
splash driver is usually the shortest first-light sequence — read it for the ORDER of
writes and the registers it does not touch (no command list, no display MMU).

## 6. The boot trace from the eMMC (RFC-0107)

Without a serial adapter the transcript of a boot is the boot trace: the loader keeps its
lines on the disk (`nxboot: trace slot=`), the kernel keeps every console byte in a
static ring that blkd writes to the same partition, and the next loader rescues the ring
of a crashed boot from RAM. `just board-log` pulls it over adb once the stock system is
back (microSD in, reset); `scripts/board-test.sh --log=<uart.log>` judges it. A warm reset
keeps this board's DRAM, a cold one does not (`2026-09-28-dram-retention/`).

## 7. The LED (`source/kernel/neuron/src/hal/boot_led.rs`)

Before the block owner runs the kernel has one channel: the user LED. By default it is a
two-state witness: lit at milestone 1, dark at milestone 11 (the runtime) — lit for good
means the kernel stopped early; a panic flickers it forever. The slow ladder (milestone k =
k/5 long and k%5 short pulses, ~22 s per boot) is a DEPRECATED diagnostic since 2026-10-04:
set `/chosen/nexus,boot-led-ladder` (an empty property) in the board tree only when a
kernel dies before the eMMC trace starts and the position matters. It found the kernel's
silent stop (2026-09-28: milestone 11, then a pending timer) that no marker could have shown.

## 8. A device in the protocol YOUR stack will use (usbfs)

The stock system drives a USB HID device in the report protocol; our stack uses the boot
protocol — what the device sends there is not on any stock screen. `boot_capture.py`
(`docs/board/measurements/2026-10-04-usb-boot-protocol/`) detaches the stock driver from ONE
interface over usbfs, sends exactly our requests (SET_PROTOCOL(0), SET_IDLE(0)), reads the
interrupt endpoint for a bounded time, restores the report protocol and hands the interface back.
Dry-run it first (no input), then capture while the operator acts. It found the 4-byte mouse
reports, the STALLed SET_IDLE and the stale report-format first frame (2026-10-04).
**Privacy: never archive what a person types** — warn BEFORE a keyboard capture ("test keys
only, no password"), analyse without printing key codes, delete raw copies, keep aggregates.

## 9. Reading a transcript honestly

Before assuming a marker was never printed, search for it TORN: on a multi-hart board two
writers interleaved byte by byte until 2026-09-29 (`grep -E 's.?t.?a.?g.?e.?:.? .?p…'`
found `stage: platform` woven into a fault dump). Compare with the same lane on QEMU
line for line; `KINIT: mm frames`, `metricsd: mm snapshot` and `gpud: present us` give the
numbers for the ledger.
