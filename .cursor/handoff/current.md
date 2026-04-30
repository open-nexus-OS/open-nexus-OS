# Current Handoff: TASK-0055C implementation (visible SystemUI first frame)

**Date**: 2026-04-30  
**Active task**: `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md` — `In Progress`  
**Active contract**: `docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md` — `In Progress`  
**Active contract carry-in**: `docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md` — `Done` (`TASK-0055B`)  
**Carry-in baseline**: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` / `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md` — `Done`  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Implementation updates applied

- `source/services/systemui/` is no longer a flat helper; it now has modular `profile`, `shell`, and `frame` seams.
- Added minimal TOML seeds for the `desktop` profile and `desktop` shell under `source/services/systemui/manifests/`.
- `windowd` visible-present evidence now uses the deterministic SystemUI first-frame source after composing it into the visible 1280x800 frame on host and exposing composed rows for OS/QEMU.
- `selftest-client` writes `windowd`-composed rows to QEMU `ramfb`, not a raw SystemUI source buffer or selftest-owned sidecar composition.
- `selftest-client`, proof-manifest, and `scripts/qemu-test.sh` are wired for the 55C marker ladder:
  - `windowd: backend=visible`
  - `windowd: present visible ok`
  - `systemui: first frame visible`
  - `SELFTEST: ui visible present ok`
- `TASK-0055C`, `RFC-0049`, and UI testing docs are synced to the current partial-closure state.

## Green evidence so far

- `cargo test -p systemui -- --nocapture`
- `cargo test -p windowd -p ui_windowd_host -- --nocapture`
- `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`
- `cargo test -p selftest-client -- --nocapture`
- `cargo test -p windowd -p ui_windowd_host -p systemui -- --nocapture`
- `cargo test -p ui_windowd_host reject -- --nocapture`
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- `scripts/fmt-clippy-deny.sh`

## Pending closure gates

Operator requested these only run later:

- `just test-all`
- `just ci-network`
- `make clean`
- `make build`
- `make test`
- `make run`

## Scope guardrails for 55C

- `TASK-0055C` proves visible `windowd` present + first visible SystemUI frame only.
- It does not claim input/cursor, dirty-rect perf closure, GPU/virtio-gpu, or kernel production-grade closure.
- Startup profile/dev-preset semantics stay separate from the `visible-bootstrap` harness marker profile.
