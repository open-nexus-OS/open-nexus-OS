# Implementation Order — the execution view (hardware fast track)

**What this file is:** the ONE sequential/lane view over `tasks/TASK-*.md`. It says what is
being built now, in which order, and what is deliberately parked. It is **not** authoritative
for scope or Definition of Done — every `TASK-*.md` header is the execution truth, and
`tasks/STATUS-BOARD.md` is the only Done list (this file links, it does not duplicate).

**Rewritten 2026-09-21.** The previous order was the UI fast track; the UI has shipped (the
UI fast lane, SMP v1/v2, filesystem, reliability spine, OTA, security and Sub-80 Phase 1 lanes
are history — `tasks/STATUS-BOARD.md`, `CHANGELOG.md` and the ledgers hold them). This order is
the **hardware fast track**: the OS boots on the reference board on the desk, shows a picture,
takes USB input, finishes Sub-80, and then builds four target pictures — memory, real GPU, real
SMP, a real browser with a real network — interleaved, because they depend on each other.

## Tasks and TRACKs

- **Task** (`TASK-XXXX-*.md`): atomic work unit with proofs (host tests + QEMU markers + board
  markers) and a Definition of Done. Status: `Draft` → `In Progress` → `Done` (or `Superseded`
  / closed by decision, always with the actual solution recorded in the ledger).
- **Track** (`TRACK-*.md`): vision document; never executed directly — it spawns tasks (next
  free number) once its gates clear. Three tracks were dissolved into tasks on 2026-09-21
  (`TRACK-DRIVERS-ACCELERATORS`, `TRACK-NETWORKING-DRIVERS`, `TRACK-NETWORK-PROOF-LANES`); the
  rest link to the task that carries their extracted item.
- **Ledger reuse rule (user, 2026-09-21):** where a ledger already exists for a subject — even
  if it is outdated — it is TAKEN: its text is recut to the end state and extended with `B`/`C`
  parts where sensible. New numbers are minted only where no ledger exists. In this file:
  `↻` = existing ledger, recut at its block's P0 · `⬜` = seeded ledger (exists, Draft) ·
  `⬜ at P0` = a B/C part seeded when its block starts · ✅ = Done.
- Every task is built to the **end system**, production-grade, no interim solutions; a new
  mechanism REPLACES the old one and the old one is deleted in the same package with a gate
  against its return (user rules 2026-09-03 / 2026-09-09; see `CLAUDE.md`).
- Ledger discipline: `In Progress` when a task starts, `Done` + docs sweep (CHANGELOG, docs,
  board, RFC status) when it closes; counters on the board are recomputed mechanically from
  ledger headers, never nudged.

---

## Active now (2026-10-04)

| # | Lane | State | Next |
|---|---|---|---|
| 0 | **Block 0 — Tooling** (one command installs the flash/serial tools on Ubuntu, Arch, Fedora) | **T0–T2 ✅ 2026-09-21** (`make doctor` green, `just board-*`, pinned vendor boot pieces, `--stage-only` measured against the boot ROM: SPL 28 ms, U-Boot 51 ms, `blk-size universal`) — T3 (`0327B`, the proof lane) needs a booting board | **T3 ✅ 2026-09-29** (0327B P4 H0: `[PASS] board-headless` on the desk board; the console made line-atomic across harts on the way) — **T4 ✅ 2026-09-30**: the driver bring-up playbook seeded as a skill (`.claude/skills/driver-bringup/`: the order measure → hypotheses → gate → code → gate → sharpen, the measurement recipes and gate library of H0/D0, the traps with their dates, a per-driver ledger template) — an insert before D1/D2 so the next driver starts with the method |
| 1 | **Block 1 — First picture** (the OS boots from eMMC over the target boot chain and shows the desktop on the HDMI monitor) | **In Progress** — B1.0–B1.6 ✅ (**B1.6 P3 ✅ 2026-09-29**: the board boots into a living userspace — `init: ready`, blkd on the SDHCI, the OS trace on the eMMC, the fleet `ready`). Open: **B1.7 Display** — D0 measure → M-L layout 1920x1080 (measured) → D1 mode authority + scanout buffer = gpud (QEMU) → D2 host `dc` model + EDID → D3 domain 7/`hmclk` via socd → D4 `dc` driver (P2a requested mode, P2b EDID) → D5 first picture 1920x1080@60 edid | **Operator priority 2026-10-04: the picture with USB input next — Block 2 (U0–U3) before P2b EDID and step 3b**; D4 = TASK-0251 P2: step 3a ✅ 2026-10-04 on the board (an anti-aliased painter, line icons as strokes, the damage grid at the layout, a Lanczos wallpaper bake, the `visible-2d` lane; open findings noted in the ledger: the settled greeter's flat band, hard-coded 1280x800 sizes, tearing, the long black before the splash — its LED part gone 2026-10-04: the ladder is a deprecated flag, the kernel's runtime at 1.7 s instead of 23.4 s) → **step 3c the controller's cursor layer — inside the USB lane, before U3 closes (operator 2026-10-05: "no parallel implementations, no dead paths; the fast path by the end of the USB lane"); proven on the board 2026-10-06 (cycles 14–20: the arrow follows at once, `windowd: hw cursor on`, no present per move)**: on the controller path the pointer is a software sprite blended in every present (`CURSOR_REPLY_SW`, every move a CPU present, `windowd: loop hz=14` on the board), while the virtio path moves a hardware overlay; measured on the stock system (`docs/board/measurements/2026-10-05-cursor-layer/`: RDMA channel 2 → composer layer 6, 64×64 ARGB8888, blend mode 5, a move = the layer's rectangle) → gpud's `dc` arms the overlay, replies `CURSOR_REPLY_HW`, `move_cursor` writes the rectangle + latch, and the controller path's software cursor is deleted with a gate → step 3b the CPU layer composite at the GL path's semantics, **with the 1280x800-era size sweep (TASK-0251 finding 2) — app-host's frame arena generation outgrown at 1080p while typing (`SELFTEST: frame arena spill FAIL`, tolerated on the board since 2026-10-06 in `config/fail-marker-allow-board.txt` ONLY until this step lands; the entry leaves with it)** → P2b EDID over DDC — step 2 ✅ 2026-10-04 on the board, first cycle (windowd's desktop through the controller: one request loop over the virtio GPU and the controller, one CPU executor, the splash held until the reveal; `[PASS] board-visible` 24 rungs + `board-visual: desktop`; `docs/board/measurements/2026-10-04-desktop-path/`) — step 1 first light ✅ 2026-10-03 on the board (gpud's splash on the monitor, `docs/board/measurements/2026-10-03-first-light/`) — D3 ✅ 2026-09-30 on the board (`[PASS] board-headless` with the display rung, H1–H4 decided in one cycle; power domain 7 from the stock tree: control `0x3f4` mode/request, status `0x0f0` bit 15; `hmclk` at 491.52 MHz from `assigned-clock-rates`; the display node trimmed to `hmclk` + `hdmi_reset`; `docs/board/measurements/2026-09-30-power-domains/`), D1 ✅ + D2 ✅ 2026-09-30 (gpud is the mode authority and owns the framebuffer, syscall 50 deleted; `nexus_gfx::backend::dc` EDID + register model with goldens), M-L ✅ 2026-09-30 (`nexus_display_proto::layout`: one home for the 1920x1080 resource layout, the `visible-fhd` lane green), D0 ✅ 2026-09-29 (`docs/board/measurements/2026-09-29-display-regs/`: `hmclk` alone, direct MMIO first-light sequence, bus address from the kernel (bank 0 identical on the controller's bus — corrected 2026-10-03), non-coherent; the encoder's power-down diff via HDMI unplug still open) |
| 2 | **Block 2 — USB input** (keyboard + mouse over USB; proven in QEMU first) | **Done 2026-10-06** (U3 ✅: the desk's keyboard and mouse drive the desktop over USB, `[PASS] board-visible`; the pointer on the controller's layer; 20 board cycles) — **U0 ✅ 2026-10-04**: measured on the stock system (`docs/board/measurements/2026-10-04-usb-topology/`, `…-usb-boot-protocol/`: xHCI 1.10, 64-byte contexts, the USB-A ports behind an on-board high-speed hub with GPIO power, both HID devices full speed → TT; the mouse sends 4-byte boot reports at ~1000/s, STALLs SET_IDLE, and its first frame after the switch is still report-format), RFC-0099 seeded (xhcid the one owner, a reactive event-ring driver, class services as clients; ADR-0039 amended), 0328 at end state, 0253B seeded, the HID boot parsers fixed to the measured reports (the old one refused every report of the desk's mouse) — **U1 ✅ 2026-10-05**: `nexus-usb` + `xhcid` (pure event-driven core + behavioural model, 14 of 16 mutants killed, two real bugs found) + the `usb` lane in `test-all` (QEMU's xHCI, a hub, a boot keyboard and mouse enumerated by the interrupt alone; every other lane `usb plane none`) — **U2 ✅ 2026-10-05**: xhcid serves the HID boot class (policyd `usb.hid`, owed attaches/detaches, fire-and-forget reports), hidrawd is one loop over its sources (virtio-input, USB HID) on one waitset, no timer; the `usb-visible` lane drives the desktop over USB alone (`SELFTEST: ui v2 input ok`) → U3 board | **now (operator priority 2026-10-04)**: U3 next (the board: socd glue, hub power, TT) |
| 3 | **Block 3 — Sub-80 remainder** (`0074 → 0066 → 0067 → 0067B → 0068`) | ledgers are end-state | **next — Block 2's gate passed 2026-10-06** (`board-visual: typed` + `pointer`); order unchanged 0074 → 0066 → 0067 → 0067B → 0068 |
| 4 | **Target pictures M/G/S/N/W** (memory, GPU, SMP, network, web) | 24 packages, interleaved | after Block 3 — **the order among M, G and S is decided after Block 3 from measurements** (operator 2026-10-06: S recut to the end state — n harts from the FDT, no distributed kernel; not to grow beyond kernel + placement + the distribution seam) |

Process per task (since 2026-09-15): review the idea against the code AND its philosophy docs
→ measure before deciding → rewrite the ledger to the end state → build → prove green (`just
check`, `just test-host`, `just test-all`; board lane where the block says so) → one commit
proposal + progress table → the user commits → next.

---

## Hardware rules (in addition to the rules at the end of this file)

1. **The FDT is the one hardware truth.** No MMIO address, IRQ number, clock frequency or hart
   count lives in code outside the FDT parser and its consumers. nxboot passes the DTB through
   and owns `/chosen/nexus,*` (boot profile, slot, display request); on QEMU nxboot reads
   fw_cfg ONCE and re-expresses it in `/chosen`; the kernel and init read only the FDT.
2. **host → QEMU → board** is the proof ladder for every package; the board lane reads the same
   proof manifest over the debug UART (`scripts/board-test.sh`, profiles `board-*`). A
   human-visible check (the picture, typed input) is an operator-acked marker
   (`just board-ack MARKER=…` → `board-visual: <name>` in the serial log) until a readback
   proof exists; never a claim in prose.
3. **Firmware blobs** live only under `resources/firmware/<device>/` with a provenance file
   (source URL, version, SHA-256, license text) and a gate; nothing enters the OS image without
   it. **Vendor kernel code is reference only** (register knowledge from dts/docs); openly
   licensed userspace driver code may be ported with its license kept.
4. **Names in files:** SoC/board identifiers, dts compatibles and USB IDs are allowed (the one
   exception to the no-vendor-names rule); every design comparison stays generic
   ("production-grade reference systems").
5. **Board on the desk:** BPI-F3 (SpaceMiT K1: 8 RV64 harts in two clusters, RVA22 + vector,
   eMMC + microSD, IMG B-series GPU, HDMI via the SoC display controller, MIPI-DSI, a dual-role xHCI
   USB3 controller + a USB2 OTG port, two GbE MACs with external PHYs, an SDIO WiFi/BT module, a 3-pin
   debug UART, M.2 PCIe). Connected over USB-C; today its stock system runs from a microSD card
   (gadget `361c:0008` visible on the host), so eMMC is ours. Boot ROM: hold the download key →
   USB download mode → `fastboot stage FSBL.bin` → `fastboot stage u-boot.itb` → U-Boot fastboot
   writes eMMC partitions. The stock SD card stays as the reference/fallback; we never build a
   "dev SD card" detour — the target chain is the first chain.

---

## Block 0 — Tooling (`TASK-0327`)

One command on a fresh Ubuntu/Debian, Fedora or Arch box gives a developer everything to flash
and watch the board: `make initial-setup` (step 7/7 "board tools") and `scripts/install-deps.sh`.

| Pkg | Content | Files | Gate |
|---|---|---|---|
| T0 Paper | ✅ 2026-09-21 — `TASK-0327` to end state with the desk-box measurement (no flash/serial tools, no udev rule, no serial group, **no USB-UART adapter connected**; board in stock-gadget mode `361c:0008`, download mode is `361c:1001`); `0327B` seeded; the second `TASK-0081` file → `TASK-0209` | `tasks/` | ledger reviewed |
| T1 Packages ✅ 2026-09-21 | `install-deps.sh`: a `BOARD` candidate list per family — `android-tools` (fastboot; apt: `android-sdk-platform-tools`\|`android-tools-adb android-tools-fastboot`), `picocom`\|`tio`\|`minicom`, `device-tree-compiler`\|`dtc`, `u-boot-tools`\|`uboot-tools` (`mkimage`, FIT), `gptfdisk`\|`gdisk`, `usbutils`, `xz`; installed by default (`BOARD=0` opt-out); udev rule `config/udev/71-nexus-board.rules` (`idVendor=361c`, uaccess) + group membership (`dialout`/`uucp`) via the existing sudo adapter; `check-deps.sh` capability rows (`fastboot`, serial tool, `mkimage`, `dtc`, udev rule present, group) | `scripts/install-deps.sh`, `scripts/check-deps.sh`, `Makefile`, `config/udev/`, `README.md` | `make doctor` green on all three families (the existing per-family container smoke), `fastboot devices` lists the board in download mode |
| T2 Recipes ✅ 2026-09-21 (`--stage-only` measured on the board) | `just board-devices` (VID `361c`: gadget vs download mode; serial port autodetect), `just board-serial` (terminal + tee to `build/logs/board--<ts>/uart.log`), `just board-flash` (stage the vendor FSBL + vendor U-Boot from `resources/board/<board>/` — fetched by `fetch-inputs.sh` with SHA pins + license — then `fastboot flash` our partitions from the image `nx image` builds), `just board-ack MARKER=…`, `docs/board/<board>.md` (boot-ROM sequence, partitions, pins) | `justfile`, `scripts/board-*.sh`, `scripts/fetch-inputs.sh`, `docs/board/` | recipes run end to end against the connected board; the serial log is parsed by the same `verify-uart` |
| T3 Board lane — P0/P1 ✅ 2026-09-27, P2/P3 ✅ 2026-09-28, P4 H0 ✅ 2026-09-29 (`[PASS] board-headless`) | ↻ `TASK-0327B` — **P0 ✅ 2026-09-27** (recut: no USB-UART adapter at the desk, so the ladder's channel is the boot trace on the boot disk, RFC-0107; the serial log where an adapter exists) — **P1 ✅ 2026-09-27** (the loader keeps everything it prints in the `trace` partition's slot for its boot and hands the slot to the OS in `/chosen/nexus,trace`; `nx image trace`; `just board-log` reads the board's trace over adb from its stock system; every lane's trace contract: the disk's loader text = the UART's) — **P2 (2026-09-28, the OS trace)**: the kernel keeps every console byte in a static ring of pages, exposed read-only to init like the tree and pinned to blkd alone (no syscall); blkd keeps it in this boot's trace slot, paced by a kernel timer (the `trace` partition has no selector); every lane's trace contract compares each boot's OS text with the UART byte for byte — **P3 (2026-09-28, the RAM rescue)**: the next loader finds the previous boot's ring in the kernel window and keeps its unkept tail (proven on QEMU's reset lane); **measured on the board: the reset scrubs DRAM** (`dram probe lost`), and the board's first OS-trace boots kept the loader's lines and no OS text — the kernel does not reach the block owner, and how far it gets needs the UART or a LED ladder (`sys-led`, measured). Then: `scripts/board-test.sh` (flash → reset → trace/serial ladder), proof-manifest profiles `board-headless`/`board-visible` (`extends` the QEMU ones, `runner = scripts/board-test.sh`), `just board-test`, in `test-all` only under `NEXUS_BOARD=1` (opt-in, never silent) — **P4 (2026-09-29, H0)**: H0a the loader loop after the stock kernel (measure the `verify FAIL` reason + the sector's first bytes in the trace, then the card-init fix, 0246B) → H0b the selftest-client on the board never writes the boot-slot block and skips virtio-shaped probes (`skipped`, never `FAIL`) → H0c `scripts/board-test.sh` + profiles `board-headless`/`board-visible` (`just board-test PROFILE=`); gate `[PASS] board-headless`, `[PASS] board-visible` with D5 | `scripts/`, `source/apps/selftest-client/proof-manifest/profiles/`, `userspace/storage` (trace), `source/boot/nxboot`, `tools/nx` | `[PASS] trace contract` in every lane; `[PASS] board-headless` once Block 1 boots |

---

## Block 1 — First picture on the HDMI monitor

**Boot chain (ADR-0066, seeded at B1.0):** boot ROM → vendor SPL (DDR init; open source, from
`resources/board/`) → OpenSBI → **nxboot as the FIT payload** → kernel. No vendor U-Boot in the
booted system — U-Boot is only the host-side flashing vehicle that runs in RAM. nxboot keeps
A/B slots, GPT and NXBD verification (RFC-0089, ADR-0059) and gains an SDHCI reader beside
`virtio.rs`, FDT pass-through plus the `/chosen/nexus,*` node. The GPT layout SSOT
(`userspace/storage/src/layout.rs`) gains the boot-ROM-mandated head (`bootinfo`, `fsbl`, the
FIT slot) so ONE image (`nx image`) serves QEMU and the board. ONE block owner with an
FDT-selected backend (ADR-0067). Timer/IPI in S-mode = `rdtime` + SBI TIME/IPI, or Sstc when
the FDT lists it — CLINT MMIO from S-mode is PMP-fenced on silicon.

**Measured 2026-09-22 (B1.0, over adb from the stock system):** timebase 24 MHz, Sstc present,
PLIC `riscv,plic0` at `0xe000_0000` (159 sources), UART `spacemit,pxa-uart` at `0xd401_7000`,
RAM in TWO banks at physical 0 and 4 GiB (4 GiB board), DMA masters not cache-coherent
(Zicbom/Svpbmt; the stock kernel bounces through swiotlb), three SDHCI hosts (eMMC HS400 with
ADMA, never flashed), display controller `spacemit,dpu-online2` + `spacemit,hdmi` (the desk
monitor negotiates 1080p60), the dual-role xHCI controller + EHCI + the OTG gadget, two GMACs, GPU `img,rgx` BVNC
36.29.52.182 (firmware present, vendor stack closed), WiFi SDIO `024c:b852` with the vendor
driver built into the stock kernel. Boot flow: the vendor U-Boot loads the DTB from bootfs
(`k1-x_deb1.dtb`) with the FIT's `fdt_1` as fallback; OpenSBI at `0x0`, the payload at
`0x0020_0000`. Details: `docs/board/measurements/2026-09-22-stock-system/README.md`.

**Hardcodes that die, by package:** CLINT/PLIC/UART/virtio-window/RTC addresses + `10 MHz` →
B1.2 · IRQ numbers derived from virtio slot indices → B1.2/B1.3 · fw_cfg boot-mode (syscall 45)
+ display-mode (syscall 50, RFC-0074/ADR-0050) → B1.2/B1.5 · fixed memory windows
(`USER_VMO_ARENA_*`, `KERNEL_PAGE_POOL_*`, `VmoPool`) → B1.4.

| Pkg | Ledger | Content | Seams | Gate |
|---|---|---|---|---|
| B1.0 Paper + measure ✅ 2026-09-22 | ↻ 0244, ↻ 0245, ⬜ 0245B, ↻ 0246, ⬜ 0246B, ↻ 0260, ↻ 0260B, ↻ 0250, ↻ 0251, ↻ 0286 | Recut to end state; seed RFC-0098 (board support: FDT truth + boot chain), ADR-0066, ADR-0067. **Measure first** (R1–R5, R14): which DTB lands in `a1` and whether the vendor SPL loads FIT `loadables`; `rdtime`/Sstc/SBI timer + IPI latency; which clocks/resets/power domains the SPL leaves on (syscon dump from the stock system); DMA coherence per master; display-controller + HDMI PHY register state at 1080p60 and the desk monitor's EDID; `/memory` as reported. Capture `config/board/<board>/board.dts` (only the nodes we depend on; MIT-licensed dts option verified), archive `docs/board/measurements/`, pin SPL/OpenSBI/U-Boot sources in `fetch-inputs.sh` | ledgers, `docs/rfcs`, `docs/adr`, `config/board/`, `docs/board/` | architecture-review verdict per ledger; measurements archived |
| B1.1 FDT ✅ 2026-09-22 | ↻ 0244 | `source/libs/nexus-fdt`: `no_std`, `forbid(unsafe_code)`, bounded parser (memory + reserved ranges, cpus + `timebase-frequency` + `cpu-map`, nodes by compatible with `reg`/`interrupts`/`clocks`, `/chosen` read + in-place property set with headroom); host tests against checked-in dtbs (the board dts compiled + QEMU's dumped virt dtb); nxboot and the kernel consume it | `source/libs/nexus-fdt`, `source/boot/nxboot`, kernel `kmain` | host tests on both goldens; QEMU boots with every platform value read from the dtb: `KSELFTEST: platform from fdt ok (uart=… plic=… tb=…Hz harts=…)` |
| B1.2 Kernel HAL | ↻ 0245 | `hal/virt.rs` → `hal/platform.rs` filled from the FDT at boot (UART `ns16550a` / `snps,dw-apb-uart`, PLIC contexts from `interrupts-extended`, timer = Sstc or SBI, ticks-per-µs from the timebase); kernel + nxboot **position-independent** (self-relocation, `R_RISCV_RELATIVE`); syscalls 45/50 read `/chosen` (fw_cfg code leaves the kernel); the kernel exposes a read-only FDT VMO to init (`device.fdt` grant in `core_plane.rs`, slot in `nexus-service-topology`), so `helpers.rs` / `route_provision.rs` device discovery becomes an FDT walk and IRQ numbers come from `interrupts`; `uartd` from the old ledger is dropped (the kernel-owned console stays) | `source/kernel/neuron/src/{hal,arch/riscv,mm/kernel_layout.rs,core/kmain.rs}`, `source/init/nexus-init/src/bootstrap/{helpers,route_provision,core_plane}.rs`, `source/libs/nexus-service-topology` | `just test-all` green with no platform literal left (`scripts/check-no-platform-literals.sh`: `0x1000_0000`, `0x0c00_0000`, `0x0200_0000`, `10 MHz`); board serial shows the kernel banner + `KSELFTEST: platform from fdt ok` |
| B1.3 SoC glue — **P0 ✅ 2026-09-22** | ↻ 0245B (In Progress) | ONE owner `socd` (driver-kit service, RFC-0106) built from `source/libs/nexus-soc` (K1 tables with provenance, ops over `Bus`, host tests seeded from the measured APMU state); the tree binds consumers the standard way (`clocks`/`resets`/`power-domains`/`pinctrl-0`, mainline ids); policy class `device.mmio.syscon` + `soc.glue.<class>`; QEMU virt answers `NotNeeded`. Measured: shared APMU registers make a per-driver library a cross-service race; the SPL/stock state per device is recorded (`regmap-apmu.txt`) | `source/libs/nexus-soc`, `source/drivers/soc/socd`, `config/board/bpi-f3/board.dts`, `policyd/src/os_lite.rs`, `core_plane.rs` | host tests; `socd: ready (no soc glue in this tree)` in every profile; board `socd: bring-up sdh@d4281000 ok` with 0246 |
| B1.4 Memory M1 — **✅ Done 2026-09-24** — **P0–P3b ✅ 2026-09-23** (P1: `frames` buddy per bank; P2: the kernel in the Sv39 high half behind `phys_to_virt`; P2b: `frame_pool` live at boot, page tables are frames, kernel half shared; P3a: `mm/vmo` page-backed objects, `Vmo { id, len }` caps, `vmo_create_contiguous` for DMA masters, `VmoPool` + arena deleted; P3b: init pages, spawn stacks and the identity window on frames, every window constant deleted, `QEMU_MEM` knob, `check-no-fixed-windows.sh`; P4a ✅ 2026-09-24: `vmo_runs` the one door for a physical address, `cap_query` names no VMO base, gpud backings scatter-gather, gate `dma-contiguous`; P4b ✅ 2026-09-24: `dma-noncoherent` in the device cap, user Zicbom, `nexus_driverkit::DmaBuffer` over `nexus_abi::DmaVmo`; P5 ✅ 2026-09-24: `mm_stats` (61), `KSELFTEST: mm frames`, exhaustion as a bounded event, metricsd gauges) | ↻ 0286 (In Progress) | **First memory package, pulled into the lane** (P0 measured: 22 pool sites, 35 window sites, the physical casts classified; the kernel moves to the high half — direct map at `PHYS_OFFSET`, RFC-0098 C4 amended — because board RAM at PA 0 collides with the user half under an identity map; order P1 frames → P2 direct map → P3 page-backed VMO → P4 DMA → P5 telemetry): physical map from `/memory` + reserved ranges; a page-frame allocator (buddy, 4 KiB + 2 MiB) replaces the fixed windows; VMO becomes an object with a page list and a backing kind (`anon`, `contiguous-DMA` — commit-on-create only for DMA masters; absorbs 0284's ownership prototype + `DmaBuffer` cache-maintenance hooks per R4); `VmoPool` deleted; RAM is 0-based on the board and `0x8000_0000`-based on virt — nothing may care; the accounting counters 0286 promised ride along | `mm/mod.rs`, `syscall/api/{vmo,vmo_pool}.rs`, `mm/kernel_layout.rs`, `nexus-hal` | `just test-all`; `-m` no longer pinned to 320M; `KSELFTEST: mm frames (total=… free=…)` on QEMU and board; `scripts/check-no-fixed-windows.sh` |
| B1.5 Storage — **P0 ✅ 2026-09-24** (measured: eMMC 5.1 at HS400 enhanced strobe with 32-bit ADMA2 on the stock system; the K1 `storage-bus` reaches only `[0, 2 GiB)`; QEMU's only SD host is `sdhci-pci` behind the ECAM host; RFC-0098 C3/C4/C5 + ADR-0067 amended) — **P1 ✅ 2026-09-24** (the tree's buses + `dma-ranges`, `Node::dma_reach`, device descriptor v1 + one kernel record per window, allocation within reach, `vmo_create(.., device)`, `vmo_runs(vmo, device, ..)` in bus addresses; gate `dma-contiguous` replaced by the kernel rule; `KSELFTEST: vmo reach ok`) — **P2 ✅ 2026-09-25** (`source/drivers/storage/sdhci`: the standard core + the K1 layer, eMMC to HS52 or HS400ES without tuning, ADMA2 through `DmaBuffer`, PIO for nxboot; host-proven against a behavioural controller + eMMC model with an exact non-coherent cache) — **P3 ✅ 2026-09-25** (`source/libs/nexus-pci`: the ECAM host from the tree, the root bus planned with BARs on pages of their own and bus mastering left to the grant; `init: devices from pci ok (…)` in every profile; QEMU's SD host found at `00:01.0`, its capability register read through the placed BAR) — **P4a ✅ 2026-09-25** (`virtioblkd` → `blkd` everywhere but dated records, `retired-names` gate in `just check`; the partition gate a pure module, its whole matrix host-tested; the virtio driver's watchdog pair from its owner, `blk: watchdog on` required) — **P4b ✅ 2026-09-25** (the boot disk from `/chosen/nexus,boot-disk`: one codec `storage::boot_disk`, init grants the recorded disk for its kind's class while only policyd runs, `blkd` checks the grant against the record and runs the backend the kind needs — virtio-blk or the SDHCI core over `storage_sdhci::os`; `blkd` alone holds the disk classes) — **P4c ✅ 2026-09-25** (socd in the core plane before the disk grant, on declared slots; `blkd` has it bring the disk's node up — and name the K1's `io` clock — before touching the controller; one socd client `nexus_ipc::socd`) | ↻ 0246 (In Progress), ↻ 0246B (P1 ✅ 2026-09-25: nxboot reads and writes the boot disk through the SDHCI core in PIO, the candidate rule, the PCI plan in the loader — a QEMU boot from `sdhci-pci` alone reaches the whole storage stack; P2 ✅ 2026-09-26: the board path — the SD hosts' measured `no-mmc`/HS400ES flags in the tree, a `no-mmc` host no disk kind, `nexus-soc` bring-up + `io` clock in the loader, host-proven); 0246 **P5 ✅ 2026-09-26** (`ci-os-sdhci` in `test-all`: `sdhci-pci` 8-bit + `emmc`, no virtio disk, the whole ladder over the SDHCI backend) | P1 DMA reach in the device capability (tree buses + `dma-ranges`, versioned device descriptor, allocation within reach, `vmo_runs` per device in bus addresses) → P2 `source/drivers/storage/sdhci` host-first (standard core + K1 layer, eMMC to HS52 8-bit, HS400ES without tuning, ADMA2-32 through `DmaBuffer`) → P3 PCI ECAM device source (one planner for init + nxboot) → P4 `blkd` (P4a the name ✅; P4b the boot disk + the backend by the granted device ✅; P4c socd before the disk grant ✅) → 0246B nxboot readers (SDHCI in PIO, full card init) → P5 `ci-os-sdhci` (`sdhci-pci` + `emmc`) in `test-all` → P6 board after B1.6 | `config/board`, `source/libs/nexus-fdt`, `source/kernel` (reach), `source/drivers/storage/sdhci`, `source/libs` (PCI planner), `source/init`, `source/services/blkd`, `source/boot/nxboot/src/disk/`, `scripts` (lane) | host: reach + driver model + planner reject tests; QEMU `ci-os-sdhci` mounts the system volume over SDHCI; board: `blkd: backend ok (kind=spacemit,k1-sdhci … mode=hs400es …)` + `packagefs: mounted` on serial |
| B1.6 Boot chain — **P0 ✅ 2026-09-26** (0260, measured on the desk board over adb: sector 0 shares the boot-ROM header with a protective MBR — U-Boot's GPT driver, and so the SPL, sees no GPT without one; the head is GPT partitions 1–4 `fsbl`/`env`/`opensbi`/`uboot` at 128K/384K/1M/2M, found by the SPL through the GPT; the backup GPT at the device's end; the vendor vehicle's `flash gpt` builds its own GPT from JSON, every partition basic data, so our GPT goes through its raw writes; the eMMC is empty, 30 535 680 sectors; RFC-0089 §2, RFC-0098 C6, ADR-0066 amended) — **P1 ✅ 2026-09-26** (every image a complete GPT with a protective MBR for its disk, the head as partitions 1–4, volumes from 4 MiB, `nx image build --target qemu|bpi-f3` — a board's image is its eMMC byte for byte, `blkd` by name and type) — **P2 ✅ 2026-09-26** (the eMMC boots from boot0 — header + SPL there, `opensbi`/`uboot` by name — so the head is two partitions and boot0 is built beside the disk; `nx image flash-plan` + `board-flash.sh --plan/--verify` write every region as a raw partition through the vehicle's environment and read it back: done on the desk board, every region exact) | ↻ 0260, ↻ 0260B (**P0–P2 ✅ 2026-09-27**: the FIT; **the board boots its eMMC into nxboot, which verifies slot A and jumps to the kernel** — read from the board's boot trace; the board tree carries what the pinned OpenSBI reads (its CLINT's hart list, the first attempt's silent stop), held by a golden test + `just board-goldens`; **P3 ✅ 2026-09-29**: the kernel boots the board into a living userspace — `init: ready`, `blkd: backend ok (spacemit,k1-sdhci hs400es)`, the OS trace on the eMMC, the fleet `ready` to `stage: platform`; found by the LED ladder + the ring rescue written back per byte: the timer re-armed through SBI while armed through `stimecmp`, `MAX_IRQ`=95, sub-page device blocks refused, no `sfence.vma` after PTE changes — all QEMU-hidden, all fixed from the tree) | 0260: the head as GPT partitions 1–4 (`NEXUS-FW-v1`) in the layout SSOT, the protective MBR in every image, the GPT built for its device, the board image = the QEMU layout + the head's contents (boot-ROM header in sector 0, pinned SPL + OpenSBI, our FIT in `uboot`); `swap` not reserved — M7 appends it with a measured size; **fastboot is the flasher protocol**, writing our bytes through the vehicle's raw writes and reading them back (the residual flasher-protocol scope closes here); 0260B: nxboot as the FIT payload (`config/board/<board>/nexus.its`: nxboot + our tree — OpenSBI stays in `opensbi`; the payload padded to nxboot's whole footprint, since the SPL appends the tree right after it; `scripts/build-fit.sh`, reproducible), `nxboot: platform=… tree=…` as the loader's first line; no platform split (fw_cfg is read only when the tree names it, TASK-0245); the first board attempt read back with `just board-log` (RFC-0107) | `source/boot/nxboot`, `tools/nx/src/cli_image.rs`, `userspace/storage/src/layout.rs`, `config/board/`, `scripts/` | QEMU A/B + NXBD lanes unchanged; **board**: `nxboot: platform=<compatible>` → `nxboot: slot A` → kernel banner → `init: ready` on serial, or — no adapter at the desk — in the boot trace read back with `just board-log` (RFC-0107) |
| B1.7 Display | ✅ 0250 (2026-09-30), ↻ 0251 (P1 ✅), ↻ 0245B P3 (display set ✅ 2026-09-30 on the board; pads, USB, GPU open), M-L (layout 1920x1080) | **D0 measure first** (DPU/HDMI register dump at 1080p60 — `devmem` on the stock system, else the vendor U-Boot staged + `oem run` + mmc write, else the driver unbound; APMU/domain-7 diff display on/off; the DDC path; EDID decoded: no 1280x800, VIC 16 1080p60 and VIC 4 720p60 present); **M-L** `LAYOUT_MAX` to 1920x1080 with measured cost (windowd FB, gpud plane rows, arena budget); 0250 (host-first): the display-controller scanout contract as a `GfxBackend` impl (planes, damage flush, EDID mode parsing) with host tests; 0251 (OS/board): gpud backend `dc` for `spacemit,dpu` + the HDMI encoder/PHY (EDID over DDC), `attach_external_framebuffer` + `set_scanout` on a contiguous-DMA VMO, `scanout_policy.rs` gets its third arm; **display-mode authority moves to gpud** (EDID on the board, `ctrl_query_display_info` on virtio) and syscall 50 + the RFC-0074 fw_cfg path are deleted (RFC-0098 amends RFC-0074); windowd stays the single present authority (RFC-0067/0093) The scanout buffer is allocated by gpud with the DPU's device cap and handed to windowd in the RFC-0093 reply (windowd's `vmo_create` deleted); `pick_mode` = the highest EDID mode ≤ the SoC's maximum with the monitor's aspect ratio, never stretched. | `source/drivers/gpud/src/backend/dc/`, `display_mode.rs`, windowd `display_backend.rs`, kernel syscall table | QEMU visible lane unchanged (pixel proof); board `gpud: dc scanout ok (1920x1080@60 edid)` + `windowd: desktop revealed` on serial + `board-visual: desktop`. **Block 1 gate = `just board-test PROFILE=board-visible` green and `README.md` says "real hardware: yes"** |

---

## Block 2 — USB input (provable in QEMU first: `qemu-xhci` with `usb-kbd`/`usb-mouse` behind a `usb-hub`; TT only on the board) — before D5 by the operator priority 2026-10-04 (the picture with USB input first); Block 3 follows U3

| Pkg | Ledger | Content | Seams | Gate |
|---|---|---|---|---|
| U0 Paper ✅ 2026-10-04 | ↻ 0328, ⬜ 0253B | 0328 to end state (extracted from `TRACK-REMOVABLE-STORAGE` CAND-REM-030); seed RFC-0099 (USB host contract: class drivers are clients of the controller service, ADR-0039 layering); measure R8 (the dual-role controller's and the PHYs' init, USB3 vs USB2 fallback); HID parser fixes in `userspace/hid` (boot mouse 3..=8-byte reports + wheel, ErrorRollOver held, not rejected) | ledgers, `docs/rfcs` | review verdict |
| U1 Host stack ✅ 2026-10-05 | ↻ 0328 | `source/libs/nexus-usb` (descriptors, control/interrupt pipes, hub class, class dispatch over IPC), `source/drivers/usb/xhcid` (rings, TRBs, command/event/transfer, PLIC IRQ from FDT, DMA via `DmaBuffer`; host-tested against a mock `Bus`), the dual-role controller's host-mode glue via `nexus-soc`; policy class `device.mmio.usb`, slots + grants + `check-slot-ssot.sh`; one `device.mmio.usb` from PCI class 0x0c03 (QEMU `qemu-xhci`) or the `snps,dwc3` node with `dr_mode = host` (board); the dual-role controller's global registers in xhcid, APMU glue/`phy_sel` as socd bring-up steps | `source/libs/nexus-usb`, `source/drivers/usb`, topology + policy | host mock tests; QEMU `ci-os-usb` profile: `xhcid: ready (ports=…)`, `xhcid: device enumerated (vid=… pid=… class=hid)`; board same ladder with a physical keyboard |
| U2 HID ingress ✅ 2026-10-05 | ↻ 0253B (P1–P3 ✅, P4 with U3) | `hidrawd` ingress becomes a `HidSource` (virtio-input on the virt profiles, USB HID boot protocol via `xhcid` on the usb profile and the board); `userspace/hid` (TASK-0252) is the shared parser; one contract (`WireHidBatch`), two transports; the virtio-only ingress code path is deleted | `source/services/hidrawd`, `userspace/hid` | QEMU `ci-os-usb-visible` drives the visible input ladder (`SELFTEST: ui v2 input ok` via USB) ✅; board: keyboard + mouse move the desktop, `inputd: live pointer route on` + `board-visual: typed`. **Block 2 gate** |
| U3 Board ✅ 2026-10-06 (`[PASS] board-visible` 46 rungs + `board-headless` 38, cycle 20 of 20) | ↻ 0328, ↻ 0253B P4, ↻ 0245B P3 (USB set) | socd brings `usb@c0a00000` up (clocks/resets/`phy_sel`/glue from R8), xhcid on the controller's window (PHY per R8; v1 serves the USB2 root ports), a keyboard and a mouse on the USB-A ports | `source/drivers/usb/xhcid`, `source/libs/nexus-soc`, `config/board/bpi-f3/board.dts` (phy nodes) | `xhcid: ready`, `xhcid: device enumerated`, `hidrawd: usb hid device (…)`, `inputd: live pointer route on` / `… keyboard route on` (inputd's own word that a real event reached its live routes) + `board-visual: typed`. **Block 2 gate** |

---

## Block 3 — Sub-80 remainder (mission: every task < 0080 Done)

Starts after U3 (the desk's priority 2026-09-29: the picture and the input on the board first).

Phase 1 (0321/0035/0028/0043/0052) ✅ 2026-09-08; reconciliation closures of 2026-09-09 and
Phase 2's ✅ 0054C, ✅ 0033, ✅ 0326, ✅ 0077B, ✅ 0077C (2026-09-18 … 09-21) are on the board and in
`CHANGELOG.md`. Remaining, in order, ledgers already rewritten to end state (2026-09-09):

| # | Task | End-state content | Size | Needs |
|---|---|---|---|---|
| 1 | TASK-0074 | Modal semantics in the DSL runtime: `.overlay(modal\|transient)`, bounded stack, ESC/backdrop dismissal, focus trap, ONE windowd verb `CONTROL_WIN_MODAL`, toast as transient overlay | M | 0077B ✅; 0324 P4a ✅ |
| 2 | TASK-0066 | WM zones: halves + occupancy-driven thirds, `zones.rs` replaces `snap.rs`, reflow on mode change, snap state in the window feed (RFC-0086 bits), fail-closed deny, registered markers — verify the snap-release → fullscreen wedge first | M | 0324 P4a/P6 ✅ |
| 3 | TASK-0067 | `clipboardd` = single content-transfer authority (multi-MIME, history 16, focus-gated reads pushed by windowd), `svc.clipboard.*` binding, DnD routing in windowd (RFC-0094, ADR); placeholders deleted; absorbs 0087 + the 0122C clipboard bridge | L | 0324 P3/P4 ✅; 0054C ✅; 0033 ✅; 0066 |
| 4 | TASK-0067B | Clipboard history panel in the desktop shell (DSL) + copy-back | S | 0067; 0074 |
| 5 | TASK-0068 | `screencapd` over the ONE readback authority (gpud `OP_READBACK` BUILT HERE in P1 — verified 2026-10-06: no readback op exists, only `OP_REVEAL`; the pixel proofs are host screendumps), windowd geometry/secure gate, consent = policy, caps (RFC-0095) | M | 0324 P6 ✅ (hard); 0074; 0033 ✅; 0054C ✅ |

**The fence (2026-10-06, binding for Block 3 — the block must END, not grow):** (1) each task
ships exactly its ledger's DoD (marker literals + host tests + docs), nothing beyond; (2) what is
found on the way is NOTED in the ledger's open findings, never built — the one exception is a
blocking defect in the same code path (0066's snap-release → fullscreen wedge), capped at one
package; (3) proof = the QEMU lanes (`visible`, `smp1`, `input-live`, `usb-visible` where input
is involved); NO board cycles inside Block 3 — ONE board smoke (`[PASS] board-visible`) is the
block's gate; (4) no services beyond clipboardd and screencapd, no widget crates, nothing drawn
in windowd; (5) a package that goes red twice for scope (not flake) stops the task for a recut;
(6) protection zones named per task up front; (7) closure per task before the next starts.
Stale items fixed at P0: 0074 D1 "IR v1.3" → the current IR version; 0067 D6 / 0068 D5 service
ids → the next free ids (xhcid took one); 0068's readback primitive is built by 0068 P1. After
the block's gate — not before — one measurement package (memory: RSS/commit per service, VMO
arena headroom at 1080p; GPU: CPU present vs GL on QEMU; SMP: R9 on the board, `-smp 8` ×10)
decides the order of M, G and S.

---

## The four target pictures — end-state decisions

**M — Memory (the memory model of the best desktop/mobile systems, chosen for our
architecture; RFC-0100 memory object model v2, ADR-0068 compressor before swap).** A VMO is a
page-backed object (page list + backing kind: anonymous / package-volume file / zero-fill /
contiguous-DMA), committed on demand — the user page fault (`fault.rs:365`, fatal today) becomes
the populate path and stays fatal only for true violations. Shared read-only code: ONE page-cache
VMO per bundle file mapped by every process (the per-process ELF copy in `exec.rs`/`exec_copy.rs`
is deleted — needs 0033's pass-through); copy-on-write for private data pages; clean/dirty per
page (PTE dirty bit, A/D where the ISA has it, software otherwise); **purgeable** VMOs for
caches (atlas, blur cache, decoded images — the kernel may discard clean or purgeable pages
under pressure); per-process accounting (RSS/commit/shared/purgeable); four pressure levels
pushed to services and apps (RFC-0087 "exhaustion is an event"); kernel-enforced limits;
**an in-kernel compressor pool before any swap** (LZ4-class over anonymous dirty pages); swap =
the `swap` partition via `blkd` as the last stage (compressed pages only; clean file pages are
dropped, never written); a termination policy (`memd`, the recut of `oomd`) as the final step,
deny-by-default, journaled.

**G — GPU (the whole desktop composited on the GPU; the reverse-engineered-laptop-GPU driver
architecture applied to our gpud / driver-kit / nexus-gfx split; RFC-0101, ADR-0069 offline
compositor shaders).** The thin side is the driver-kit service `gpud` backend `img`: firmware
load (blob under the provenance gate), GPU MMU with **explicit VM management** (`vm_bind` /
`vm_unbind` of VMO pages into the GPU address space), submission queues over
`nexus-driverkit::SubmitRing`, IRQ, and **explicit sync only** through kernel fences; power,
clocks and resets from the FDT via `nexus-soc`. The userspace driver lives in `nexus-gfx`
behind the existing `GfxBackend` + `CommandBuffer` seam: buffer objects, command-stream encoding,
pipeline state; the compositor's shaders are **compiled offline on the host** and shipped in the
bundle (no on-device compiler in this track). windowd and app-host never see the GPU directly
(RFC-0067 boundary). The virgl backend on QEMU implements the same v2 trait; the CPU raster path
survives only for headless/QEMU proofs. Gaming is out of scope.

**S — Real SMP (n harts from the FDT, work genuinely spread; recut 2026-10-06 to the end
state — RFC-0102 lock decomposition, RFC-0107 `CpuSet` ABI, ADR-0072 cluster-aware spreading,
ADR-0073 split order, ADR-0074 compute-distribution seam).** Not `MAX_CPUS` 4 → 8: the hart
count comes from the FDT at boot and every per-hart structure (secondary stacks, IRQ stash, PLIC
contexts, deadline shadow, console lines, counters, TLB mailboxes, run queues, `current`) is
allocated from a boot slab sized by it; the only compile-time ceiling is the CPU-set width
(`CpuSet` = 64 bits, replacing the `u8` affinity mask — the one true limit, an ABI), `HART_LOCALS`
stays static at that width (fixed address for `trap.S`), `MAX_CPUS` is deleted with a gate. The
cluster topology from the FDT `cpu-map` (the board: 2 × 4 harts, an L2 per cluster) is a kernel
model exported to init/execd (no literal masks in userspace); PLIC contexts and IRQ affinity on
every hart (the display chain no longer needs cpu0); the HSM retry bug (a virtual start address)
fixed — the likely MTTCG "lost hart" flake; the big kernel lock is decomposed in a MEASURED
order (R9: hold histogram + IPC rate per hart first) — scheduler → IPC router + waitsets →
address spaces (per-ASID targeted shootdown) → timers/fences last; placement spreads by load
within a cluster first, across clusters second, the interactive chain on cluster 0 protected by
the 0288 budgets, the affinity SSOT keeps only hard pins; workpool/pinched take their worker
count from the topology (never a worker on the soft-RT hart). **No distributed kernel** (operator
decision 2026-10-06): one kernel per machine, capabilities never travel; the distribution unit
is pinched's job (VMO bytes + descriptor + determinism), and the lane lays only the seam —
`Backend::{Local, Remote}` with `Remote` refused by name — and the ADR. **No consumer in this
lane** (operator: the lane must not grow): the first real parallel consumer — the band-parallel
CPU present (gpud's executor on the workpool, disjoint rows, fence join; the measured 30 ms
full-frame present and the typing flicker) — belongs to G3/G4 / TASK-0251 finding 6 and rides on
S5. Gate per step: the BKL budgets shrink monotonically, the ipc call budget holds, the board
runs all 8 harts (`KSELFTEST: smp online ok harts=8 clusters=2`), QEMU `smp` (2 harts) stays
the deterministic lane, `smp8` is best-effort until the retry fix is proven over ten boots.

**N — Network (RFC-0105 network onboarding, ADR-0070 TLS choice, ADR-0071 FullMAC WiFi).**
**WiFi first (user decision 2026-09-21):** the SDIO function on the SDHCI driver + a FullMAC
driver for the module + its firmware (provenance gate) + `wifid` (WPA2/3 key handling,
credentials via settingsd). Honest warning recorded at N1's P0: no openly maintained driver
exists for this SDIO part — P0 measures the vendor driver corpus and the firmware license (R7)
before committing; Ethernet (SoC MAC + external PHY) is a small package that must not wait for
that outcome. Then DHCP + DNS in netstackd (`netcfgd`), TLS on device (rustls `no_std` + alloc
first, `std` later; `getrandom` over rngd closes the RFC-0009 gap; the ingressd TLS slot =
0323 residual), **the clock loads from the internet, localized** (SNTP → `timed` anchor, tz +
region from settings, RFC-0076/0077), and dsoftbus on the real network: the network family
(0024 → 0030 → 0038 → 0040) and the proof lanes W1–W3h leave HOLD once Ethernet has a lease.

**W — Web (the isolated engine, the way the well-known capability OS solved it; RFC-0103 std
target + component runtime, RFC-0104 web engine component protocol).** A Rust `std` target
`riscv64gc-unknown-nexus` (`std::sys` over our syscalls/IPC: threads, heap, fs via vfsd, sockets
via the netstackd facade, time via timed) plus a C library + C++ runtime port (clang target) for
the engine's C/C++ parts; both run as isolated **components** spawned by execd with
kernel-attributed identity (`0332B`). The engine component `webengined` sits behind a
Context/Frame/Navigation protocol: frames are delivered through ADR-0042 per-app surfaces (a new
`nexus-surface` crate absorbs app-host's client sequence), input through `OP_SURFACE_INPUT`,
network only through the facade, no device caps. The DSL gets a `WebView` node bound to a Frame;
the browser app keeps DSL-authored chrome. The sanitizer renderer of the old webview ledgers is
deleted from the plan — no dual structure.

---

## Ledger map for the target pictures

| Lane | Ledger | Was | Becomes |
|---|---|---|---|
| M | ✅ 0286 | per-task RSS counters + pressure snapshots | **M1** (in Block 1): physical map from FDT + page-frame allocator + page-backed VMO + `contiguous-DMA` kind (absorbs ↻ 0284) |
| M | ⬜ at P0 0286B / 0286C | — | **M2** demand paging + CoW fault path · **M3** page cache + shared RO code (`exec_copy.rs` deleted) |
| M | ↻ 0287 | watermarks + hard limits + OOM handoff | **M4** per-process accounting + four pressure levels pushed + kernel limits |
| M | ↻ 0290 | VMO seals + write-map denial + reuse truth | **M5** purgeable VMOs + seals (atlas + blur cache converted) |
| M | ⬜ at P0 0287B / 0287C | — | **M6** in-kernel compressor pool · **M7** swap partition via `blkd` + `memd` termination policy (absorbs ↻ 0228, superseded) + evidence |
| G | ⬜ 0329 (+ ⬜ at P0 0329B) | `TRACK-DRIVERS-ACCELERATORS` CAND-DRV-010 | **G0** GPU truth (measure) + **G1** thin side in gpud (firmware, GPU MMU, power, IRQ, fence bridge); **G5** app-surface zero-copy import (0329B) |
| G | ↻ 0280 | DriverKit v1 core contracts (queues, fences, buffers) | **G2** explicit VM/BO/sync API: driver-kit contracts + `GfxBackend` v2 (`create_bo / vm_bind / submit(fences)`), cpu_mock implements v2 |
| G | ↻ 0169B, ↻ 0170B, ↻ 0171 | nexus-gfx resource/fence core; windowd handoff + pass planning; wgpu host parity | **G3** the nexus-gfx GPU driver: resources/fences over the real backend, command encoder, offline shader pipeline; the host parity backend is the host-first proof of the same `CommandBuffer` |
| G | ↻ 0215, ↻ 0216 | compositor v2.2 planes / async present / color (host, OS) | **G4** the whole desktop on the GPU: plane planner, async present, cursor plane, color; CPU compositing deleted on the board; display chain unpinned |
| S | ⬜ 0330 (+ ⬜ at P0 0330B / 0330C) | — (0012, 0012B, 0042, 0247, 0277, 0283, 0288 ✅) | **S0** measure (per-hart static inventory, `-smp 8` ×10 boots vs the HSM retry bug, R9 BKL histogram + IPC rate per hart on the board, the `cpu-map`) · **S1** n harts from the FDT (boot slab, `CpuSet` 64-bit ABI = RFC-0107, `MAX_CPUS` deleted) + cluster topology exported + per-hart PLIC/IRQ affinity (0330) · **S3** IPC router + waitsets off the BKL (0330B) · **S4** address spaces off the BKL + per-ASID shootdown (0330C) |
| S | ↻ 0306 Phase 3 | BKL hold reduction (Phases 1–2 delivered) | **S2** scheduler off the BKL: per-hart runqueue locks + lock-free wake/IPI |
| S | ⬜ at P0 0042B | (0042 ✅ affinity/QoS ABI) | **S5** timers/fences last + placement v2 (spread by load, cluster-aware, hard pins only), workpool/pinched workers from the topology (none on the soft-RT hart) · **S6** the compute-distribution seam: ADR-0074 (jobs travel, capabilities never; one kernel per machine; brokers compose), pinched `Backend::Remote` declared and refused by name, the job's determinism contract host-tested — no network code |
| N | ↻ 0248 (+ ⬜ at P0 0248B), ↻ 0249 | virtio-net host core / virtionetd OS | **N1** SDIO function + FullMAC WiFi driver core + firmware contract (0248), `wifid` + netstackd on the real NIC (0249); **N2a** Ethernet MAC + PHY (0248B) |
| N | ↻ 0138, ↻ 0139 | offline netcfgd / sim-dhcp / dnsd; Settings page | **N2b** `netcfgd` real DHCP + DNS in netstackd (0138); network Settings page + `nx net` (0139, lands with W3) |
| N | ↻ 0193, ↻ 0194 | devnet TLS / fetchd | **N3** TLS on device + `getrandom` over rngd + ingress TLS slot |
| N | ↻ 0299 | SNTP seed | **N4** clock from the internet, localized (SNTP → `timed`, tz/region) |
| N | ⬜ 0331; ↻ 0024 → ↻ 0030 → ↻ 0038 → ↻ 0040 | `TRACK-NETWORK-PROOF-LANES` W1/W2/W3h; the network family | **N5** proof lanes on a real network (board ↔ QEMU/host, `ci-network` green), then the family in order (HOLD lifted at N2) |
| W | ⬜ 0332 (+ ⬜ at P0 0332B) | — (`TRACK-CONSOLE-AND-TOOLCHAINS` "managed runtimes" linked) | **W1** `std` target + C/C++ toolchain port (0332); **W2a** component runtime for std binaries under execd + `nexus-surface` crate (0332B) |
| W | ↻ 0111, ↻ 0176, ↻ 0177 | webviewd sanitizer renderer; sanitizer → Scene-IR; httpstubd | **W2b** `webengined` behind Context/Frame/Navigation (0111); engine host harness (0176); network only through the facade — fetch/downloadd (0177) |
| W | ↻ 0113, ↻ 0186, ↻ 0187, ↻ 0205, ↻ 0206 | browser app; webview core; file chooser; persistence | **W3** DSL `WebView` node + browser app with DSL chrome (0113); history/find/session (0186), file chooser + leases (0187), persistence + crash recovery (0205/0206) — all on the engine |

Seeds: RFC-0098 board support · RFC-0099 USB host · RFC-0100 memory object model v2 · RFC-0101
GPU driver split · RFC-0102 SMP lock decomposition · RFC-0103 std target + component runtime ·
RFC-0104 web engine component protocol · RFC-0105 network onboarding · RFC-0106 SoC glue (one owner, seeded 2026-09-22); ADR-0066 boot chain ·
ADR-0067 one block owner, FDT-selected backend · ADR-0068 compressor before swap · ADR-0069
offline compositor shaders · ADR-0070 TLS choice · ADR-0071 FullMAC WiFi · ADR-0072
cluster-aware spreading · ADR-0073 BKL split order. (RFC-0094/0095 stay reserved for 0067/0068.)

---

## Interleaved package order after Block 3 (dependency-correct)

Each row's prerequisites are earlier rows; a picture never blocks another longer than its
prerequisite edge. M1 (0286) is already in Block 1.

| # | Pkg | Ledger | Needs | Gate (short) |
|---|---|---|---|---|
| 1 | S1 | 0330 | B1.2 | S0 measured first; `ci-os-smp` (2 harts) green, `smp8` best-effort until the retry fix holds ×10; board `KSELFTEST: smp online ok harts=8 clusters=2`, IRQs served on non-boot harts; `MAX_CPUS` retired-name gate |
| 2 | M2 | 0286B | M1 | host fault-model tests; QEMU CoW proof marker |
| 3 | G0 | 0329 P0 | B1.0 | GPU compatible/revision, firmware + license, register/IRQ map, power sequence archived (R6) |
| 4 | S2 | 0306 P3 | S1 | BKL hold budgets shrink (`KSELFTEST: bkl budget ok`), ADR-0073 order confirmed by R9 |
| 5 | M3 | 0286C | M2, 0321 ✅ | RSS drop measured across the 14 services (threshold from R10) |
| 6 | G1 | 0329 | M1 (contiguous DMA), S1 (IRQ off hart0), B1.3 | board `gpud: fw ok`, `gpud: mmu map/unmap ok`, a null job signals a fence |
| 7 | N1 | 0248 + 0249 | B1.5 (SDHCI core), M1, S1; R7 decided | board `wifid: assoc ok ip=…` (static address first) |
| 8 | S3 | 0330B | S2 | RFC-0096 numbers per hart; `TRACK-TIME-AS-RESOURCE` gate turns green |
| 9 | M4 | 0287 | M3 | host snapshot tests; QEMU pressure-event marker |
| 10 | G2 | 0280 | G1, S3 | `cpu_mock` implements v2; board: clear colour into the scanout BO |
| 11 | N2 | 0248B + 0138 | N1 (or its P0 verdict), B1.3 | `ci-os-dhcp` unchanged; board: both links lease, DNS resolves — **HOLD on the network family lifts here** |
| 12 | W1 | 0332 | M3, M2; R12 | a cross-built `std` `hello` runs on QEMU; threads/sockets/fs gates; a C++ test binary; `just test-std` |
| 13 | S4 | 0330C | M2, S3 | shootdown-count gate (targeted, not full); `ci-os-smp` |
| 14 | G3 | 0169B + 0170B + 0171 | G2 | board textured quad; host encoder goldens |
| 15 | M5 | 0290 | M4 | host; atlas + blur cache purgeable and reclaimed under a pressure marker |
| 16 | N3 | 0193 + 0194 | N2 (rustls `no_std` first; `std` variant after W1) | devnet lane recut; ingress TLS slot filled |
| 17 | G4 | 0215 + 0216 | G3, M5 | board `board-visual: gpu-desktop` + a frame-time gate; QEMU virgl lane unchanged |
| 18 | S5 | 0042B | S4, G4 | per-hart utilisation gate on QEMU and board |
| 19 | N4 | 0299 | N2 | QEMU against a host SNTP stub; board sets real time + tz |
| 20 | M6 + M7 | 0287B, 0287C | M4, M5, B1.5 (swap partition) | compression ratio/throughput measured (R11); OOM ladder marker; a low-`-m` QEMU profile |
| 21 | W2 | 0332B + 0111 + 0176 + 0177 | W1, M3, M4, N3 | QEMU: the engine renders a local page into a surface; board same |
| 22 | N5 | 0331; then 0024 → 0030 → 0038 → 0040 | N2, N3, S3 | `just ci-network` green + a two-device proof (board ↔ QEMU/host) |
| 23 | W3 | 0113 + 0186 + 0187 + 0205 + 0206 + 0139 | W2, N4 | QEMU + board `board-visual: browser` |
| 24 | G5 | 0329B | G4, W2 | app + engine surfaces imported zero-copy on the GPU |

Critical edges: G1 ← M1 + S1 · W1 ← M3 (a `std` process without shared code doubles RSS) ·
W2 ← M4 (the engine must receive pressure) + N3 · S3 before G2 (submission/fence IPC storms under
the BKL would invalidate frame-time gates) · S4 ← M2 (ASID lifetime needs the new address-space
object) · M6 ← M5 (purgeable pages are dropped before anything is compressed) · N1 ← B1.5 (SDIO
shares the SDHCI driver).

Risks measured before deciding (each at its P0): R1 SPL → OpenSBI → FIT handoff · R2 timer/IPI
on silicon · R3 clocks the SPL leaves on · R4 DMA coherence per master · R5 display controller +
HDMI PHY state, EDID · R6 GPU revision, firmware license, MMU page size · R7 WiFi firmware +
FullMAC/SoftMAC · R8 xHCI + PHY init · R9 BKL hold histogram + IPC rate per hart · R10 RSS
of the 14 services + bytes duplicated by `exec_copy` · R11 compressor ratio/throughput on RV64 ·
R12 the pinned nightly builds `std` for a custom target (`-Zbuild-std`) and the engine's C++
dependencies' OS requirements · R13 boot-ROM fastboot quirks on the desk board · R14 `/memory`
as reported on 4/8/16 GB boards.

---

## Where the other existing ledgers go

| Ledger | Disposition |
|---|---|
| 0246 / 0248 / 0249 | taken (above): their virtio subjects are Done elsewhere (0314, 0003); the numbers now carry the hardware block/network drivers |
| 0261 | parked: recovery target exposes a fastboot gadget (device-side flashing) — after Block 2 and USB gadget mode |
| 0284 | ✅ Done 2026-09-24 — closed by 0286 M1 P4b: `DmaBuffer` ownership + cache maintenance |
| 0228 | superseded by 0287C (`memd`, one memory authority) |
| 0281 / 0282 | kernel Rust-idiom closures — unchanged, run with S when a package touches the same files |
| 0316 – 0320 | storage end-state ladder (nxfs v2, nxfsd, perf, CoW/snapshots, encryption) — parked, unchanged |
| 0145B / 0269B | runtime quality budgets — parked; 0269B gets a board row (boot → first frame on hardware) once Block 1 is green |
| 0122B / 0122C | DSL app platform — parked (0122C's clipboard bridge is in 0067) |
| 0323 | ingress follow-ups: TLS → N3; UDP data plane + listener close stay with the family (N5) |

---

## Parked reference (after the fast track; no daemon/app/marker exists yet)

- **Notifications:** `0123`, `0124`, `0125` · **Search / palette:** `0151`–`0154` · **Share /
  DnD consumers:** `0126`–`0128` · **Content / files apps:** `0081`–`0093`, `0232`, `0233` ·
  **Media / audio:** `0099`–`0102`, `0155`, `0156`, `0184`–`0187`, `0217`–`0220`, `0254`,
  `0255` · **Accessibility:** `0114`–`0118` · **Camera / privacy:** `0103`–`0106`, `0191`,
  `0192` · **Store / distribution:** `0180`, `0181`, `0221`, `0222` · **Backup / L10n / power /
  sensors:** `0161`, `0162`, `0236`, `0237`, `0256`–`0259`, `0271`, `0272` · **Session /
  accounts / lifecycle:** `0234`, `0235`, `0109`, `0110`, `0223`, `0224`, `0126B`, `0159` ·
  **IME (RFC-0075):** `0147`, `0149`, `0150`, `0203`, `0204` · **Renderer / compositor v2
  leftovers:** `0199`, `0200`, `0207`, `0208` · **IME-store encryption:** `0300`.
- **From the dissolved tracks:** audio device-class service, camera/ISP, NPU runtime (was
  `TRACK-DRIVERS-ACCELERATORS` CAND-DRV-020/030/040 → `TRACK-NEXUSINFER-SDK` keeps the NPU
  vision); zero-copy packet buffers (was `TRACK-NETWORKING-DRIVERS` CAND-NETDRV-010 → a follow-up
  of 0249 once N1 lands).

## Active TRACKs (spawn tasks when gates clear)

| Track | Purpose | State / link |
|-------|---------|--------------|
| TRACK-REMOVABLE-STORAGE | USB/SD/external disks: providers, FAT32, formatting | CAND-REM-030 (USB host) extracted → `TASK-0328`; FAT32/formatd candidates wait for Block 2 |
| TRACK-TIME-AS-RESOURCE | Conserved time/budget rights along the init tree | gates RED until S3 (`0330B`) — bounded kernel ops need the BKL decomposed |
| TRACK-CONSOLE-AND-TOOLCHAINS | In-OS console, managed runtimes/tools | managed runtimes ride on the `std` target + component runtime → `TASK-0332` |
| TRACK-NEXUSGFX-SDK | Graphics SDK for apps | after G4/G5 (the GPU backend exists) |
| TRACK-NEXUSINFER-SDK | On-device ML runtime (CPU ref + future NPU) | after G (driver-kit GPU path is the NPU template) |
| TRACK-NEXUSMEDIA-SDK | Audio/video/image SDK | after the audio device-class service (parked) |
| TRACK-STASH-USER-DATA-FS | User-data FS ladder + stash | milestones 1–5 Done (0291–0295); 6–12 = 0314–0320 parked |
| TRACK-ZEROCOPY-APP-PLATFORM | RichContent + OpLog + connectors | TASK-0031 ✅, TASK-0067 (Block 3) |
| TRACK-APP-STORE · TRACK-DEVSTUDIO-IDE · TRACK-DSL-V1-DEVX | distribution · IDE · DSL masterplan | after the fast track; DSL phases 1–6 Done, runtime scale = 0077C ✅ |
| Dissolved 2026-09-21 | `TRACK-DRIVERS-ACCELERATORS` → 0329 · `TRACK-NETWORKING-DRIVERS` → 0248/0249 · `TRACK-NETWORK-PROOF-LANES` → 0331 | header blocks in the track files point here |

---

## Rules

1. **Lane order over task number.** Execute the active lane in its stated order; outside a
   lane, numerical order. Skip a blocked task, note why, move on.
2. **End system only.** No interim solution; a new structure replaces the old one and the old
   one is deleted in the same package, with a gate against its return. Prerequisites are
   pulled into the lane, not bypassed (M1 into Block 1 is the example).
3. **100 % rule.** A task is Done only when every stop condition is met and the ledger, board,
   CHANGELOG and affected docs are swept; counters are recomputed from ledger headers.
4. **No fake success.** Markers prove real behaviour (`stub`/`placeholder`, never `ok`, for
   stubs); a marker is a contract — change it together with `scripts/qemu-test.sh`,
   `tools/nx/chains/markers.txt` and the proof manifest. A manifest `emit_when` only suppresses
   surprise; a REQUIRE lives in the harness.
5. **Visible-proof-surface rule (UI tasks).** A UI capability is not claimed from host goldens,
   markers or an isolated demo alone: it must appear on the shared visible proof surface of the
   real QEMU desktop (and, from Block 1 on, the board) and be live-checkable there.
6. **UX quality gate (desktop/launcher quality claims).** The first gate — visible login/greeter
   or dev session; live pointer + keyboard; cursor, hover, focus, click, scroll; launcher/dock;
   app start with a visible window and focus/close/move; SVG icon sources; settings / quick
   settings; no global input leaks; no marker-only desktop claims — is met (Phase 1 24/24,
   2026-09-08). The end-state bar from 2026-09-29 on: the fluency and material quality of an
   early tablet operating system or a phone's docked desktop mode — translucent blur materials,
   60/120 Hz motion without tearing, immediate pointer/touch response, transitions instead of
   cuts — in our own design language; measured (frame-time gates, present median, input
   latency), never claimed. `inputd` normalizes, `windowd` owns hit-test/hover/focus/click, the
   DSL shell owns shell/launcher/session surfaces, apps get only their surfaces.
7. **Boundary.** windowd is the compositor SERVICE (single present authority); window UI lives
   in widgets and the DSL shell app; the GPU is reached only through gpud/nexus-gfx; no
   Linux/Wayland paths; ADR for any kernel↔userspace, service↔service, host↔OS,
   policy-authority or device-class crossing.

## Related

- Status board (Kanban view, the Done list, group counters): `tasks/STATUS-BOARD.md`
- Task workflow rules: `tasks/README.md` · RFC process: `docs/rfcs/README.md`
- Agent guide (rules, gates, commands): `CLAUDE.md`
- Board notes (from Block 0 on): `docs/board/`
