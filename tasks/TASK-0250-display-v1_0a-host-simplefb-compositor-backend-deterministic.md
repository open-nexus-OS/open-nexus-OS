---
title: TASK-0250 Display v1.0a (host-first): the display-controller scanout contract — a `GfxBackend` implementation for a planes-and-timings controller, EDID mode selection, host-tested
status: Draft (recut 2026-09-22 to the end state — Block 1 B1.7 host half of the hardware fast track; was "simplefb compositor backend + premultiplied alpha + dirty rects", Draft since 2025-12-29)
owner: @ui @runtime
created: 2025-12-29
updated: 2026-09-22
depends-on:
  - tasks/TASK-0244-bringup-rv-virt-v1_0a-host-dtb-sbi-shim-deterministic.md
follow-up-tasks:
  - tasks/TASK-0251-display-v1_0b-os-fbdevd-windowd-integration-cursor-selftests.md
links:
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C7, Phase 5)
  - The seam: userspace/nexus-gfx/src/backend/traits.rs (`GfxBackend`), backend/cpu_mock.rs (template); gpud backends source/drivers/gpud/src/backend/ (`attach.rs`, `present.rs`, `scanout_policy.rs`, `display_mode.rs`)
  - Boundary that stays: docs/rfcs/RFC-0067-* (windowd = single present authority), docs/rfcs/RFC-0093-* (handoff), docs/adr/0062-*
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Display"), `edid-hdmi.bin`
  - Playbook: CLAUDE.md
---

## History

Seeded 2025-12-29 as a "simplefb" compositor backend (a bootloader-provided framebuffer).
Recut 2026-09-22: there is no bootloader framebuffer in our chain (ADR-0066 — U-Boot's splash
`framebuffer@7f000000` belongs to the vendor chain), so the first picture IS the display
controller driver; this ledger is its host-testable half.

## Context (measured 2026-09-22)

The SoC display path: `display-subsystem-hdmi` (`spacemit,saturn-hdmi`, `0xc044_0000`, 0x2a000)
with the pipeline `spacemit,dpu-online2` (IRQs ONLINE 139 / OFFLINE 138), encoder
`spacemit,hdmi` (`0xc040_0500`, 0x200, IRQ 136, clock `hmclk`, reset `hdmi_reset`, power
domain 7), `dpu_reserved` 768 KiB. The desk monitor reports a preferred 2560×1440@59.95 EDID;
the vendor driver selects 1920×1080@60 (the SoC's HDMI ceiling), 8 bpc. gpud today has two
backends behind `GfxBackend` (`mmio` scanout, `virgl`), both virtio; the display mode comes
from fw_cfg (RFC-0074).

## Goal

The scanout contract for a real display controller, as pure host-tested code:
- `nexus-gfx` backend `dc` (`userspace/nexus-gfx/src/backend/dc/`): a `GfxBackend` whose
  `set_scanout`/`transfer_to_host`/`move_cursor` map onto a planes-and-timings controller model
  — primary plane (BGRA8888, stride, contiguous-DMA framebuffer), cursor plane, a mode
  (timings from EDID), damage → plane flush, vblank/ONLINE interrupt as the present fence —
  with a register-writer trait the OS half fills (TASK-0251) and a host mock that records the
  register sequence (goldens).
- `edid.rs` (`userspace/nexus-gfx` or a small `nexus-edid` crate): bounded EDID 1.4 parser —
  detailed timings, established/standard timings, extension blocks (CEA-861 short video
  descriptors), `pick_mode(caps)` = highest mode within the controller's limits; goldens
  include the desk monitor's `edid-hdmi.bin` (→ 1920×1080@60 given the 1080p60 ceiling) and
  QEMU's virtio display-info path expressed through the same `DisplayMode` type.
- `scanout_policy.rs` in gpud gets its third arm: `Dc` next to `Gl` and `VirtioNonGl`.

## Non-Goals

The register-level driver (TASK-0251), MIPI-DSI, HDMI audio, HDCP, multiple outputs, hotplug
beyond a boot-time detect, any composition on the controller beyond two planes.

## Packages

- **P0** — this recut; the controller model derived from the mainline driver documentation. **Measured 2026-09-29 (D0):** `docs/board/measurements/2026-09-29-display-regs/` — the live block map at 1920x1080@60 (OUTCTRL2 at 0x18000 holds the timing, CMPS2 at 0x4c00, RDMA at 0xa80+i·0x100, DPU_CTL at 0x500), the first-light sequence (about thirty direct writes, no command list, no display MMU, a contiguous buffer by bus address), the HDMI encoder's PLL/PHY/DDC words, the EDID; the pipeline runs on `hmclk` alone.
  (which registers make a plane, a mode, a flush); EDID goldens captured.
- **P1 EDID** — parser + `pick_mode`, `test_reject_*` (bad checksum, truncated, extension
  overflow).
- **P2 `dc` backend model** — plane/mode/flush over the register-writer trait, host goldens of
  the register sequence for a 1080p60 bring-up and a damage flush.
- **P3 policy arm + windowd contract check** — `scanout_policy` third arm; the RFC-0093
  handshake unchanged (windowd never learns the backend).

## Constraints / invariants

- windowd stays the single present authority; gpud composes into the scanout framebuffer as
  today (`attach_external_framebuffer` + `present_scanout_damage`); the `dc` backend only
  scans out and flushes.
- No `unwrap` on EDID bytes (untrusted-shape input from the monitor).
- **Scanout memory is made for the controller (TASK-0246 P1, RFC-0098 C4).** The DPU sits on
  `multimedia-bus`, which translates the upper bank (bus `0x8000_0000` → CPU `0x1_0000_0000`):
  `vmo_runs` answers only in the DPU's bus addresses, and a contiguous framebuffer can only be
  made with the DPU's device capability (the kernel refuses a contiguous object without a
  device). windowd holds no device capability today, and its framebuffer is anonymous — P0
  decides between gpud (the DPU's holder) making the scanout buffer for windowd, and the plane
  reading windowd's anonymous framebuffer as a list, if the controller's DMA takes one.

## Definition of Done

Host tests + goldens green; the EDID picks 1080p60 for the desk monitor and QEMU's mode for
virt; the policy arm compiled and gated; RFC-0098 Phase 5 host half ✅.
