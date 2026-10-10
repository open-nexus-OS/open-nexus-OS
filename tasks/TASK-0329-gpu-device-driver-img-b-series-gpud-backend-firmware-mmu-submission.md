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
