# RFC-0074: Display-mode authority — compositor owns the mode, sourced from fw_cfg

- Status: Superseded in its authority statement (2026-09-30) by RFC-0098 C7 — the display mode
  is gpud's (it reads the lane's request from its own tree slot, applies the one policy and
  grants the mode with the framebuffer it owns, RFC-0093 §5 v3); the kernel relay (syscall 50)
  is deleted. What stays from here: the request is race-free and wins over a device report;
  fw_cfg is the lane's transport of that request to nxboot. Originally: Draft
- Owners: @runtime / @kernel-team
- Created: 2026-07-20
- Last Updated: 2026-07-20
- Links:
  - Tasks: `tasks/` (execution + proof, TBD)
  - ADRs: `docs/adr/0050-display-mode-authority.md`
  - Related RFCs: `docs/rfcs/RFC-0067-windowd-compositor-boundary.md`

## Status at a Glance

- **Phase 0 (fw_cfg display-mode SSOT + kernel probe + syscall)**: 🟨 — since 2026-09-22 (TASK-0245 P3, RFC-0098 C2) the kernel reads the request from `/chosen/nexus,display-mode`, written by nxboot from fw_cfg on QEMU; the kernel has no fw_cfg code. Syscall 50 dies in RFC-0098 Phase 5 (gpud owns the mode).
- **Phase 1 (gpud commands the authoritative mode; device hint = capability only)**: 🟨
- **Phase 2 (event-driven mode-change: resize/hotplug/multi-output)**: ⬜ (future)

"Complete" = the contract is defined and the proof gates are green.

## Problem

The visible display mode was resolved by a **one-shot `GET_DISPLAY_INFO`
query at gpud probe**. Under QEMU's GTK backend that query races the
window-realize: QEMU's transient `ui_info` reports the un-realized window's
default size (~640×507) instead of the configured `xres=1280,yres=800`. gpud
then set its bootstrap scanout at that wrong size, which pins the GTK window
small (self-reinforcing), and the whole compositor sizes to it. Measured:
GTK boots latched 640×507 in ~4/5 runs, 1280×800 in ~1/5; `egl-headless`
(no window ⇒ no race) was correct every time.

Root cause: the display server **passively latched a transient host hint**
instead of owning the mode and driving the hardware to it.

## Scope boundaries (anti-drift)

- **This RFC owns**:
  - The **authority model**: the compositor (windowd) owns the display mode;
    gpud **commands** it onto the scanout. The host device's
    `GET_DISPLAY_INFO` reply is **validated capability data**, never authority.
  - The **configuration source**: the intended mode is explicit configuration
    passed host→guest via QEMU `fw_cfg` `opt/org.open-nexus/display-mode`
    (`"WIDTHxHEIGHT"`), the SAME channel as `selftest-mode`/`selftest-profile`.
  - The **delivery**: the kernel reads the fw_cfg key once (it already maps
    fw_cfg for boot-mode), and exposes it via a new syscall
    `SYSCALL_BOOT_DISPLAY_MODE (50)` — so services share the kernel's
    fw_cfg-derived mode WITHOUT mapping fw_cfg themselves (mirrors
    `SYSCALL_BOOT_MODE (45)`). No new per-service MMIO capability.
  - The **resolution policy** (pure, host-tested): prefer the configured
    fw_cfg mode; else the device capability; else the fixed layout maximum;
    always clamped to `[MIN, layout_max = 1280×800]`; reject degenerate
    (0 / oversized / garbage) values.

- **This RFC does NOT own**:
  - Event-driven mode changes (runtime resize / hotplug / multi-output) — a
    later `OP_DISPLAY_MODE_CHANGED` wire op (Phase 2).
  - The compositor's fixed 1280×800 resource layout (RFC-0067).
  - The hardcoded proof-marker strings (`display: mode 1280x800`) — separate
    cleanup.

## Contract

- fw_cfg key: `opt/org.open-nexus/display-mode` = ASCII `"<w>x<h>"` (e.g.
  `"1280x800"`). Absent ⇒ guest uses the layout maximum (proof/headless-safe).
- Syscall `SYSCALL_BOOT_DISPLAY_MODE (50)`: `ecall0` → packed `w | (h << 16)`
  as a `usize`; `0` when unknown/unset/host. Read-only, no args, no capability.
- gpud reports the **commanded** mode via `OP_GET_DISPLAY_MODE`.

## Invariant

The compositor only ever renders at a mode in `[MIN, 1280×800]`. A racy or
malicious device `GET_DISPLAY_INFO` reply can no longer shrink or drive the
scanout — it is validated capability data, not authority. Proven by the pure
resolver's `test_reject_degenerate_display_mode` host test.
