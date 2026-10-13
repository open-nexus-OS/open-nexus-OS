---
title: TASK-0329 GPU device online — the thin side in gpud: the published firmware, the GPU MMU with explicit VM, power through socd, the interrupt, the fence bridge (Block 4 layer 3, RFC-0101 Phase 2)
status: Done 2026-10-13 (P0 rewrite to the end state 2026-10-13 — this file is the SSOT of Block 4 layer 3; G1 green on the board in cycle 5, `board--2026-10-13T13-46-51`; G5 = TASK-0329B stays open)
owner: @runtime @ui
created: 2026-09-21
depends-on: [TASK-0280 (layer 1, the contracts ✅), TASK-0216 (layer 2, the scanout contract ✅), TASK-0245B (socd, the display set ✅), TASK-0284/0284B (`DmaBuffer`, cache maintenance ✅)]
follow-up-tasks: [TASK-0280 G2 (the device behind `GfxDevice`), TASK-0329B G5 (app-surface import), TASK-0330 S3 (the IPC call budget on the board)]
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md "Block 4 in layers", layer 3
  - Contract: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (Phase 2)
  - SoC glue: docs/rfcs/RFC-0106-… (Phase 2 per-class floor, Phase 3 GPU set)
  - Playbook: CLAUDE.md · method: .claude/skills/driver-bringup/
  - Measurements: docs/board/measurements/2026-10-10-gpu-stock-baseline/ (G0), docs/board/measurements/2026-10-13-gpu-online/ (G1's cycles)
  - Seam: source/libs/nexus-driverkit/src/gpu.rs (the contract types), userspace/nexus-gfx/src/backend/gpu/ (the host model), source/drivers/gpud/src/backend/gpu/ (the device)
  - Firmware: resources/firmware/gpu/ (the published blob, its licence, its provenance; gate `scripts/check-firmware-provenance.sh`)
---

## Origin

Extracted from `tasks/TRACK-DRIVERS-ACCELERATORS.md` CAND-DRV-010 for target picture G of the
hardware fast track. Recut 2026-10-12 into Block 4's layers: layer 3 is "the GPU online" — the
device answers, nothing is drawn yet (layer 4 draws the first quad, layer 5 the desktop).

## Context

gpud has a display backend (`dc`, the board's controller: the plane ring flipped at the vertical
blank, layer 2) and two virtio backends (QEMU). The board's GPU — one core of the B-series XE
family, id `36.29.52.182`, a MIPS firmware processor, 40-bit GPU virtual addresses, 36-bit
physical bus, `dma-noncoherent` — has never been touched by our chain. The end state keeps the
gpud / driver-kit / nexus-gfx split (RFC-0101): a thin side that owns firmware, the GPU MMU with
explicit VM management, submission rings, the interrupt and the fence bridge; the userspace
driver (layers 4–5) owns command encoding and offline shader binaries.

The reading list is the open kernel driver (dual GPL/MIT — the device logic is portable under
MIT) and the published firmware's documented host interface; the vendor kernel is reference
only. What they say about this core, its firmware format, power-up, MMU, submission and
interrupt is distilled below with its provenance; every number our bring-up compares against is
the stock system's own (`2026-10-10-gpu-stock-baseline/gpu-registers-running.txt`).

## Goal

The device online on the board, host-first:

- `gpud: gpu glue ok (` — socd brought the GPU node up (power domain 2 by its software
  sequence, `gpu_clk` at 614.4 MHz, the reset released at least 32 clock cycles after the clock);
- `gpud: gpu core ok (bvnc=36.29.52.182 …)` — the window maps, the core id reads the measured
  word (the domain is really on);
- `gpud: gpu fw ok (boot_us=…)` — the published firmware loaded by its documented layout, the
  MIPS boot data and page table built, the start sequence run, `firmware_started` observed,
  the first interrupt served;
- `gpud: gpu mmu ok (` — a VM context (page catalogue) with a VMO page bound at a GPU address
  through `nexus_driverkit::gpu::VmSpace`, the firmware's MMU-cache flush acknowledged in its
  return slot, the page unbound and flushed again;
- `gpud: gpu fence ok (rt_us=… n=…)` — a null job (a client command buffer holding one timeline
  update, kicked through the kernel command buffer) completes: the firmware writes the timeline
  word, the interrupt fires, gpud signals the kernel timeline fence (RFC-0033) — the fence
  bridge; the mean round trip is the frame's fixed cost on our side, bar: **below the stock
  stack's 0.64 ms**.

No pixel is drawn here. Layer 4 (G2, TASK-0280) puts `GfxDevice` on this device.

## Requirements the base structure carries (from the planning of the layers above, 2026-10-10)

Unchanged from the seed, now with their place in this task:

1. **One context per client with its own GPU address space** — the VM context here is one page
   catalogue; the firmware memory context object names it; the firmware assigns the hardware
   catalogue slot when it schedules a context (the host never writes `BIF_CAT_BASE` for MIPS).
   G1 builds one context (gpud's own, for the null job); the type is per client from the start.
2. **Queue kinds in the submit contract** — render, compute, transfer (`nexus_driverkit::gpu::QueueKind`,
   layer 1). G1's null job runs on a transfer-fragment context; compute is declared, refused by
   name until G3 (`test_reject_compute_submit_before_g3`).
3. **One sync primitive: the kernel's timeline fence** — the firmware's timeline word per queue
   (a 32-bit "UFO" in firmware-visible memory) is bridged to a kernel fence by gpud's interrupt
   handler: `fence_signal(fence, word)`. No second fence kind.
4. **Buffers are VMOs bound with `vm_bind`, each with a cache mode** — the firmware heap, the
   command buffers and the page tables are page-backed VMOs (`vmo_create_for`, their runs by
   `vmo_runs`); everything the firmware reads or writes is device-uncached on the GPU side
   (SLC bypass) and either uncached or Zicbom-maintained on the CPU side (`DmaBuffer`); the
   measured truth is `dma-noncoherent`.
5. **Priority, preemption, timeout, reset per context** — the firmware context carries
   `priority` and `max_deadline_ms`; whether the firmware preempts between contexts is
   measured when two contexts run (layer 4's first cycle), recorded in `Capabilities`.
6. **One kernel crossing per batch** — the kick is one register write after the command buffers
   are written in gpud's memory; no syscall per command.
7. **GPU code is client data** — nothing here; the firmware blob is the device's, not a shader.

## What the open sources say about this core (the reading list, distilled; 2026-10-10 research)

Provenance: the open kernel driver at mainline `789f290b` (2026-10-08), its register and
firmware-interface headers; the published firmware tree at `8a58f818` (2026-04-15); the open
userspace driver's device-info tables. The stock system's live registers (G0) are quoted where
they confirm or contradict.

- **Support status.** The open kernel driver lists three cores by id and treats ours as
  unknown (a module parameter bypasses the check); the open userspace driver has our device
  info (public name BXE-2-32) and calls it "unsupported, not under active development". The
  published firmware for exactly our id exists in the vendor's firmware tree
  (`powervr/rogue_36.29.52.182_v1.fw`, 126 976 bytes, version 1.1 build 6976702, sha256
  `1ca06dd4…`, licence: binary redistribution without modification, no disassembly). Nobody
  upstream claims the open firmware + open driver were tested on this core; Mesa says so. Our
  first cycle is that test.
- **Firmware image format** (`pvr_fw_info.h`): an ELF32 padded to 4 KiB blocks; at
  `size − 4K` an info header (`info_version 3`, header 48 B, up to 8 layout entries of 24 B,
  the id, `fw_page_size 4096`, flag bit 0 = open-source build, version 1.1); the layout table
  follows it; a device-info block (BRN/ERN/feature masks + parameters) precedes the header.
  Our blob's six layout entries: MIPS code at FW virtual `0xC0000000` (alloc `0x17000`),
  exceptions `0x9FC02000`, boot code `0xBFC00000`, private data `0xC0032000` (`0x3000`),
  boot data `0xBFC01000`, stack `0xCF600000`. Code `0x19000`, data `0x5000` in total.
  Features that matter here: MIPS processor, `LAYOUT_MARS 1`, one cluster, 36-bit physical
  bus, 40-bit VA, SLC 16 KiB / 512-bit lines, `PBE2_IN_XE`, `XE_TPU2`, `SYS_BUS_SECURE_RESET`,
  `PBVNC_COREID_REG`, `IRQ_PER_OS`, `COMPUTE`; kernel quirk `BRN 63553` (an extra MIPS remap so
  address 0 does not fault); no watchdog timer, no ECC (no safety events).
- **Loading (MIPS).** A 16 MiB firmware heap at GPU virtual `0xE1C0000000` (MIPS virtual base
  `0xC0000000`), mapped by the firmware's own page table: up to 4 pages of 32-bit entries, one
  per 4 KiB page, entry = `(phys >> 12) << 6` with flags valid|dirty|global and the cache policy
  in bits 3..4 (uncached for everything firmware-shared). The host copies each ELF `PT_LOAD`
  into the layout section holding its virtual address, fills the boot data at the start of the
  boot-data section (`stack_phys, reg_base = the register window's physical address,
  pt_phys[4], pt_log2_page_size 12, pt_num_pages`), and creates the firmware structures the
  interface fixes: connection control, OS init, SYS init at the top 192 KiB of the heap ("the
  config heap", three 64 KiB slots), KCCB (128 slots) + return slots, FWCCB (32 slots), power
  sync word, HWR info, MMU-cache sync word, sysdata, a fault page, trace buffers, runtime config.
  Host-written init values worth mirroring: `pds_exec_base 0xDA00000000`, `usc_exec_base
  0xE000000000`, `initial_core_clock_speed = 614 400 000`, `firmware_started = false`,
  `align_checks 0`, `wdg_period_us 0`, `active_pm_latency_ms 0`.
- **Start sequence** (`pvr_fw_startstop.c`, our feature set): `SYS_BUS_SECURE (0xA100) = 0`;
  `SOFT_RESET (0x0100, 64-bit) = 0xFFEFFFFFFFFFFC3D`, `SOFT_RESET2 (0x0108) = 0x1FFFFF`;
  rascal/dusts out; everything out but the firmware processor (`SOFT_RESET = 1 << 32`);
  `SLC_CTRL_MISC (0x3800)` low = hash-1 decode | burst-combiner bypass (**stock: `0x200003`**);
  the MIPS wrapper: `MIPS_WRAPPER_CONFIG (0x0810) = (0xCF800000 >> 16) | microMIPS` (**stock:
  `0x1CF80`**), remaps 1/2/3 for boot code / boot data / exception code (`CONFIGn1 = MIPS
  address | enable`, `CONFIGn2 = physical | 12`; **stock: `0x1FC00001/…800C`,
  `0x1FC01001/…800C`, `0x1FC02001/…700C`**), remap 5 for BRN 63553 (**stock: `0x1 / …800C`**),
  `MTS_GARTEN_WRAPPER_CONFIG (0x0B50) = 0`, `MIPS_DEBUG_CONFIG (0x08C8) = 0`;
  `AXI_ACE_LITE_CONFIGURATION (0x38C0)` to the fixed "outer cache" word; 3 µs; `SOFT_RESET = 0`;
  3 µs. Liveness: `sysinit.firmware_started` within 5 s. **The stock stack runs the same
  sequence** — the live words match the open driver's values bit for bit where both write.
- **Power** (the manual sequence the binding describes): domain on → core clock on → ≥ 32 clock
  cycles → reset released; off in reverse. Our SoC: domain 2 is software-sequenced in the APMU —
  control word `0x0d0`, sleep1 bit 2 set, 20 µs, sleep2 bit 3 set, 20 µs, isolation bit 1 set
  (released), 10 µs, status `0x0f0` bit 0 polled (the vendor's domain driver, read as reference;
  stock live: control `0xe`, status bit 0 set; our chain boots with it off). The stock driver
  timed its own power-on at 155 µs + 226 µs; firmware boot 354 timer ticks (≈ 150 µs at
  416.67 ns a tick).
- **Interrupt.** One line (`75`); status `MIPS_WRAPPER_IRQ_STATUS (0x08A8)` bit 0, cleared by
  writing bit 0 to `MIPS_WRAPPER_IRQ_CLEAR (0x08B0)`; the handler loops "clear, drain, re-check"
  so an event during processing is not lost. One interrupt means "the firmware did something":
  KCCB progress (return slots), an FWCCB message, a timeline word. `sysdata.interrupt_count` is
  the firmware's own count — our gate compares it with ours.
- **MMU.** 40-bit VA, three levels: page catalogue (1024 × 32-bit, 1 GiB each), page directory
  (512 × 64-bit, 2 MiB each, page size in bits 3..1), page table (512 × 64-bit for 4 KiB pages;
  entry: physical page in bits 39..12, bit 3 SLC bypass, bit 1 read-only, bit 0 valid; the open
  driver writes cache-coherent = 0 always). Fixed heaps: GENERAL `0x80_0000_0000` (128 GiB),
  PDS `0xDA_0000_0000`, USC `0xE0_0000_0000`, FIRMWARE `0xE1_C000_0000`, TRANSFER_FRAG
  `0xE4_0000_0000`. Every table change is followed by a KCCB `MMUCACHE` command (flags PT/PD/PC
  + TLB + interrupt) whose return slot the host waits for; an unmap is always followed by a
  waited flush. A flush that times out is treated as a firmware lock-up.
- **Submission.** KCCB (host → firmware, 128 commands, a control block of write/read offsets,
  a return-slot word per command the firmware sets to `CMD_EXECUTED`); FWCCB (firmware → host,
  32); per queue a client command buffer (32 KiB byte ring, commands never wrap — padding fills
  the tail) and a firmware common context (`dm`, `priority`, `max_deadline_ms`, the memory
  context, a register-state save area, the 4-byte timeline word). A job = optional `FENCE_PR`
  waits + the work command + an `UPDATE` writing the sequence number to the timeline; then one
  KCCB `KICK` and one register write `MTS_SCHEDULE (0x0B00) = 0`. Command type values carry the
  magic `0x2ABC << 16`. Completion = "timeline word ≥ sequence number", read after the
  interrupt — never a per-job record.
- **What is not open:** the firmware's default preemption and priority behaviour (the interface
  declares context-switch enables and profiles; the open driver sets none). Measured, not
  promised (layer 4's first cycle with two contexts).

## Three lenses (architecture-review, before code)

- **Scope.** In: `resources/firmware/gpu/` (blob + licence + README + the provenance gate);
  `userspace/nexus-gfx/src/backend/gpu/` (the host model: firmware image, MIPS boot, the start
  sequence, the interface structs with offset asserts, the command buffers, the page tables);
  `source/drivers/gpud/src/backend/gpu/` (the device: glue, core, memory, firmware, interrupt,
  MMU, the null job, the fence bridge, markers); `source/libs/nexus-abi/src/mmio.rs` +
  `nexus-driverkit/src/mmio.rs` (64-bit register access — the GPU's reset and clock words are
  64-bit and must not be written as two halves); `source/libs/nexus-soc` (domain 2's software
  sequence as a plan step); `source/init/nexus-init` (the GPU window from the tree into gpud's
  slot, class `device.mmio.gpu`); `nexus-service-topology` (two slots); policyd (the class, and
  RFC-0106's per-class floor `soc.glue.<class>`); the proof manifest, `board-test.sh`,
  `IMPLEMENTATION-ORDER.md`, RFC-0101 Phase 2, RFC-0106 status, CHANGELOG. Out: anything that
  draws (G2–G4), the v2 `GfxDevice` on the device (G2), compute execution, shader binaries, the
  firmware trace decoder (a follow-up when a cycle needs it), the scanout path (layer 2, done),
  app-surface import (G5).
- **Invariant.** The GPU never sees a physical address the kernel did not hand to gpud for the
  GPU's device capability (`vmo_runs` with the GPU's cap; a run outside its reach is refused by
  the kernel) and never a mapping outside a `VmSpace` binding
  (`test_reject_bind_outside_reach`, `test_reject_unbind_unbound`, `test_reject_overlap`,
  layer 1 + this task's page-table builder: `test_reject_unaligned_va`,
  `test_reject_va_beyond_40_bits`). The firmware blob loaded is the one the provenance file
  names (`test_reject_firmware_wrong_id`, `test_reject_firmware_not_open_build`,
  `test_reject_firmware_bad_trailer`: the parser refuses a blob whose header id, flag or
  version differs — before a byte reaches memory). Identity for socd and init stays the
  kernel's `sender_service_id`; gpud may bring up only GPU and display nodes
  (`test_reject_soc_glue_class_mismatch`). No `unwrap` on the blob or on firmware-written words.
- **Contract.** RFC-0101 Phase 2 (this task's rows written there: the firmware's place, the
  heaps, the memory context, the fence bridge, the gates); RFC-0106 Phase 2's per-class floor
  (first consumer) and Phase 3's GPU set; RFC-0098 C3 (the window and the line from the tree);
  RFC-0033 (the timeline fence); ADR-0046 (budgets). No new ADR: no boundary moves — init grants
  a window as it grants the display's, gpud speaks to socd over the existing client, the kernel
  ABI is unchanged.

## Packages (host-first; one board cycle per hypothesis set)

- **P0 — the SSOT and the firmware** ✅ 2026-10-13: this rewrite; RFC-0101 Phase 2 extended;
  `resources/firmware/gpu/rogue_36.29.52.182_v1.fw` + `LICENSE` + `README.md` (source tree,
  commit, sha256, version) + `scripts/check-firmware-provenance.sh` (every blob under
  `resources/firmware/` has a README naming its sha256 and licence, and the hash holds) in
  `just check`; the measurement folder seeded with the hypotheses the first cycle decides.
- **P1 ✅ 2026-10-13** — 34 unit tests + 8 goldens over the published blob (`tests/gpu_goldens.rs`:
  the trailer, the six-section layout, the device-info values the research read — MIPS, 36-bit
  bus, 40-bit VA, SLC 16 KiB/512-bit, quirk 63553 —, every segment inside its region, the stock
  remap/wrapper/SLC words from the start sequence; `test_reject_firmware_*`, the MMU reject
  matrix, the client ring's wrap rule). The interface structs are byte encoders at the pinned
  offsets (no `unsafe`, no transmute).
- **P2 ✅ 2026-10-13** — `nexus-soc`: `DomainOn::Software` + `Step::DomainOnSoftware` /
  `SettleUs` (`Pause::pause_us`), the GPU domain in the K1 table, `nexus,clock-before-reset`
  (gates first, 1 µs, then the reset), `class.rs` (compatible → class); the GPU node plans
  `[domain 2 software, gate 0xcc bit 4, settle 1 µs, reset bit 1, rate pll1_d4/1]` and from cold
  writes `0x4, 0xc, 0xe` then `0x10, 0x12` — the stock words (`tests/k1/gpu.rs`, 35 tests).
  socd: the per-class floor (`verdict::required_capability`, an empty plan `NotNeeded` without a
  policy trip, `test_reject_soc_glue_class_mismatch`, 12 tests); `policies/base.toml`:
  `soc.glue.storage|usb|display|gpu`. init: the `img,rgx` window into the GPU's slot
  (`init: gpu plane from tree`). Topology: `gpud::GPU_IRQ` (0x2a) as a declared interrupt-notify
  endpoint. `MmioWindow::{read64, write64}` + `Mmio::{read64, write64}`. Board tree: the GPU
  node's rate, order and coherence; golden regenerated.
- **P3–P6 ✅ 2026-10-13, five board cycles** (`docs/board/measurements/2026-10-13-gpu-online/`):
  `gpud::gpu::{glue, regs, memory, firmware, device, job}` — one DMA VMO (128 pages) backs the
  firmware heap page by page with explicit cache maintenance; every wait is the declared
  watchdog one-shot on a waitset beside the interrupt endpoint (the repo's wait rule). Cycle 5:
  `gpu glue ok`, `gpu core ok (bvnc=36.29.52.182)`, `gpu fw ok (boot_us=10862 health_us=82 irqs=1
  fw_irqs=1 executed=1)`, `gpu caps (… cache_block=64 …)`, `gpu mmu ok (map_us=57 unmap_us=50)`,
  **`gpu fence ok (rt_us=131 worst_us=3190 n=64 irqs=64 below the stock 640us)`** — the null
  job's round trip 4.9× below the stock stack's fixed cost of a frame; 55 rungs, no FAIL marker.
  What the cycles decided: a double reservation of the config slots (1); the health check's
  liveness is `kccb_cmds_executed`, not its return slot (2); the MMU-cache flush between mapping
  firmware objects and the kick is mandatory — a firmware page fault, message 112, without it
  (3); an `UPDATE` alone is no job (4); the interface's `NULL` task plus its update is (5). The
  QEMU `visible` lane caught a stack overflow of gpud (the GPU state in the service loop's
  static frame) — the slot tables moved to the heap, the state is boxed, the ladder never inlined.
  Handed on: firmware-shared memory needs an uncached user mapping for several commands in
  flight (TASK-0290 input, M2); the firmware's boot time varies tenfold between boots (1.1 / 23 /
  10.9 ms — layer 4's first cycle, with the firmware trace).
- **P1 — the host model** (`nexus_gfx::backend::gpu`): `fw_image` (the trailer, the layout
  table, the device-info masks, the ELF32 program headers → placements; goldens = the published
  blob itself, read from `resources/`), `mips_boot` (boot data words, remap register pairs, the
  firmware page table entries; goldens = the stock remap words with their physical halves
  substituted), `start` (the start/stop sequences as recorded writes over `RegWriter`; goldens =
  the open driver's values confirmed by the stock words), `fwif` (the interface structs as Rust
  with `const` size/offset asserts mirrored from the MIT-licensed check headers), `ccb` (the
  kernel and client command buffers as pure rings over a byte slice: reserve, write, return
  slots, wrap padding), `mmu` (page catalogue / directory / table builders over a page
  allocator trait; `VmSpace` from layer 1 is the bookkeeping, this is the encoding). Gate:
  `just test-host` with the reject matrix above by name.
- **P2 — the SoC side**: `nexus-soc` domain 2 as `DomainOn::Software` (the vendor sequence with
  its settle times as plan steps, the status bit polled with a bound), the GPU node's plan =
  domain → clock → ≥ 32 cycles → reset; socd's per-class floor (`soc.glue.<class>`, class from
  the node's compatible; policyd rules for blkd/gpud/xhcid; `SELFTEST: soc glue class deny ok`);
  init grants the `img,rgx` window to gpud (`device.mmio.gpu`, slot `gpud::GPU_DEVICE`) with its
  line; `nexus-abi::MmioWindow::{read64, write64}`.
- **P3 — the device online** (`gpud::backend::gpu`): glue → window → core id → firmware memory
  (page-backed VMOs, uncached on both sides, `vmo_runs` for the physical pages) → load → start
  → the interrupt bound to `gpud::GPU_IRQ` on the service waitset → `firmware_started` seen on
  the first interrupt (a bounded wait on the waitset, never a spin) → `gpud: gpu fw ok
  (boot_us= irqs=)`; the interrupt handler: clear, drain FWCCB, read return slots, read
  timeline words, signal fences, re-check. FAIL lines carry `MIPS_EXCEPTION_STATUS`,
  `MARS_IDLE`, the boot-stage word and the fault registers.
- **P4 — the MMU on the device**: one VM context (page catalogue + firmware memory context), a
  VMO page bound through `VmSpace` and encoded into the tables, `MMUCACHE` flush waited on its
  return slot, unbind + flush → `gpud: gpu mmu ok (map_us= unmap_us=)`.
- **P5 — the null job and the fence bridge**: a transfer-fragment firmware context with its
  client command buffer and timeline word; the job = one `UPDATE`; `KICK` + `MTS_SCHEDULE`;
  interrupt → timeline word → `fence_signal`; 64 round trips → `gpud: gpu fence ok (rt_us=
  n=64)`; the bar 640 µs from G0. The device's `Capabilities` filled from the firmware's
  device-info block and reported once (`gpud: gpu caps (…)`).
- **P6 — the board cycles**: cycle 1 decides H1–H5 below; the gate shown red once on a real
  fault before green; `[PASS] board-visible` with the new rungs; docs closure.

## Hypotheses the first cycle decides (driver-bringup step 2)

| # | Hypothesis | Decision line in the trace |
|---|---|---|
| H1 | Domain 2 comes up by the vendor's software sequence through socd (sleep1, sleep2, isolation released; status bit 0) | `socd: bring-up …gpu… ok` with `0x0d0 … >0xe`, `0x0f0` bit 0 set; else a named fault with the status word |
| H2 | The register window answers once the domain and clock are up | `gpud: gpu core ok (bvnc=36.29.52.182 id=0x0024001d003400b6)`; all-ones = the domain or clock is not up (H1 wrong, or the reset not released) |
| H3 | The published firmware boots on this core with the open start sequence | `gpud: gpu fw ok (boot_us=…)`; FAIL with `MIPS_EXCEPTION_STATUS`, `MARS_IDLE`, the boot-stage word: a remap or boot-data fault (our physical addresses), or a core/firmware mismatch nobody has tested |
| H4 | Line 75 reaches gpud through the kernel's PLIC path and the firmware's `interrupt_count` matches ours | `gpud: gpu fw ok (… irqs=N fw_irqs=N)`; `irqs=0` with `fw_irqs>0` = the line (PLIC enable, hart) — the console's lesson: read the enable back |
| H5 | Firmware-shared words are seen by the CPU without stale cache lines when mapped uncached (non-coherent SoC) | the KCCB return slot flips to `EXECUTED` for the first `HEALTH_CHECK` within its bound; a slot that never flips while `interrupt_count` advances = a cache mode fault |

## Gates

- Host: `just test-host` — the goldens and the reject matrix above by name.
- QEMU: unchanged (virt has no such GPU; `init: gpu plane none` stays; every lane green).
- Board (`board-headless` and `-visible`, REQUIRED rungs): `gpud: gpu glue ok (`, `gpud: gpu
  core ok (`, `gpud: gpu fw ok (`, `gpud: gpu mmu ok (`, `gpud: gpu fence ok (`; `gpud: FAIL
  gpu ` is a FAIL marker (the FAIL gate catches it); `rt_us` in the fence line below 640 is the
  verdict against the stock stack (printed in the line, judged in the measurement README).
- Measurement folder `docs/board/measurements/2026-10-13-gpu-online/`: Question → Instrument →
  Results → Verdict, the hypotheses table filled per cycle, the transcript lines archived.

## Non-Goals

The userspace driver (G2 = TASK-0280 v2 API on the device, G3 = 0169B/0170B/0171, G4 =
0215/0216), an on-device shader compiler, the firmware trace decoder, gaming-class features,
any implicit sync, the stock firmware (closed interface; not redistributable as far as known).

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + licence and a gate;
  never disassembled; redistributed unmodified with the licence text.
- **Vendor kernel code is reference only**; the open kernel driver's device logic is portable under
  its MIT option (keep the notices); openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input (the blob, firmware-written words);
  `forbid(unsafe_code)` in userspace crates except the one documented MMIO/DMA seam per driver.
- **Legal**: no company or product names in code, comments or docs; compatible strings, file names
  and register names are data.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite
  — G0 ✅ 2026-10-10; the power-up register window is G1's first cycle (the stock system cannot
  show the domain off).
- **RED**: 64-bit registers written as two 32-bit halves — `SOFT_RESET = 1 << 32` would release
  every reset for the time between the halves. `write64` first.
- **YELLOW**: the firmware is untested on this core by anyone upstream; H3's FAIL form carries
  enough registers to tell our fault from theirs.
- **YELLOW**: a `MMUCACHE` flush that times out is a firmware lock-up upstream (hard reset, retry
  once). G1 names it and stops; the reset path is layer 4's.
- **GREEN**: the stock register dump shows the vendor stack running the open start sequence — the
  sequence is right for this silicon.

## Definition of Done

- ✅ The measurement folder exists and answers its question (power-up, boot, interrupt, the
  null job's round trip 131 µs against 640 µs).
- ✅ The host model reproduces the measured words (remaps, wrapper, SLC) and refuses the wrong
  blob (`tests/gpu_goldens.rs`).
- ✅ The board lane: `ladder complete: 55 rungs present`, no FAIL marker (`board--2026-10-13T13-46-51`).
- ✅ The gate was red on a real failure four times before it was green (cycles 1–4, the
  transcripts' lines in the README).
- ✅ Docs sweep: RFC-0101 Phase 2, RFC-0106 Phase 2 per-class floor and Phase 3 GPU set, the
  order file's layer 3 row, CHANGELOG.
