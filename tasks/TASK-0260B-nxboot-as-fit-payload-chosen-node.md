---
title: TASK-0260B nxboot is the FIT payload on the board — `/chosen/nexus,*` written by nxboot, fw_cfg read only there, one image for QEMU and the board
status: Done (P0 recut, P1 the FIT and P2 the first eMMC boot ✅ 2026-09-27; **P3 ✅ 2026-09-29 — the board boots our chain into a living userspace**: `init: ready`, blkd on the SDHCI, the OS trace on the eMMC, the fleet `ready`; verified with the board's own tree, four harts and the LED ladder; four QEMU-hidden causes fixed from the tree — see P3; seeded 2026-09-22 at Block 1 P0 as the B part of TASK-0260)
owner: @reliability @runtime
created: 2026-09-22
depends-on:
  - tasks/TASK-0260-provisioning-recovery-v1_0a-host-image-builder-flasher-protocol-deterministic.md
  - tasks/TASK-0246B-nxboot-sdhci-disk-reader.md
  - tasks/TASK-0245-bringup-rv-virt-v1_0b-os-kernel-uart-plic-timer-uartd-selftests.md
follow-up-tasks:
  - tasks/TASK-0251-display-v1_0b-os-fbdevd-windowd-integration-cursor-selftests.md
  - tasks/TASK-0327B-board-proof-lane-serial-ladder-profiles.md
links:
  - Decision: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C1, C2, C6, Phase 4)
  - nxboot: source/boot/nxboot (ADR-0059, RFC-0089); tools: scripts/build-fit.sh (new), scripts/board-flash.sh (TASK-0327)
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Boot flow"), resources/board/bpi-f3/PROVENANCE.md (FIT facts: OpenSBI at 0x0, payload at 0x0020_0000, `fdt_1 = k1-x_deb1`), docs/board/measurements/2026-09-26-boot-medium/README.md (TASK-0260 P0: the SPL finds `opensbi`/`uboot` through the GPT and selects a FIT configuration by name)
  - Playbook: CLAUDE.md
---

## RECUT 2026-09-27 — P0: the FIT measured, the payload's footprint, the first board attempt read from the trace

Measured before building (the pinned vendor FIT, `dumpimage -l`/`fdtget`, and nxboot's own link):

- The vendor FIT holds U-Boot and one tree per board variant. OpenSBI is not in it: the SPL
  loads OpenSBI from its own partition, `opensbi`. So our FIT holds **nxboot + our tree**, and
  OpenSBI stays the pinned `fw_dynamic.itb` (the earlier "OpenSBI + nxboot + dtb" is corrected
  here and in ADR-0066).
- The payload's shape: root `#address-cells = <2>`; the image `type = "standalone"`,
  `os = "U-Boot"`, `arch = "riscv"`, `compression = "none"`, `load = <0 0x200000>`, a crc32 hash
  (OpenSBI's next stage is the image with `os = "U-Boot"`). The configurations:
  `default = "conf_1"`, each `{description = "<board name>", loadables, fdt}`, and the SPL picks
  one by the board's name (`k1-x_deb1`). Our one configuration carries that name and is the
  default, so both paths take it.
- **The payload must cover its whole footprint.** The SPL places the tree right after the
  payload's bytes and then grows it in place. In U-Boot v2022.10 (the vendor's base),
  `common/spl/spl_fit.c` sets `image_info.load_addr = spl_image->load_addr + spl_image->size`
  and then calls `fdt_shrink_to_minimum(fdt, 8192)`. OpenSBI grows it again in place: in v1.3,
  `lib/utils/fdt/fdt_fixup.c` calls `fdt_open_into(fdt, fdt, fdt_totalsize(fdt) + 1024)` for its
  reserved memory, and +1024/+32 for the other fixups.
  - nxboot's `.bss` is `NOLOAD`: its heap (192 KiB), the trace's capture buffer (64 KiB) and its
    stack (16 KiB). A flat image ends at `__load_end` (130 KiB).
  - So the tree would sit inside `.bss`, zeroed by nxboot's first instructions, and the loader
    would reset before it has a console or a disk.
  - So the FIT's payload is nxboot's flat image padded to `__image_end` (416 KiB), its whole
    footprint, read from the ELF's own symbols.
- nxboot needs no platform split: it reads fw_cfg only when the tree lists a
  `qemu,fw-cfg-mmio` node (TASK-0245), and everything else already comes from the tree.
- The eMMC's boot configuration needs no change. Measured from the stock system (debugfs
  `ext_csd`): `PARTITION_CONFIG` [179] = `0x00` (BOOT_PARTITION_ENABLE 0), `BOOT_SIZE_MULT` 32
  (4 MiB), `BOOT_INFO` `0x07`.
  - That is the state the vendor's own eMMC flow leaves: `clear_emmc()` in its fastboot code
    sets `mmc_set_part_conf(mmc, 0, 0, 1)`, and nothing after it enables a boot partition.
  - So the boot ROM reads boot0 by partition access, not by the eMMC's boot operation.

### Packages

- **P1 — the FIT ✅ 2026-09-27.**
  - `config/board/bpi-f3/nexus.its` (the measured shape, one configuration named `k1-x_deb1`).
  - `scripts/build-fit.sh`:
    - nxboot from the last OS build, padded to its footprint;
    - the tree from `scripts/build-board-dtb.sh`;
    - `mkimage` with no build time recorded (`SOURCE_DATE_EPOCH=0`);
    - the shape checked with `dumpimage`, the size against the partition.
  - `just board-image` always builds the FIT and puts it into `uboot`.
  - nxboot's first line, `nxboot: platform=<root compatible> tree=0x<a1> size=<bytes>`, printed as
    soon as it has a console. It records which tree reached `a1` and where it lies (R1): on the
    board, where the SPL put it.
    - It is the first line in the trace, like every loader line.
    - It replaces RFC-0098 C8's planned `platform=<compatible> slot=<s>`: the slot is known only
      after the boot-selection block and is already on `nxboot: jump slot=<s>`, and the hart is
      always 0 once nxboot has moved itself there.
  - The build checks that the FIT's payload covers the footprint.
  - Proof:
    - two builds byte-identical (`c820eefe…`);
    - five mutations each refused by the build: the load address, no padding, the default
      configuration, a FIT larger than its partition, no nxboot;
    - the harness's new requirement run standalone: PASS, FAIL, SKIP;
    - `just check`, `ci-os-smp1` and `just test-all` EXIT=0 (12 QEMU runs, every trace contract
      with the platform line first).
- **P2 — the first eMMC boot ✅ 2026-09-27** (`docs/board/measurements/2026-09-27-first-emmc-boot/`).
  Each attempt:
  1. flash (FDL) and read back exactly;
  2. a baseline `just board-log`: the trace is empty;
  3. the microSD out, reset: the boot ROM boots the eMMC;
  4. the microSD in, reset;
  5. `just board-log`.
  - **Attempt A:** no boot kept, USB silent throughout (no fallback to the boot ROM's or the
    SPL's download mode), only the power LED.
    - Measured without changing anything:
      - the eMMC boot config is the vendor flow's own;
      - the SPL takes our FIT (its strings);
      - the pinned OpenSBI's K1 code reads nothing from the tree and it never reads
        `riscv,isa`.
    - **But OpenSBI runs on our tree, and our CLINT had no `interrupts-extended`.** OpenSBI
      v1.3's ACLINT timer then serves no hart, `init_coldboot` prints "timer init failed" and
      calls `sbi_hart_hang()`: a silent stop before our loader.
    - The fix: the CLINT names every hart's MSIP (3) and MTIP (7), and the UART also carries
      `spacemit,pxa-uart`, the name the firmware's console matches.
    - Held by a host test on the board golden (both mutations killed). `just board-goldens` in
      `just check` keeps that golden byte-equal to `board.dts`: it was never checked before,
      and `dtc` is now a core dependency and in CI.
  - **Attempt B, with the fixed tree:** the loader's six lines from the board.
    - The platform, and the tree at `0x268000`: R1 measured, right after the padded payload,
      where an unpadded one would have put it inside `.bss`.
    - The eMMC found through `mmc@d4281000`.
    - The boot selection block, verify ok, `/chosen`.
    - The jump to the verified kernel at `0x400000`.
    - One boot kept, so no reset loop.
- **P3 — the kernel on the board** (in progress 2026-09-28). The OS trace (TASK-0327B P2)
  showed the loader's lines and no OS text on every board boot: the kernel starts and never
  reaches the block owner. The RAM rescue (TASK-0327B P3) measured that the reset scrubs DRAM,
  so the kernel's earliest phase has no witness on the disk. **The boot LED ladder** is the
  instrument this package adds (no serial adapter at the desk):
  - The tree names the user LED (`/chosen/nexus,boot-led = <&gpio 3 0 0>`, `nexus,boot-led-pad`
    = the pad's mux register and value, both measured on the stock system) and gains the GPIO
    block (mainline shape; the K1 table learns its two APBC gates, mainline v6.16 `ccu-k1`).
  - nxboot brings the block up through the SoC glue, muxes the pad, sets the direction and
    pulses twice (`nxboot: boot led ok (…)` / `boot led none (<why>)`).
  - The kernel pulses `k` times at milestone `k` (`hal/boot_led.rs`): 1 platform from the tree,
    2 high half, 3 kmain, 4 init's segments copied, 5 init's image loaded, 6 init spawned, 7 selftests done + runtime. Busy waits on `time`; ~13 s on the board,
    a no-op on QEMU. How to read it: `docs/board/bpi-f3.md`.
  - `nexus-fdt`: `Chosen::boot_led()` / `boot_led_pad()`, golden-tested for both trees.
  - Not done: the kernel's tree-driven init (`hal::platform::init_from_fdt`) is not host-testable
    against the board golden — `mod hal` is target-only; its pure part would have to move out
    (as `phys.rs` did). A follow-up.
  - **What the ladder and a rescued console said (2026-09-28):** the LED ran `long, 1, 2, 3` and
    stopped, three times alike — the kernel reaches `kmain` and never `init loaded`. A quick
    reset kept the DRAM once and the next loader rescued the kernel's console: the platform
    from the tree, the high half, four harts online, every kernel selftest, init's segments
    mapped — and the text ends MID-LINE inside `init loaded …` (66 + 51 counted bytes lost).
    A death mid-line, at the same place three times, is a clock, not a code point.
  - **Not the SoC watchdog.** The vendor U-Boot's tree names `spacemit,k1x-wdt` at 0xd4080000
    and uses it for `wdt-reboot`, so nxboot names it in the tree and stops it first thing
    (`watchdog.rs`: the vendor driver's unlock words, then `ENABLE` = 0, read back). Measured:
    `nxboot: watchdog off (0xd4080000 was=0x0)` — it was not running; the ladder still stopped
    at 3. The stop stays (a chain that starts with it running would need it), the claim does
    not.
  - **A code point after all.** Two later rescues (the DRAM held twice in a row) end at exactly
    6482 bytes, on the complete line `map flags bits=0x17`: the kernel mapped init's third
    PT_LOAD segment (RW, 0x80c1d, starting on the page the read-only segment ends on) and died
    copying it — before `derived gp`. The earlier 117 "lost" bytes were the decaying DRAM's.
  - **The death moves.** With the segments' frames named and every page of the third segment
    logged (`KSELFTEST: segment copied (…)`, `shared page …`, `pg va=… pa=…` — the first two now
    contract markers in the manifest and in every lane's expected sequence, proven on QEMU),
    three rescued consoles of the same kernel ended at 6579, 6579 and 7875 bytes: two boots
    died at the same page of the third segment, the third 25 pages later. A death that moves
    is a clock's, not a code point's — and not the SoC watchdog's (off). The experiment: a
    long LED group before the segment loop; a board whose LED stops inside it died on a clock.
  - **The rescued bytes are not a death point.** Three rescues of one kernel differ in length
    and carry holes and bit flips inside otherwise intact lines: the ring is written through
    the D-cache, and whatever a line held that was not written back before the reset is lost
    or stale. The synchronous LED is the only position witness; the ring is a lucky transcript.
  - **The kernel reviewed against the board (2026-09-28), not QEMU.** Trap entry
    (`arch/riscv/trap.S`): full-context save, the `sscratch` swap idiom, direct-mode `stvec`
    — nothing QEMU-shaped. `fence.i` where copied bytes become instructions: nxboot's jump,
    the kernel's exec copy plan, the address-space copy. The kernel is `riscv64imac`: no FP or
    vector state to save, no `sstatus.FS` to enable. A PTE is present by its V bit, so DRAM at
    physical 0 (the board's first bank) is not a niche; user tables receive only the kernel
    half of the boot table (its identity gigabyte — gigabyte 0 on the board, where user
    addresses live — never reaches a user table); the tree's `opensbi@0` is carved out of the
    frame pool. **Found and fixed:** four literals that meant "10 MHz" — the bring-up wait
    budget (`budget_ns / 100`: 500 ms became 208 ms on the board), the 1 ms poll, the TLB
    shootdown ack budget, the BKL contention threshold, and the console's timestamps — all now
    `hal::platform::ns_to_ticks`/`ticks_to_ns` from the tree's timebase. None stops a boot by
    itself; every wait on the board ran at 42 % of its budget.
  - **No service is involved.** init is never spawned (the LED never reaches that milestone):
    no service has started when the kernel stops, so nothing service-side, and no "late
    service", can be the cause yet.
  - **The ladder, recut to encircle the stop.** Eleven milestones (long pulse = 5, short = 1):
    4 trap runtime installed · 5 secondary harts gated · 6 selftests begin · 7 a child task ran
    in user mode, exited, was waited · 8 init's address space · 9 segments copied · 10 spawn ·
    11 runtime; a panic flickers the LED forever (`hal::boot_led::fatal` from the panic
    handler). The per-page `pg` lines are gone (bytes, not evidence).
  - **The tree decides the harts.** `nexus-fdt` `Cpus::harts()` skips `status = "disabled"`
    (fixture `cpus-disabled.dts`), the kernel keeps a hart mask from the tree and
    `start_secondary_harts` never starts a disabled one (`KINIT: hart{n} disabled by the tree
    — not started`): the single-hart experiment is a tree edit, never a kernel patch.
  - **The ladder lied after the table switch (found 2026-09-28, the recut's first cycle).**
    The recut kernel showed 1·2·3 and stopped before 4 — exactly as every kernel before it
    had stopped before *its* next milestone, wherever that milestone stood (after the segment
    loop, before it, after the trap runtime). The common point is not a place in the boot: it
    is the first LED pulse after the kernel adopts its runtime page table. That table maps the
    device windows the tree names for the kernel — the console and the PLIC — and nothing
    else; the boot table's gigabyte leaves had covered the GPIO block by accident. The first
    pulse after the switch was a store to an unmapped window: page fault, panic, and the
    panic's own LED write faulted again. "The death moves" moved with the milestone. Fix:
    the LED's GPIO window is a kernel device window like the console's
    (`hal::boot_led::window()` → `mm::kernel_layout`). What the board kernel does after the
    switch is unmeasured until this cycle boots.
  - **ROOT CAUSE of the board kernel's stop (2026-09-28, six cycles with the ring written
    back per byte):** the kernel ran its whole selftest ladder and stopped ~16–20 ms after
    init's spawn, mid-line, whatever the configuration (one hart or four, LED on or off,
    UART poll bounded, PMU power-down bits already clear). With S-mode interrupts held off
    from `plic_init` to the runtime, it reached milestone 11 and stamped `sip=0x20`: a
    pending supervisor timer. Re-enabling interrupts killed it at once. The timer was armed
    through `stimecmp` (the tree lists Sstc) but re-armed in the tick handler through
    `sbi::set_timer`; the board's firmware does not fold `set_timer` onto `stimecmp`, so the
    compare stayed in the past and the first tick after that point stormed the hart in
    silence. QEMU's bundled firmware does fold it, which hid the mix for a year. Fix: ONE
    arming path, `hal::platform::arm_timer_ticks`, in the handler and the bring-up park
    (`trap/handler.rs`, `smp/bringup.rs`). Then the board booted init: `init: entry` …
    `init: ready`, `policyd: ready`, `abi-profile: ready`, `mmio policy deny ok`, `device
    tree grant ok` — userspace on the board (`docs/board/measurements/2026-09-28-dram-retention/`).
  - **Then two board-shaped refusals in userspace, both QEMU literals:** `hal::plic::MAX_IRQ = 95`
    (virt's source count) rejected blkd's device descriptor — the storage host is source 101 of
    159 (`init: mmio cap_create err svc=blkd err=abi:invalid-argument`, init-lite fatal); the
    bound is now the tree's `plic_ndev()`, the tables sized to the PLIC's ceiling (1023). And
    init refused any block whose registers start inside a page (`base % PAGE != 0`): the
    APMU syscon at 0xd4282800 (`socd: window for Apmu not granted`); a window is now the
    pages the registers occupy and socd applies the in-page offset from the tree node.
  - **With those two, the board's disk path came up** — `init: boot disk ok
    (kind=spacemit,k1-sdhci record=/soc/storage-bus/mmc@d4281000 irq=101)`, `socd: ready
    (providers=6)`, `socd: bring-up /soc/storage-bus/mmc@d4281000 ok (domains=1 resets=2
    clocks=2)` — and blkd took a user page fault on its very first store into the DMA table
    it had just mapped (`[USER-PF] STORE @ sepc=0x2af20 stval=0x50005001`, the ADMA2
    descriptor attribute `0x21` written byte-wise at the table's first page; blkd's tree alias,
    MMIO window and table lie at 0x50000000, 0x50004000, 0x50005000). The kernel had no
    `sfence.vma` after any PTE change (`mm/*`: none): RISC-V lets the walker cache a miss,
    QEMU never does. `vm_ops::fence_asid` now follows every `map_runs` and
    `set_leaf_flags` (Linux's `update_mmu_cache` fences for the same reason).
  - Proof (2026-09-29, `docs/board/measurements/2026-09-28-dram-retention/board-boot-2026-09-29-first-os-trace.txt`):
    the board's trace reaches `init: ready`, `blkd: backend ok (kind=spacemit,k1-sdhci mode=hs400es
    bus=8 sectors=30535680)`, `blkd: gpt ok (parts=7)`, `blkd: trace os ok`, `bundlemgrd: system
    volume verified (bundles=23)`, the fleet `ready` through `stage: platform`, the selftest-client's
    ladder on the board (ipc bench rt=25us, vfs/pkgimg/sandbox ok), 111 idle heartbeats. Left for
    the next ledgers: `SELFTEST: dma buffer FAIL` / `cap query vmo FAIL` (virtio-shaped probes),
    a spawned child's null load (`[USER-PF] LOAD sepc=0x1020 pid=0x32`), rngd without an entropy
    source, the board's self-reset after a hang (source unknown; the SoC watchdog is off).
  - **Verified with the board's own tree (four harts, the LED) 2026-09-29 09:54**
    (`board-boot-2026-09-29-verified-4harts-led.txt`): the ladder to `2 long + 1` read by a person,
    `KGATE: smp bringup ok mask=0xf`, `plic ctx cpu0..3 ok`, `plic isolation ok`, milestone 11 at
    23.4 s, `init: ready`, `blkd: backend ok`, `gpt ok`, `blkd: trace os ok (slot=1 seq=2)`, the
    planes declared absent, the OS trace on the eMMC.
  - **Measured, not fixed: an eMMC boot right after the stock system ran loops in the loader.** An
    eMMC boot left untouched for three minutes (2026-09-29, `board-boot-2026-09-29-untouched-3min-panic-loop.txt`)
    held four loader boots, each `bsb ok (slot=a seq=8)` → `verify FAIL (slot=a nxbd)` → `fallback ->
    slot=b` → `verify FAIL (slot=b nxbd)` → `nxboot: PANIC (both slots bad)` → SBI reset → again. Across
    every kept boot the verdict correlates with one thing: the boots that fail are the ones whose DRAM
    probe reads `lost` — the boots right after the stock system had run (its kernel used the DRAM) —
    2 of 2; every boot after `fastboot reboot` (the flash vehicle) and every warm restart of our own
    chain (`kept`) verifies, 12 of 12. The stock kernel leaves the eMMC in a state nxboot's reader
    cannot read the slot descriptor from, and the state survives the SoC reset (the SPL still loads
    the FIT from the same card). Next: nxboot names the failing check of `verify_nxbd` and the
    sector's first bytes in its trace, then the card init gets its fix (the full reset the SPL's
    driver performs) — the board proof lane's package (TASK-0327B P4). The earlier suspicion of a
    PMIC watchdog is dropped; the SoC watchdog is off.
  Done: the board's trace reaches `init: ready`, with this ledger's Definition of Done.

## Context (measured 2026-09-22)

The SPL loads two FITs from the `opensbi` (1 MiB) and `uboot` (2 MiB) partitions; OpenSBI
jumps to the `uboot` FIT's image (load `0x0020_0000`) in S-mode with the FIT's selected DTB in
`a1`. The vendor payload is U-Boot; ours is nxboot. On QEMU nxboot is the `-kernel` payload
and the only place that may read fw_cfg (RFC-0098 C2).

## Context added 2026-09-26 (TASK-0260 P0, measured)

The FIT goes into the head partition the SPL knows as `uboot` (2 MiB at 2 MiB; partition 2 since
TASK-0260 P2): the SPL loads it by that name (`CONFIG_SYS_LOAD_IMAGE_SEC_PARTITION_NAME`, the vendor's
defconfig), whatever the slot holds. The SPL picks a FIT configuration by name (`Boot from fit
configuration %s`) — which name it asks for, and so which configuration our single-config FIT must
answer (or its default), is measured with the first boot (TASK-0260 P3). (Since P1, `just
board-image` always builds the FIT and puts it into the board image; `just board-flash --plan`
writes it.)

## Goal

- `scripts/build-fit.sh` (`mkimage`): `config/board/bpi-f3/nexus.its` → a FIT with image
  `nxboot` at `0x0020_0000` (the slot the SPL loads) and our `board.dtb` (padded with `/chosen`
  headroom); `nx image` places it in the FIT partition of the layout SSOT (TASK-0260's head).
- nxboot: `platform/{qemu,board}.rs` — on QEMU read fw_cfg (`selftest-profile`,
  `display-mode`) and write `/chosen/nexus,boot-profile` / `nexus,display-mode`; on both write
  `nexus,boot-slot` and `nexus,boot-record`; hart lottery from `/cpus`; self-relocation from
  the FIT load address (position-independent, TASK-0245); disk via TASK-0246B; SBI SRST on
  failure as today.
- The kernel's `/chosen` readers (TASK-0245) are the only consumers.
- Proof: QEMU A/B + NXBD lanes unchanged with the `/chosen` path; **board**: `nxboot:
  platform=spacemit,k1-x slot=a` → kernel banner → `KSELFTEST: platform from fdt ok` →
  `init: ready` on the serial console — measured R1 (which DTB reached `a1`) recorded here.

## Non-Goals

Signed FIT / secure boot (follow-up), SPI-NOR boot, replacing the SPL or OpenSBI (ADR-0066),
recovery over the USB gadget (TASK-0261, parked).

## Definition of Done

FIT built reproducibly (`sha256` stable across two builds); `just board-flash` writes the
image; the board's serial ladder reaches `init: ready`; ADR-0066 Accepted; RFC-0098 Phase 4 ✅;
`docs/board/bpi-f3.md` boot section rewritten from "vendor chain" to "our chain".
