# Current Handoff: TASK-0055B closed (RFC-0048 Done)

**Date**: 2026-04-29  
**Active task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `Queued`  
**Active contract**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Done`  
**Carry-in baseline**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` / `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Closure summary

- Added an opt-in QEMU graphics path selected by `NEXUS_DISPLAY_BOOTSTRAP=1`: headless QEMU remains the default, while the `visible-bootstrap` proof profile uses `-display gtk` + `-device ramfb`.
- Kept profile semantics separate: `visible-bootstrap` is a proof-manifest harness/marker profile, not a future SystemUI/launcher start profile like desktop/TV/mobile/car.
- Added policy-gated `device.mmio.fwcfg` distribution from `nexus-init` to `selftest-client` for QEMU `fw_cfg` access.
- Added the fixed visible mode in `windowd`: `1280x800`, `5120` byte stride, ARGB8888/BGRA bytes, deterministic bootstrap pattern, present evidence, and pre-scanout marker gating.
- `selftest-client` writes the `windowd` composed frame into the framebuffer VMO and configures QEMU `etc/ramfb` through `fw_cfg` DMA only after the visible-bootstrap present evidence exists.
- Added host reject coverage for invalid mode/stride/format, invalid display capability handoff, and pre-scanout marker attempts.

## Proof

- Closure-hardening proofs are green, including the full required sequence:
  - `scripts/fmt-clippy-deny.sh`
  - `just test-all`
  - `just ci-network`
  - `make clean`, `make build`, `make test`, `make run`
  - plus `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`

Observed marker ladder on closure run:

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `windowd: present ok (seq=1 dmg=1)`
- `display: first scanout ok`
- `SELFTEST: display bootstrap guest ok`

## Scope Guardrails

- `TASK-0055B` closes only a deterministic QEMU `ramfb` visible bootstrap pattern path.
- It does not close visible SystemUI/launcher profile selection, input routing, cursor, dirty-rect display service behavior, virtio-gpu/GPU work, perf budgets, or kernel/core production-grade display closure.
- `TASK-0251` remains the owner for fuller display OS integration / `fbdevd`; `TASK-0055C` is the next UI lane for visible SystemUI first frame.
