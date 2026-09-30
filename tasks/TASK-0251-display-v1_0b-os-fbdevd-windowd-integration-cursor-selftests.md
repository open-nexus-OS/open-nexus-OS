---
title: TASK-0251 Display v1.0b (OS/board): the display controller + HDMI driver in gpud, the display mode's one authority is gpud (EDID on the board, virtio display-info on QEMU), syscall 50 and the fw_cfg path deleted — the first picture
status: In Progress (P1 done 2026-09-30 — the mode authority is gpud on QEMU: request from gpud's tree slot, framebuffer owned and granted by gpud, windowd builds on the grant, inputd asks windowd once, syscall 50 deleted; P2/P3 follow on the board; recut 2026-09-22 to the end state — Block 1 B1.7 of the hardware fast track; was "fbdevd service + windowd simplefb integration + cursor + splash", Draft since 2025-12-29)
owner: @ui @runtime
created: 2025-12-29
updated: 2026-09-30
depends-on:
  - tasks/TASK-0250-display-v1_0a-host-simplefb-compositor-backend-deterministic.md
  - tasks/TASK-0245B-board-support-v1c-soc-clock-reset-pinmux-power-from-fdt.md
  - tasks/TASK-0286-kernel-memory-accounting-v1-rss-pressure-snapshots.md
  - tasks/TASK-0260B-nxboot-as-fit-payload-chosen-node.md
follow-up-tasks:
  - tasks/TASK-0327B-board-proof-lane-serial-ladder-profiles.md
links:
  - Contract: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md (C7, Phase 5); amends docs/rfcs/RFC-0074-display-mode-authority-fwcfg.md + docs/adr/0050-display-mode-authority.md
  - Host half: tasks/TASK-0250-display-v1_0a-host-simplefb-compositor-backend-deterministic.md
  - gpud: source/drivers/gpud/src/backend/ (`mod.rs` `VirtioGpuBackend`, `lifecycle.rs` IRQ bind, `attach.rs`, `present.rs`, `display_mode.rs`, `scanout_policy.rs`); windowd `display_backend.rs`
  - Measurement: docs/board/measurements/2026-09-22-stock-system/README.md ("Display")
  - Playbook: CLAUDE.md
---

## History

Seeded 2025-12-29 as `fbdevd` + windowd "simplefb" integration + splash. Recut 2026-09-22:
no fbdevd (gpud is the display owner, RFC-0067/0093), no bootloader framebuffer (ADR-0066);
the display-mode authority moves out of the kernel.

## Context (measured 2026-09-22)

Nodes and numbers: TASK-0250 Context. The stock driver brings the pipeline up as
`dpu_init` → `hdmi_setup` (id `0xa28501`, 8 bpc) → "DPU type 0 id 2 Start!", reads EDID over
the encoder's DDC twice, sets 1920×1080@60. On QEMU the mode comes from fw_cfg
(`opt/org.open-nexus/display-mode`, RFC-0074, syscall 50) because the virtio display-info was
racy at boot; TASK-0326 already moved gpud's GL decision to first need, and the
`ctrl_query_display_info` path exists.

## Goal

gpud backend `dc` on the board: power domain 7 + `hmclk` + `hdmi_reset` via `nexus-soc`,
EDID over DDC, mode pick (TASK-0250), plane programming into a contiguous-DMA framebuffer VMO
(the same `attach_external_framebuffer` windowd already hands over), damage flush, ONLINE IRQ
as the present completion, cursor plane. **gpud is the display-mode authority on both
platforms** (EDID / virtio display-info); windowd learns the mode through the existing
RFC-0093 handshake; syscall 50 (`BOOT_DISPLAY_MODE`), the fw_cfg key and RFC-0074's authority
are deleted (`/chosen/nexus,display-mode` stays a lane REQUEST that gpud may honour on QEMU).
Proof: QEMU visible lane unchanged (pixel proof, mode from display-info); board:
`gpud: dc scanout ok (1920x1080@60 edid)` + `windowd: desktop revealed` on the serial console
and the desktop on the monitor (`board-visual: desktop`, TASK-0327B).

## Non-Goals

GPU composition (target picture G), DSI panel, HDMI audio, hotplug after boot, mode switching
at runtime (one mode per boot; the runtime preset mechanism of TASK-0055D stays QEMU-side).

## End state (binding)

- `source/drivers/gpud/src/backend/dc/{mod,regs,plane,hdmi,ddc}.rs` — the register writer
  for TASK-0250's model, over `nexus_hal::Bus` with the `device.mmio.display` grant + IRQ from
  the FDT (`interrupts` 139/138/136 on the board).
- `display_mode.rs`: ONE `DisplayMode` source per backend (`dc` → EDID; virtio →
  `ctrl_query_display_info`); the kernel syscall and `nexus_abi::fwcfg` display key removed;
  `docs/rfcs/RFC-0074` marked amended by RFC-0098 C7; `scripts/qemu-launcher.sh` passes the
  lane's requested mode to nxboot (`/chosen`) instead of the kernel.
- windowd: no change in contract; `display_backend.rs` reads the mode from gpud's handshake as
  it does for virtio.
- Markers registered in the proof manifest: `gpud: dc scanout ok (` (board profiles),
  `board-visual: desktop` (operator-acked, TASK-0327B).

## Packages

- **P0** — this recut. **Measured 2026-09-29 (D0):** see `docs/board/measurements/2026-09-29-display-regs/README.md` — the pipeline needs `hmclk` + `hdmi_reset` + power domain 7 only; the scanout buffer may live in bank 0 (bus = CPU − 0x8000_0000); the controller is not cache-coherent; ONLINE IRQ 139.
- **P1 Mode authority = gpud** (QEMU) — **done 2026-09-30.** Built: gpud reads the lane's request
  from its own read-only tree slot (`NamedSlot::DeviceTree`, `slots::gpud::DEVICE_TREE`, pinned by
  init next to the harness's) and decides with `resolve_display_mode_sourced` (`gpud: display mode
  WxH (request|device|maximum)`); gpud owns the shared framebuffer (made at the first grant with
  `vmo_create_for` its device, kept) and grants mode + a clone in ONE answer
  (`OP_FRAMEBUFFER_REQUEST` → `FramebufferGrant`, RFC-0093 §5 v3; `gpud: framebuffer granted`);
  windowd asks once before its compositor exists, builds at the granted mode (`windowd: display
  mode from gpud (WxH)`) and attaches without a cap (an attach with a cap is refused); a stack
  without gpud is named (`windowd: display none (…)`); inputd asks windowd once
  (`OP_GET_DISPLAY_SPACE` over its new declared reply inbox, `slots::inputd::REPLY`; `inputd:
  display space from windowd (WxH)`). Deleted: syscall 50 (kernel), `nexus_abi` wrapper,
  windowd's `resolve_boot_display_mode` and its framebuffer allocation, the dead cap-moving
  legacy handoff; the fw_cfg key STAYS — it is the lane's transport of the request to nxboot
  (RFC-0098 C7 amended). Gates: `check-retired-names.sh` (the relay's names),
  `check-display-ssot.sh` rule 7 (windowd allocates no VMO), `qemu-test.sh` REQUIRES the four
  chain markers with the lane's mode in every display lane. Proof: host tests of the wire
  (`test_reject_grant_*`, `test_reject_refused_or_malformed_display_space`), `just check`, the
  `smp1`, `visible` (1280x800) and `visible-fhd` (1920x1080) lanes with the full chain and the
  pixel proof, `just test-all` EXIT=0 (60 PASS, 0 FAIL, 2026-09-30).
- **P2 `dc` driver** — power/clock/reset, DDC + EDID, plane + mode + flush, IRQ; QEMU cannot
  emulate this controller → host goldens (TASK-0250) + the board.
- **P3 First picture** — on the board through the boot chain (TASK-0260B): markers + the
  operator ack; photo in the ledger. **Block 1 gate.**

## Constraints / invariants

- The framebuffer is a contiguous-DMA VMO (TASK-0286) with the coherence hooks (RFC-0098 C4)
  around every CPU write before a flush.
- Deny-by-default grants: `device.mmio.display` to gpud only.
- No fake success: `dc scanout ok` prints the mode read from EDID and the flush count of the
  first present.

## Red flags / decision points

- **RED:** the controller's register semantics come from documentation and the mainline
  driver's structure, not from the vendor's code; the first bring-up is measured on the serial
  console with the stock system's `dmesg` sequence as the oracle (`dpu_init`, `hdmi_setup`).
- **YELLOW:** DDC/I2C for EDID may need the SoC's I2C controller (`i2c@d401d800` is the
  encoder's bus in the stock tree) — a small I2C master in `nexus-soc` or in the `dc` driver.
- **GREEN (measured):** the monitor negotiates 1080p60 with the stock driver — no mode
  gamble on the desk.

## Definition of Done

QEMU: all lanes green with gpud as the mode authority and the kernel free of display-mode
code; board: the two markers + the operator ack on the serial log of a `just board-test`
run, the desktop visible; docs (RFC-0074 amended, ADR-0050 superseded note, RFC-0098 Phase 5
✅, `docs/architecture/graphics/display-output-service-chain.md`, `README.md` "real hardware:
yes", CHANGELOG).
