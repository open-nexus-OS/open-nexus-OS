# ADR-0050: Display-mode authority — compositor owns the mode, sourced from fw_cfg

## Status

Proposed (2026-07-20). Contract in `docs/rfcs/RFC-0074-display-mode-authority-fwcfg.md`.

## Context

The visible display mode was resolved from a one-shot virtio-gpu
`GET_DISPLAY_INFO` query at gpud probe. Under QEMU's GTK backend that races
the window-realize and frequently returns the un-realized window's transient
default (~640×507) instead of the configured `xres=1280,yres=800`. gpud then
pinned its scanout to that wrong size (self-reinforcing → whole compositor
shrinks). `egl-headless` (no window, no race) was always correct — proving the
defect is "the display server passively latches a transient host hint."

## Decision

The **display server owns the mode and drives the hardware to it**; the host
device report is validated capability data, never authority.

1. **Configuration source = fw_cfg** (`opt/org.open-nexus/display-mode`,
   `"WxH"`), the existing host→guest channel used for `selftest-mode`. The
   launcher emits it from `QEMU_GPU_XRES/YRES`.
2. **Kernel-derived delivery**: the kernel reads the key once (it already maps
   fw_cfg for `boot_mode`) and exposes it via `SYSCALL_BOOT_DISPLAY_MODE (50)`
   — mirroring `SYSCALL_BOOT_MODE (45)`. Services share the kernel's
   fw_cfg-derived mode without mapping fw_cfg or holding any new capability
   (strictly less surface than a per-service device-cap grant).
3. **gpud commands the resolved mode** onto the bootstrap scanout and reports
   it via `OP_GET_DISPLAY_MODE`. `GET_DISPLAY_INFO` is demoted to a validated
   capability/diagnostic read.
4. **Pure resolution policy** (`resolve_display_mode`, host-tested): prefer
   fw_cfg mode; else device capability; else the fixed layout maximum; clamp
   to `[MIN, 1280×800]`; reject degenerate values.

## Consequences

- Deterministic 1280×800 on `just start`; "follow xres/yres" preserved
  host-independently (config, not a racy host hint).
- No new per-service MMIO capability; one new read-only syscall (RFC-0074).
- Boundary crossing (gpud↔windowd mode authority) — hence this ADR.
- Runtime resize / hotplug / multi-output remain future work (RFC-0074 Phase 2,
  a later `OP_DISPLAY_MODE_CHANGED` wire op).

## Alternatives rejected

- **Pin gpud to 1280×800 unconditionally** — deterministic but drops
  follow-xres and hardcodes policy in the driver.
- **gpud maps fw_cfg itself** — adds a device-cap grant + duplicate reader;
  diverges from the "kernel is the single fw_cfg source" pattern.
- **QEMU `zoom-to-fit=on`** — tested; does NOT fix the race (still 640×507).
