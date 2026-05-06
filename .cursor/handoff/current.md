# Current Handoff: TASK-0253 live-input driver split checkpoint

**Date**: 2026-05-05  
**Completed task**: `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md` — `Done`  
**Completed contract seed**: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md` — `Done`  
**Active task**: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` — `In Progress`  
**Active contract seed**: `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md` — `In Progress`  
**Driver-layer follow-on RFC**: `docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md` — `In Progress`  
**Latest implementation commit**: `0503499` (`task-0253: unblock normal input service proofs`)  
**Gate policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, `production-floor`)  

## Carry-in baseline (must stay true)

- `TASK-0055`/`TASK-0055B`/`TASK-0055C`/`TASK-0056`/`TASK-0056B` are `Done` and remain the visible-input carry-in.
- `TASK-0252` host-first core crates/tests are `Done` and remain the only parser/keymap/repeat/accel authority.
- Live-input authority boundaries remain explicit:
  - `TASK-0252`: host-core parsing/keymaps/repeat/accel,
  - `TASK-0253`: OS/QEMU ingestion/services/routing into existing UI authority,
  - `inputd`: normalize/route low-level input,
  - `windowd`: hit-test/hover/focus/click authority,
  - IME full behavior remains follow-up scope (`TASK-0146`/`TASK-0147`).

## Landed in the latest slice

- Kernel/runtime service-scale blocker was addressed for the focused proof lane:
  - page-table/address-space/VMO pressure diagnostics landed,
  - per-service kernel mapping cost was reduced,
  - `exec_v2` cleanup destroys new address spaces on failure,
  - embedded-init boot triage now emits a root-level `neuron-boot.map`.
- `hidrawd`, `touchd`, and `inputd` are now in the default init-lite/QEMU service set with bounded startup stacks.
- Focused startup proof is green:
  - `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`
- Deterministic visible scene proof is green:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`
  - observed marker floor includes `windowd: full-window color visible`, `windowd: cursor move visible`, `windowd: hover visible`, `launcher: click visible ok`, `windowd: keyboard visible`, `SELFTEST: ui visible input ok`
- Supporting focused proofs are green:
  - `cargo test -p windowd`
  - `cargo test -p nx --test init_lite_input_service_startup`
  - `cargo test -p nexus-proof-manifest --test cli_verify_uart`
  - `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`
- UART verification is hardened against non-UTF8 noise.
- `selftest-client` now opts into a 512 KiB service heap for the heavy visible-bootstrap proof lane.

## Interactive closure hardening in progress

- `make run` is now the minimal-marker interactive runner that reuses `make build` artifacts.
- `just start` builds first, then launches the same live runner with full breadcrumbs.
- Focused contracts cover:
  - linker retention of the private selftest stack (`KEEP(*(.bss.selftest_stack_body))`),
  - bounded `fw_cfg` runtime mode/profile retry for late capability transfer,
  - marker honesty for interactive breadcrumbs,
  - VMO arena headroom for the live ramfb framebuffer.
- Stable UI bootstrap labels now include `bootstrap: failed framebuffer-vmo` and related `fw_cfg`/ramfb labels.
- The remaining live-QEMU blocker is now explicit architecture, not runner setup:
  - QEMU exposes `virtio-keyboard-device`, `virtio-mouse-device`, and `virtio-tablet-device`,
  - init can discover `VIRTIO_DEVICE_ID_INPUT` windows,
  - the missing piece is the bounded real driver-owner polling path now split into `RFC-0054`.

## Remaining closure blockers

- Broad closure gates remain deferred until explicitly requested:
  - `scripts/fmt-clippy-deny.sh`
  - `just test-all`
  - `just ci-network`
  - `make clean`, `make build`, `make test`, `make run`
- `nx input keymap set`, `nx input cursor`, and `nx input test type` are still host/preflight helper surfaces, not live daemon-affecting controls.
- The deterministic `visible-bootstrap` lane is green and remains the canonical Soll-proof, but final 0253 closure still needs the live QEMU session to show real mouse movement, hover/click rectangle reaction, and keyboard rectangle reaction.
- Final live closure must not route long-term through `selftest-client`; the driver-owner slice is now tracked under `RFC-0054`.

## Next focus

- Finish the live-lane evidence loop without weakening the deterministic proof:
  - implement the `RFC-0054` minimal `virtio-input` driver layer first,
  - run only focused checks unless the user explicitly requests broad gates,
  - use `just start` for full interactive breadcrumbs when a live smoke is requested,
  - keep `TASK-0056C` limited to latency/coalescing/no-damage/present fast path after functional live input exists.
