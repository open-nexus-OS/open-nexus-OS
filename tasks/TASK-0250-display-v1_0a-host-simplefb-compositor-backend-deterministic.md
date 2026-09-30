---
title: TASK-0250 Display v1.0a (host-first): the display-controller scanout contract — a `GfxBackend` implementation for a planes-and-timings controller, EDID mode selection, host-tested
status: Done (2026-09-30 — P1/P2 done: `nexus_gfx::backend::dc`: bounded EDID/CEA parser + `pick_mode`, the measured register map, the bring-up/plane/flush model over `RegWriter`, goldens against the 2026-09-29 dump; P3's policy arm moves to TASK-0251 P2 where its consumer lands; recut 2026-09-22 to the end state — Block 1 B1.7 host half of the hardware fast track; was "simplefb compositor backend + premultiplied alpha + dirty rects", Draft since 2025-12-29)
owner: @ui @runtime
created: 2025-12-29
updated: 2026-09-30
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

- **P0** — this recut; the controller model derived from the mainline driver documentation. **Measured 2026-09-29 (D0):** `docs/board/measurements/2026-09-29-display-regs/` — the live block map at 1920x1080@60 (OUTCTRL2 at 0x18000 holds the timing, CMPS2 at 0x4c00, RDMA at 0xa80+i·0x100, DPU_CTL at 0x500), the first-light sequence (about thirty direct writes, no command list, no display MMU, a contiguous buffer by bus address), the HDMI encoder's PLL/PHY/DDC words, the EDID; the pipeline runs on `hmclk` alone. **M-L done 2026-09-30:** the resource layout has one home, `nexus_display_proto::layout` (`LAYOUT_MAX` 1920x1080, plane rows, offsets, atlas, stride), consumed by windowd, gpud and systemui; the `visible-fhd` QEMU lane proves the full-HD layout end to end (greeter + desktop surface at 1920x1080, pixel proof complete, present 22–25 µs).
  (which registers make a plane, a mode, a flush); EDID goldens captured.
- **P1 EDID** — **done 2026-09-30.** `userspace/nexus-gfx/src/backend/dc/edid.rs`: EDID 1.4 base
  block + up to two extensions, every block checksummed before a field is read, CEA-861 video
  data block (the progressive VICs with known timings) and detailed timings, bounded mode list
  (24), no `unwrap`; `pick_mode(modes, soc_max, screen_cm)` = the largest progressive mode inside
  the SoC's maximum with the monitor's aspect (±2 %), 60 Hz preferred, the monitor's own timing
  preferred over the table. Goldens: the reference monitor → preferred 2560x1440, 1080p60
  detailed h 88/44/148 v 2/5/38; `pick_mode(…, (1920, 1080))` = that timing, `(1280, 800)` = 720p
  (never a stretched 4:3). `test_reject_*`: bad checksum (base, extension), truncated, bad
  header, extension overflow, garbage descriptors, nothing fits.
- **P2 `dc` backend model** — **done 2026-09-30.** `dc/regs.rs` (the measured block map and the
  first-light word offsets) and `dc/model.rs` (`bring_up`/`flush`/`set_plane_address` over
  `RegWriter`; a bounded `Sequence` recorder): the 1080p60 bring-up reproduces the live dump's
  timing, RDMA and composer words exactly (`tests/dc_goldens.rs`: archived EDID → `pick_mode` →
  bring-up → the dump's words); a plane flip is four writes; every write lies inside the
  controller window (`test_reject_writes_outside_the_window`).
- **P3 policy arm** — **moved to TASK-0251 P2 (2026-09-30):** a `Dc` arm in gpud's
  `scanout_policy` has no constructor until the controller backend exists, and the warning gate
  (rightly) refuses an unconstructed variant; it lands with its consumer. The windowd contract
  check it named is TASK-0251 P1's grant (done).

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
  **Decided 2026-09-30:** gpud makes it (TASK-0251 P1, RFC-0098 C7 amended) — the controller
  takes base + stride, no list (D0), so the board's kind is the contiguous one, made with the
  controller's capability in TASK-0251 P2; D0 measured bank 0 in the controller's reach (bus =
  CPU − 0x8000_0000), so the buffer need not live in the upper bank.

## Definition of Done

Host tests + goldens green; the EDID picks 1080p60 for the desk monitor and QEMU's mode for
virt; the policy arm compiled and gated; RFC-0098 Phase 5 host half ✅.

**Met 2026-09-30:** host tests + goldens green (`tests/dc_goldens.rs`: the desk monitor's EDID →
1920x1080@60 → the dump's words); on virt the mode is the lane's request decided by gpud, not an
EDID (TASK-0251 P1 — virtio's display info is the device's capability, and the `visible` lane
proves 1280x800); RFC-0098 Phase 5 host half ✅. The policy arm moved to TASK-0251 P2 with its
consumer (see P3) — this ledger is closed without it.
