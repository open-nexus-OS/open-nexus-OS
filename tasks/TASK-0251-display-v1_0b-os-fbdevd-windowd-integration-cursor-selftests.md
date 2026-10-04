---
title: TASK-0251 Display v1.0b (OS/board): the display controller + HDMI driver in gpud, the display mode's one authority is gpud (EDID on the board, virtio display-info on QEMU), syscall 50 and the fw_cfg path deleted — the first picture
status: In Progress (P2a step 3a ✅ 2026-10-04 on the board — the picture at 1080p: an anti-aliased painter, line icons as strokes, the damage grid at the layout, a Lanczos wallpaper bake, the `visible-2d` lane; open findings noted for their topics; operator priority: USB input (Block 2) before step 3b and P2b EDID; P2a step 2 ✅ 2026-10-04 windowd's desktop through the controller; P2a step 1 ✅ 2026-10-03 first light; P1 done 2026-09-30 — the mode authority is gpud; recut 2026-09-22 to the end state — Block 1 B1.7 of the hardware fast track; was "fbdevd service + windowd simplefb integration + cursor + splash", Draft since 2025-12-29)
owner: @ui @runtime
created: 2025-12-29
updated: 2026-10-04
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

- **P0** — this recut. **Measured 2026-09-29 (D0):** see `docs/board/measurements/2026-09-29-display-regs/README.md` — the pipeline needs `hmclk` + `hdmi_reset` + power domain 7 only; the scanout buffer may live in either bank (bank 0 is identical on the controller's bus, bank 1 at bus 0x8000_0000 — corrected 2026-10-03; D0 first read "bus = CPU − 0x8000_0000"); the controller is not cache-coherent; ONLINE IRQ 139.
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
  emulate this controller → host goldens (TASK-0250) + the board. Power/clock/reset is socd's
  (TASK-0245B P3, built 2026-09-30): gpud asks for the controller's and the encoder's node;
  socd powers domain 7, releases `hdmi_reset`, gates `hmclk` on at 491.52 MHz and prints every
  register it touched before and after — proven on the board 2026-09-30 (D3's own cycle, the
  board ladder's `SELFTEST: soc glue display ok` rung; `docs/board/measurements/2026-09-30-
  power-domains/` H1–H4 decided), so P2 starts from a powered, clocked, released pipeline and
  its first read is the controller's version word (`0x03001030` at offset 0). P2b needs the
  encoder's DDC pads (`hdmi_0_grp`, measured) and their pad-number mapping in the table.
  - **P2a step 1 — first light (gpud's boot splash through the controller): ✅ 2026-10-03 on the
    board** (two cycles, `docs/board/measurements/2026-10-03-first-light/`: the first scanned
    nothing — the composer's layer word named RDMA3 while the plane was fed through RDMA1 — and
    the encoder saw no hot-plug — its pads unmuxed; the second passed every gate: pads =
    the live words, `hpd=1`, the raw vsync, `[PASS] board-headless` with 20 rungs, the splash on
    the monitor acknowledged as `board-visual: splash`). The MMIO seam first: `nexus_abi::MmioWindow` (the one mapped,
    bounds-checked register window; a block inside its page is a `window` at the tree's
    offset) + `nexus_driverkit::{Mmio, MmioSet}` (`nexus_hal::Bus` over it) — the six
    per-driver volatile copies (sdhci, virtio-blk, socd, virtio-input, nexus-net-os, and the
    harness's probe bus) deleted, `MmioBus` a retired name; sdhci and socd now forbid
    `unsafe`. init: `device_tree::display_plane()` by compatible, `core_plane::
    grant_display_plane` → `device.mmio.display` (policy: gpud) into `slots::gpud::
    DISPLAY_CONTROLLER/ENCODER`, `init: display plane ok|none (…)`; gpud's declared legs (its
    reply inbox + `slots::gpud::SOCD`, route `Gpud → Socd`). gpud `backend/dc/`: `glue` (socd
    for both nodes, found by compatible in gpud's tree), `controller` (version check, the model's
    `bring_up` over the window, the output line counter as the scan proof), `encoder` (HPD, the
    measured sequence — `nexus_gfx::backend::dc::encoder`, goldens against the D0 dump — and a
    bounded PLL-lock wait), `framebuffer` (`DmaVmo::contiguous` for the controller, the whole
    layout, Zicbom clean of the display plane), `mod` (mode: `display_mode::resolve` + the
    standard CEA timing — `cea_mode` — until P2b; the splash via `compose_splash_region`;
    `gpud: dc scanout ok (WxH@60 cea bus=… lines a->b)`; windowd's framebuffer request refused
    by name). The D0 reach was misread (bus = CPU − 0x8000_0000); corrected 2026-10-03 — bank 0
    is identical on the controller's bus, bank 1 at bus 0x8000_0000, and the kernel names the
    address. Gate: `gpud: dc scanout ok (` is a `board-headless` rung (red on the 2026-09-30
    trace); `board-visual: splash` the operator's ack. H5 (the boot loader's minimal encoder
    sequence gives a picture on this monitor) and H6 (the controller from reset — no display MMU,
    no command list — scans a bus address in bank 0) confirmed by cycle 2. Between the cycles:
    the encoder's pads through socd (TASK-0245B P3 pad half, the live words as the gate), the
    scan proven by either witness (the post-processing line counter does not count without
    post-processing — the raw vsync does), every written controller word read back (it found
    the boot loader's background word outside its 12-bit field; the model no longer writes it),
    the vendor boot loader's console captured in every flash session (`oem log`: it sees the
    hot-plug and puts its framebuffer at CPU 0x7f700000 in bank 0), the status LED's pad
    corrected (GPIO 96 is register 0x1e0, not 0x180).
  - **P2a step 2 — windowd's desktop on the controller: ✅ 2026-10-04 on the board, first
    cycle** (`docs/board/measurements/2026-10-04-desktop-path/`: hypotheses and gates written
    before the cycle; `[PASS] board-visible: 24 rungs, 1 operator marker(s)`, `[PASS]
    board-headless`; the operator saw the greeter). gpud's request loop (`service.rs`) keeps
    the wire — decode, command validation, damage, the chain trace, the present statistics,
    the reveal latch, every answer — and drives ONE display behind `backend::display::Display`:
    the virtio GPU (`backend/virtio_display.rs`, the 2D and the virgl scanout unchanged) or the
    controller (`backend/dc/`); the controller's own loop and its named refusal are gone. ONE
    CPU executor (`backend/cpu_frame.rs`) for the 2D scanout, the virgl path's per-command
    fallbacks and the controller, with the software cursor's state (host-tested,
    `test_reject_*`). On the board: windowd's framebuffer is one contiguous block for the
    controller (the whole layout), made at bring-up right after the splash (one run of 99.5 MB is
    found most surely before the fleet takes memory) and granted at windowd's first request; a
    present runs through the executor and its damage is cleaned
    (`nexus_gfx::backend::dc::damage_spans`, `test_reject_a_damage_outside_the_plane`); the boot
    splash lives in a plane of its own and holds the glass until the first present after
    windowd's reveal (`splash_hold`, RFC-0093 §5 amended), which cleans the display plane and
    switches the controller to it (`nexus_gfx::backend::dc::flip`: address, stride, latch;
    `gpud: dc reveal flip ok (bus=0x8fd2000 stride=7680 readback 3/3 …)`); a switch that does
    not read back keeps the splash and acks no reveal (`test_reject_a_reveal_after_a_refused_switch`);
    the cursor is windowd's software sprite. Measured on the board: a full-screen present costs
    14.0 ms of CPU plus 0.8 ms of cache clean (`gpud: dc first present (…)`). Deleted: the
    never-armed save-under cursor, the unused overlay fallback, `present_committed`,
    `set_plane_address` (retired names); `service.rs` 1018 → 692 lines, `backend/present.rs`
    under the module-size limit. Proof: host tests (gpud, nexus-gfx), the 2D scanout at
    1920x1080 before and after (`smp1`, the executor the controller runs), `visible` (virgl,
    pixel statistics equal to every run since 2026-09-30), `just check`, `just test-all`
    EXIT=0 (60 PASS). **What the operator saw, the next step's input:** a long black before the
    splash (the kernel's LED milestones run to 23.4 s), a short flicker as the splash appears,
    the wallpaper not over the whole screen, chopped vector graphics, soft text. The 2D scanout
    captured at 1:1 (M3) shows the executor's picture: the wallpaper covers the screen and the
    text is crisp; the avatar circle's top is cut flat and the glass is flat blue — the CPU
    layer composite lacks the GPU path's rounded mask, content scaling, opacity and tinted blur.
    The monitor's own mode is 2560x1440 (EDID): a 1080p signal is scaled 4/3 inside it, which
    softens text (to be checked against the stock system at 1080p on the same monitor).
  - **P2a step 3 — the picture at 1080p, as sharp as the hardware allows** (in progress
    2026-10-04). Measured first (the operator's notes after step 2, then the 1:1 capture of the
    2D scanout at 1920x1080 — the executor the board runs — against the GL path's, and the code):
    - *The wallpaper stops at row 832 on the CPU path* (the operator: "full width, not full
      height"; the capture: rows ≥ 840 one flat colour). windowd's damage tile grid is still the
      1280x800 one (`TILES_X = 20`, `TILES_Y = 13` — an M-L leftover), so rows past 832 never
      count as dirty and the base pass never paints them; the GL path draws Plane 0 directly.
    - *The icons look chopped* (the operator: mainly the three bottom-right buttons). The icon
      import cuts every stroke of the line-icon set into separate quads (no joins, no caps) and
      the vector fill samples one point per pixel, even-odd — at 17 px the 1.4 px stroke turns
      into stair steps with gaps, on every path (the content is the app's, CPU-rendered).
    - *A straight bar above every round button*: the `inset 0 1px 0` highlight is a straight
      1 px line inset by 30 % of the radius — on a circle it overhangs the shape.
    - *The wallpaper source is 1536x1024* (3:2): the 16:9 crop is 1536x864, upscaled 1.25× by
      a box filter that degenerates to nearest-neighbour when it scales up.
    - *Soft text* is the monitor's: it scales 1080p to its 2560x1440 panel; the operator
      confirmed the stock system's text equally soft (1080p is the SoC's maximum).
    **3a** — the tile grid derived from `nexus_display_proto::layout::LAYOUT_MAX` (a test proves
    it covers the layout); the line icons imported as polylines with their stroke width and
    painted as anti-aliased capsules (round caps and joins — the icon set's own stroke model)
    over the union of all strokes (`ShapeKind::Stroke`); the inset highlight as the top-facing
    edge band of the element's own shape (an arc on a circle); the wallpaper bake resampling
    with a reconstruction filter (separable Lanczos-3, widened when it scales down) — and a
    native-resolution source asset when one exists; a `visible-2d` lane (the 2D scanout at
    1920x1080 with the VNC pixel proof) so the CPU path's picture is judged in every
    `test-all`. **3a ✅ 2026-10-04 on the board** (`[PASS] board-visible`, 24 rungs; the operator:
    "icons and circles look good now"; `docs/board/measurements/2026-10-04-picture-1080p/`).
    **Open findings, noted for their topics** (the operator's rule: record them, do not debug
    legacy now — the priority is the picture with USB input; hard-coded sizes stay a problem):
    1. *The settled greeter covers the bottom of the wallpaper* — on the board ("still not full
       screen") and on QEMU's 2D scanout with the same windowd (bundle `c00ad8d6…`): at the
       reveal the wallpaper reaches row 1080 (grab +0.4 s, no flat row), the settled greeter
       (grab +6 s, with its clock) paints rows ≥ ~840 one flat dark colour — a 1280x800-era
       size in the desktop-surface path (the greeter page's own height, app-host's band
       geometry — `probe/scroll.rs` budgets "the desktop base's 800" rows —, or windowd's
       desktop-surface composite) — or, the operator's hypothesis, an element built wrongly for
       the larger mode: the shell's top bar or task bar, or the greeter's bottom row (the three
       round buttons) as an opaque full-width container, which at 1280x800 falls off the
       bottom edge or has no height and so never showed on QEMU. The pixel proof samples only
       at the reveal and misses it: a settled snapshot belongs into the proof.
    2. *Hard-coded 1280x800-era sizes* — sweep them before the layout changes again, with a
       gate: windowd's tile grid (fixed in 3a), app-host `probe/env.rs` ("landscape 1280×800"),
       `probe/scroll.rs` (the atlas budget from "the desktop base's 800"), `greeter.toml` (pixel
       values "at the canonical 1280x800 mode"), windowd `markers.rs` (`READY_MARKER`,
       `DISPLAY_MODE_MARKER`), `systemui_shell::DeviceProfile::qemu_default`; the display-ssot
       gate's literal rule covers windowd and inputd only — extend it to app-host, systemui and
       the manifests.
    3. *Rows switching top to bottom* at the splash's first frame and at the reveal (the
       operator: "like a scanline") — tearing: the controller scans while the CPU writes the
       plane (no second plane) and the switch latches mid-frame (no flip at vertical blank).
       The tear-free step with the compositor/GPU work: the ONLINE interrupt as vsync, a second
       plane, flips at blank.
    4. *The long black before the splash* — the kernel's LED milestone pulses alone run to
       23.4 s (the no-UART desk's wait signal), and nothing can show before gpud (the display
       driver is a userspace service; an on-screen boot console before it would need the loader
       to drive the display). Quick lever when wanted: short LED blinks now that the eMMC trace
       is the observation channel.
    5. *The entry animation's first second* on the CPU path (a doubled password pill, a cut
       avatar) — the transform overrides (3b).
    **3b** — the CPU layer composite at the GL path's semantics (the rounded mask,
    content scaling, opacity, the glass's blur and tint — the avatar circle's flat top, the flat
    blue glass), host goldens against the GL compositor's math; on the board a display-plane
    probe (flush, then sample a grid — what the controller reads); the scroll/transform
    overrides on the CPU path (today they wait for windowd's next full present).
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
