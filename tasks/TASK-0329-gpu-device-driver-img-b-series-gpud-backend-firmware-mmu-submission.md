---
title: TASK-0329 GPU device driver: IMG B-series backend in gpud — firmware, GPU MMU with explicit VM, submission ring, fences (the thin side of the split)
status: Draft (seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; rewritten to end state at its block's P0)
owner: @runtime @ui
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Extracted from: tasks/TRACK-DRIVERS-ACCELERATORS.md (CAND-DRV-010)
  - Split contract: docs/rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md, docs/adr/0032-gpu-command-ring-and-pipelined-present.md, docs/adr/0018-driverkit-abi-versioning-and-stability.md
  - Seam: userspace/nexus-gfx/src/backend/traits.rs (`GfxBackend`), source/drivers/gpud/src/backend/
  - Cross-process surfaces: docs/adr/0042-cross-process-surface-transport.md
  - RFC/ADR seeds (at P0): RFC-0101 GPU driver split, ADR-0069 offline compositor shaders
---

## Origin

Extracted from `tasks/TRACK-DRIVERS-ACCELERATORS.md` CAND-DRV-010 ("GPU service skeleton") for target picture G of the hardware fast track. The track is dissolved; its audio/camera/NPU candidates are parked in the order file.

## Context

gpud has two backends, both virtio (`mmio` scanout, `virgl` 3D). The board's GPU is an IMG B-series core (revision from the dts, measured at P0). The end state keeps the gpud / driver-kit / nexus-gfx split and follows the reverse-engineered-laptop-GPU driver architecture: a thin side that owns firmware, the GPU MMU with explicit VM management, submission and IRQ/fences, and a userspace driver that owns command encoding and (offline) shader compilation. Needs M1 (page-backed VMOs with a `contiguous-DMA` kind), S1 (IRQs off hart0) and `nexus-soc` (clocks/power).

## Goal

G0: the GPU truth measured and archived (compatible/revision, firmware blob + license under the provenance gate, register/IRQ map, power sequence, MMU page size). G1: the device online — firmware boots, `vm_bind`/`vm_unbind` of VMO pages into the GPU address space proven, a null job signals a kernel fence, on the board (`gpud: fw ok`, `gpud: mmu map/unmap ok`). G5 (`TASK-0329B`, seeded at P0): app surfaces (ADR-0042 VMOs) imported zero-copy.

Input from Block 1 (2026-10-09): the GPU's power/clock set (TASK-0245B P3) moves here — `nexus-soc`
already refuses the software-sequenced GPU domain until its consumer measures it (G0 measures the
power sequence; G1 brings the domain up through socd). With it, as the next new socd consumer, the
per-class floor (RFC-0106 Phase 2): `soc.glue.<class>` replaces the one `soc.glue` capability, so
blkd may bring up only storage nodes, gpud only display and GPU nodes, xhcid only USB nodes
(`test_reject_*` per class in socd's verdict).

Input from the planning of the layers above this driver (2026-10-10, the operator): an explicit
graphics/compute API (command buffers, pipeline objects, explicit sync) and an array/ML framework
(lazy graphs over memory the CPU and the GPU share) will sit on this driver. Neither is built here,
and both are libraries above G2/G3 — but the driver's base structure must not make them expensive
later. The P0 rewrite and the RFC-0101 seed take these as requirements:

1. **One context per client with its own GPU address space** — isolation by the GPU MMU, never by
   parsing command streams (RFC-0033 keeps validation out of the kernel). Today gpud runs one
   context and only windowd has a route.
2. **Queue kinds in the submit contract from the start** — render, compute, transfer; compute is
   declared (and refused by name) before G1 executes it.
3. **One sync primitive: the kernel's timeline fence** (RFC-0033 Phase 2) — the GPU's completion
   signals it, CPU code signals and waits on it, it travels as a capability; no second fence kind.
4. **Buffers are VMOs bound with `vm_bind`, each with a declared cache mode** — CPU-cached with
   Zicbom maintenance at sync points, uncached, or GPU-private (the K1's DMA masters are not
   cache-coherent, measured); ownership moves at fences (the `DmaBuffer` pattern, TASK-0284/0284B);
   scatter-gather through `vmo_runs`; bound GPU memory is charged to the client (M4).
5. **Priority, preemption, a timeout and a reset per context** — long compute never stalls
   compositing; windowd's context has the highest priority.
6. **One kernel crossing per batch, never per dispatch** — command buffers live in the client's
   buffers (the BKL makes every syscall expensive until S2–S4).
7. **GPU code is client data** — the driver accepts finished binaries (offline now; a JIT later is
   a library) and holds no built-in kernel list; capabilities (queue kinds, limits, memory types)
   are queryable.

G0 measures two more things for this: whether the GPU is cache-coherent with the CPUs, and which
preemption and priority levels the firmware offers.

## G0, the stock half — measured 2026-10-10 (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`)

- The node carries `dma-noncoherent`; core id `0x0024001D003400B6` (36.29.52.182, one core);
  614.4 MHz; power-on 155 µs + 226 µs as the stock driver timed itself; the running register
  values are archived as the expected values of our bring-up.
- Two firmware candidates: the published one (open host interface, unmodified redistribution
  allowed, no disassembly) and the stock one (another build, closed interface). **G1 starts
  with the published firmware**; its provenance and licence go under the firmware gate.
- The frame through the stock driver, our scene at 1920x1080: fixed cost 0.64 ms, opaque fill
  1.8 ns a pixel (opaque overdraw free), a blended full-screen layer 1.8–2.7 ms, a texture
  fetch about 1 ns; the glass scene 60 ms, 23 ms with one quarter-size blur. These are G1's,
  G3's and G4's bars (the order file's layer table).
- The open kernel driver does not list this core and the open userspace driver names it
  unsupported; what both say about firmware format, power-up, MMU, submission and interrupts
  is the reading list of G1's P0. The method is `.claude/skills/driver-bringup/`.
- Still G0's, in G1's first cycle: the register window with the domain off and through
  power-up, the MMU page size as configured, preemption between contexts under load.

## Non-Goals

The userspace driver (G2 = `TASK-0280` v2 API, G3 = 0169B/0170B/0171, G4 = 0215/0216), an on-device shader compiler, gaming-class features, any implicit sync.

## Packages (from the order file; the P0 rewrite fixes them)

- **G0 measure** — firmware availability + license + version pairing, register/IRQ map, power sequence, MMU page size (R6); `resources/firmware/gpu/` provenance.
- **G1 device online** — gpud backend `img`: firmware load, GPU MMU + explicit VM, `SubmitRing` submission, IRQ, fence bridge; power/clock/reset via `nexus-soc`. Gate: board markers above.
- **G5 app-surface import** (`TASK-0329B`) — after G4: ADR-0042 VMO → GPU map without a copy.

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite.
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
