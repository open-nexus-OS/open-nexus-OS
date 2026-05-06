---
title: TASK-0253 Input v1.0b (OS/QEMU): hidrawd + touchd + inputd + windowd/IME hooks + `nx input` + selftests
status: In Progress
owner: @ui
created: 2025-12-29
depends-on:
  - TASK-0252
follow-up-tasks:
  - Kernel/runtime service-scale closure for init-lite multi-service proofs
  - TASK-0056C
  - TASK-0146
  - TASK-0147
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Production gates: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Queue/quality context: tasks/IMPLEMENTATION-ORDER.md
  - RFC (contract seed): docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md
  - Live driver-layer RFC: docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md
  - Input core (host-first): tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md
  - Visible input baseline: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - Later IME consumer: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - Input routing baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for Input v1.0:

- `hidrawd` service (USB-HID userspace driver),
- `touchd` service (I²C touch stub),
- `inputd` service (zentrale Event-Pipeline),
- windowd/IME hooks.

This task is pulled directly after `TASK-0252` so `TASK-0056B` does not grow a
temporary inputd-light path. `TASK-0056`/`TASK-0056B` provide the windowd
routing and visible-affordance authority; this task delivers the **low-level
input device drivers** and **event pipeline** that feed windowd. IME integration
is a bounded hook/stub here; full IME keymaps/OSK behavior follows in
`TASK-0146`/`TASK-0147`.

Gate alignment:

- This task contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`).
- Latency/perf closure remains explicit follow-up scope in `TASK-0056C`; 0253 must
  provide deterministic, bounded, and measurable live-input behavior without
  claiming perf-budget closure.
- Scope is expanded narrowly to include kernel/runtime service-scale fixes required
  to boot `hidrawd`, `touchd`, and `inputd` as real init-lite service processes
  without kernel heap exhaustion. This is not permission for broad kernel redesign.
- The remaining live-QEMU closure gap now also includes a minimal `virtio-input`
  driver layer per `RFC-0054`; this task must not close via a permanent
  `selftest-client` bridge around the real driver owner.

## Goal

On OS/QEMU:

1. **OS/QEMU input source wiring**:
   - wire guest-visible HID and touch source configuration through the existing
     QEMU/OS service startup path (no ad-hoc side channel),
   - keep source selection deterministic and profile-gated in proof-manifest/harness.
2. **hidrawd service** (`source/services/hidrawd/`):
   - parse HID reports for keyboard and mouse (boot protocol subset) using library from `TASK-0252`
   - expose API (`hid.capnp`): `subscribe()` → `stream:List(HidEvent)`
   - markers: `hidrawd: ready`, `hidrawd: device kbd`, `hidrawd: device mouse`, bounded event logs
3. **touchd service** (`source/services/touchd/`):
   - scan DT for touch node, emit normalized events using library from `TASK-0252`
   - for QEMU, generate deterministic synthetic touches (fixture) behind a flag to exercise the path
   - API (`touch.capnp`): `subscribe()` → `stream:List(TouchEvent)`
   - markers: `touchd: ready`, `touchd: synthetic mode` (if enabled)
4. **inputd service** (`source/services/inputd/`):
   - merge sources (`hidrawd`, `touchd`) → `InputEvent` (key, pointer, touch)
   - focus & dispatch: target `windowd` (cursor move, click), `systemui` (global shortcuts), `imed` hook stubs (text)
   - key repeat (configurable via `settingsd`: `keyboard.repeat.delay_ms`, `keyboard.repeat.rate_hz`)
   - keymaps (US/DE/JP/KR/ZH base): table-driven mapping; IME switch key (e.g., `Ctrl+Space`)
   - pointer acceleration (simple linear curve; deterministic)
   - API (`input.capnp`): `subscribe()` → `stream:List(InputEvent)`, `setKeymap(name)`, `getKeymap()` → `name`
   - markers: `inputd: ready`, `inputd: keymap=de`, `inputd: repeat start code=…`, `inputd: dispatch windowd cursor=(x,y)`
5. **SystemUI & IME hook stubs**:
   - `windowd`: consume pointer/touch for cursor and focus; small hover highlight to verify
   - IME overlay hook: when `inputd` detects text focus, send `imed.show()`; on blur, `imed.hide()` (stub contract only; full IME behavior is `TASK-0146`/`TASK-0147`)
   - markers: `systemui: imed show`, `systemui: imed hide`
6. **Settings integration**:
   - seed keys: `keyboard.layout` (`"us"|"de"|"jp"|"ko"|"zh"`), `keyboard.repeat.delay_ms`, `keyboard.repeat.rate_hz`, `pointer.accel`
   - provider side-effects: update `inputd`
7. **CLI diagnostics** (`nx input ...` as a subcommand of the canonical `nx` tool):
   - `nx input devices`, `nx input keymap set de`, `nx input keymap get`, `nx input test type "Hello, 世界!"`, `nx input cursor 640 400`
   - markers: `nx: input keymap=de`, `nx: input cursor set (640,400)`
8. **OS selftests + postflight**.
9. **Kernel/runtime service-scale closure**:
   - fix the kernel/runtime blocker that currently appears when the three input
     services are added to the normal QEMU service set: additional service address
     spaces/page tables exhaust the 2 MiB kernel heap before later exec proofs finish,
   - add resource-category diagnostics so future failures identify page-table,
     address-space, stack, cap-table, IPC, or VMO pressure instead of forcing log guessing,
   - prove the normal service set plus input services can continue through the
     required visible-bootstrap proof without script-only profile exceptions.
10. **Minimal `virtio-input` driver layer (RFC-0054)**:
    - establish a bounded userspace `virtio-input` MMIO polling layer for QEMU
      `virt` input devices (device ID `18`),
    - make the live driver owner explicit (`device.mmio.input` must not remain a
      long-term `selftest-client` ownership path),
    - translate keyboard/pointer events into the existing
      `hidrawd -> inputd -> windowd` authority chain without introducing a second
      routing authority,
    - keep the runtime model cooperative and bounded (`yield_()`-based, no
      unbounded busy loop, no fake ready markers).
11. **Visible live-input scene proof + host-driven live lane**:
    the final visible-bootstrap proof and interactive OS-start lane must render a
    full colored window, one visible pixel that follows routed pointer motion, a
    bottom-left square that changes color on hover and click, and a right-side
    square that changes color on keyboard input. UI logs/errors must expose
    enough structured state to debug failures without guessing.

## Non-Goals

- Broad kernel redesign beyond the narrow service-scale work required for this proof.
- Full IME engine (handled by `TASK-0146`/`TASK-0147`).
- Real hardware (QEMU HID/touch only).
- A permanent `selftest-client` ownership bridge for live input devices.
- Latency/coalescing/no-damage/present fast-path closure; that is `TASK-0056C`
  and must not be claimed by this task.

## Constraints / invariants (hard requirements)

- **No duplicate input authority**: `inputd` is the single authority for input event routing. Do not create parallel input services.
- **No duplicate keymap authority**: `inputd` uses the keymaps library from `TASK-0252`. `TASK-0146` (IME) must share/extend the same keymap tables to avoid drift.
- **Determinism**: HID parsing, touch normalization, keymaps, repeat, and acceleration must be stable given the same inputs.
- **Bounded resources**: keymaps are table-bounded; repeat timing is bounded.
- **Device access**: assumes `TASK-0010` (device MMIO access model) is Done; real HID/I²C touch paths may additionally
  require device-class caps (USB/I²C controller access) beyond the v1 MMIO primitive.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security / authority invariants

- **Fail-closed device/input ingestion**:
  malformed HID/touch frames, invalid routing targets, or stale subscriptions must reject with stable classes.
- **Single routing authority**:
  `inputd` normalizes/routes raw input; `windowd` remains hit-test/hover/focus/click authority.
- **No ambient capability creep**:
  device access for HID/touch is capability-gated and deny-by-default; no broad service gets unconditional MMIO-style access.
- **Bounded queues and logs**:
  event queues/retry loops are bounded; logs/markers use bounded metadata (no raw payload dumps).
- **No marker-only closure**:
  success markers are emitted only after state transitions verified by selftests/harness checks.

## Red flags / decision points

- **RED (input authority drift)**:
  - Do not create a parallel input service that conflicts with `inputd`. `inputd` is the single authority for input event routing.
- **RED (keymap authority drift)**:
  - Do not create parallel keymap tables. `inputd` and later `imed` (`TASK-0146`) should share the same keymap library to avoid drift.
- **YELLOW (input routing vs windowd)**:
  - `TASK-0056` plans input routing (hit-test/focus) in windowd. `inputd` provides low-level event pipeline. Document the relationship explicitly: `inputd` → `windowd` → surfaces.
- **YELLOW (perf claim drift)**:
  - 0253 must not claim latency/smoothness closure without explicit budgets/scenes (`TASK-0056C` scope).

Red-flag mitigation now:

- keep one input routing chain: `hidrawd|touchd -> inputd -> windowd`,
- reuse `TASK-0252` keymaps/repeat/accel crates (no service-local clones),
- require deterministic marker order + reject proofs for malformed/stale/unauthorized paths,
- publish bounded counters/diagnostic signals that 56C can consume for perf closure,
- keep non-claims explicit for perf-budget closure and full IME/OSK behavior.

## Contract sources (single source of truth)

- RFC contract seed: `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md`
- Live driver-layer contract seed: `docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md`
- QEMU marker contract: `scripts/qemu-test.sh`
- Input core: `TASK-0252`
- Later IME keymaps: `TASK-0146` (US/DE keymaps for IME)
- Input routing: `TASK-0056` (hit-test/focus in windowd)
- Device MMIO access: `TASK-0010` (prerequisite)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `hidrawd: ready`
- `hidrawd: device kbd`
- `hidrawd: device mouse`
- `touchd: ready`
- `inputd: ready`
- `inputd: keymap=de`
- `inputd: repeat start code=4`
- `inputd: dispatch windowd cursor=(36,28)`
- `systemui: imed show`
- `systemui: imed hide`
- `SELFTEST: input keymap de ok`
- `SELFTEST: input cursor ok`
- `SELFTEST: input touch ok`
- `SELFTEST: input repeat ok`

Additional closure floor:

- marker order is deterministic and profile-verified via the canonical harness,
- normal OS/QEMU service startup proves `hidrawd`, `touchd`, and `inputd` as real init-lite service processes without kernel heap OOM or script-only profile exceptions,
- kernel/runtime resource diagnostics identify the pressure category when service-scale proofs fail,
- visible-bootstrap proves real visible behavior, not only markers:
  - full colored window,
  - one pixel follows routed pointer motion in the proof scene,
  - bottom-left square changes color on hover and click,
  - right-side square changes color on keyboard input,
  - UI-side logs/errors expose the observed state transitions,
- the live-input closure path includes a real driver layer:
  - `device.mmio.input` ownership for live QEMU input moves to the driver owner
    service rather than staying on a long-term `selftest-client` bridge,
  - the `virtio-input` polling/event loop is bounded and cooperative,
  - real QEMU keyboard/pointer events traverse `hidrawd -> inputd -> windowd`
    before any live-lane closure claim,
- host-driven live OS start proves the same scene in a non-proof interactive
  lane:
  - `make run` reuses `make build` artifacts, starts QEMU live, and uses the
    `interactive-minimal` runtime mode with minimal breadcrumbs,
  - `just start` performs its own build, starts QEMU live through the same
    runner, and uses the `interactive-full` runtime mode with full breadcrumbs,
  - live breadcrumbs never masquerade as deterministic `SELFTEST:` proof
    markers,
- boot/resource gates prevent pre-input failures from being rediscovered by
  manual QEMU runs:
  - `neuron-boot.map` must retain a non-zero private selftest stack
    (`__selftest_stack_top - __selftest_stack_base == 0x8000`),
  - runtime mode/profile must be read via bounded `fw_cfg` retry so late
    capability transfer does not fall back to the wrong profile,
  - the VMO arena must leave enough headroom for the live ramfb framebuffer
    after normal service bring-up,
- stable bootstrap failure labels must exist for:
  - `fw-cfg-map`,
  - `fw-cfg-signature`,
  - `ramfb-file-missing`,
  - `framebuffer-vmo`,
  - `interactive-scene-evidence`,
  - future input route timeout classes (`input-route-timeout`,
    `keyboard-route-timeout`) when the live daemon path is wired beyond the
    current scene readiness gate,
- `cargo test -p input_v1_0_host -- --nocapture` stays green as the RFC-0052 carry-in authority baseline,
- required reject-path tests exist for malformed HID/touch and invalid keymap/repeat/accel/routing settings,
- proof-manifest / harness verification remains the OS acceptance authority; marker-only grep closure is forbidden,
- quality gates are green before `Done` claim:
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `scripts/fmt-clippy-deny.sh`
  - `just test-all`
  - `just ci-network`
  - `make clean` -> `make build` -> `make test` -> `make run`
- perf/non-perf honesty:
  - 0253 provides bounded/measurable live-input behavior,
  - latency-budget closure remains explicitly owned by `TASK-0056C`.

## Current implementation reality (2026-05-05)

- Commit `0503499` captured the kernel/runtime service-scale follow-up after `f24011b`.
- Focused proofs currently green:
  - `cargo test -p windowd`
  - `cargo test -p nx --test init_lite_input_service_startup`
  - `cargo test -p nexus-proof-manifest --test cli_verify_uart`
  - `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`
  - `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`
- The normal init-lite/QEMU service set now includes `hidrawd`, `touchd`, and `inputd` with bounded startup stacks; the focused startup proof is no longer hidden behind profile-only startup exceptions.
- The deterministic visible scene now emits pixel/state-backed markers for full-window color, cursor motion, hover, click, keyboard input, and `SELFTEST: ui visible input ok`.
- Interactive live-lane hardening is now proven in both runners:
  - `make run` is the minimal-marker interactive runner that reuses `make build`
    artifacts and now reaches `windowd: interactive scene ready` plus
    `inputd: live pointer route on` in a time-capped live run,
  - `just start` builds first, then starts the same live runner with full
    breadcrumbs and now reaches `windowd: interactive full markers on`,
    `windowd: input visible on`, `windowd: cursor move visible`,
    `windowd: hover visible`, `launcher: click visible ok`,
    `windowd: keyboard visible`, and `SELFTEST: ui visible input ok`,
  - focused contracts cover linker retention of the private selftest stack,
    bounded `fw_cfg` runtime config retry, interactive marker honesty, and VMO
    arena headroom for the ramfb framebuffer,
  - stable UI bootstrap failure labels now expose failures such as
    `bootstrap: failed framebuffer-vmo` instead of a generic failure.
- Live-QEMU input closure is now carried by the `RFC-0054` slice:
  - QEMU exposes `virtio-keyboard-device`, `virtio-mouse-device`, and
    `virtio-tablet-device`,
  - init discovers `VIRTIO_DEVICE_ID_INPUT` windows and hands ownership into the
    `hidrawd -> inputd -> windowd` chain instead of a permanent
    `selftest-client` bridge.
- Remaining closure before `Done`:
  - broad-gate reruns are no longer deferred:
    - `just dep-gate`, `just diag-os`, `just diag-host`, `just ci-network`,
      `make clean -> make build -> make test`, and a time-capped `make run` are
      green,
    - `scripts/fmt-clippy-deny.sh` and therefore `just test-all` currently stop
      on repo-wide rustfmt drift outside the TASK-0253 slice (for example
      `source/kernel/neuron/src/mm/address_space.rs`,
      `source/init/nexus-init/src/os_payload.rs`,
      `tools/nx/src/commands/input.rs`, and
      `userspace/keymaps/src/layout.rs`),
  - `nx input` commands remain host/preflight helper surfaces only.

## Touched paths (allowlist)

- `source/services/hidrawd/` (new)
- `source/services/touchd/` (new)
- `source/services/inputd/` (new)
- `source/drivers/input/virtio-input/` (new)
- `source/services/windowd/` (extend: input integration, cursor, focus)
- `source/services/windowd/idl/input.capnp` (extend only if routing/event contract changes)
- `source/services/ime/` (extend: overlay hooks/stubs only)
- `source/services/systemui/` (extend: IME show/hide hook markers only)
- `source/services/settingsd/` (extend: keyboard/pointer provider keys)
- `tools/nx/` (extend: `nx input ...` subcommands; no separate `nx-input` binary)
- `source/apps/selftest-client/` (markers + scene observer only; not final driver authority)
- `source/init/nexus-init/` (extend: input MMIO capability ownership / distribution)
- `source/apps/selftest-client/proof-manifest/` (marker/profile updates)
- `docs/dev/ui/input/input.md` (extend with OS/QEMU live-input scope/proof notes)
- `docs/devx/nx-cli.md` (extend `nx input` diagnostics)
- `tools/postflight-input-v1_0.sh` (new)

## Plan (small PRs)

1. **DTB updates + hidrawd + touchd**
   - DTB: I²C touch + USB nodes
   - hidrawd service
   - touchd service
   - markers

2. **inputd service**
   - event merge & dispatch
   - keymaps + repeat + accel
   - focus & routing
   - markers

3. **virtio-input driver layer + hidrawd backend**
   - minimal `virtio-input` driver crate
   - cooperative polling loop
   - capability ownership via init
   - `hidrawd` live backend markers / reject tests

4. **windowd/IME hook stubs + settings + CLI**
   - windowd input integration
   - IME overlay hook stubs
   - settings provider
   - `nx input` CLI
   - markers

5. **OS selftests + postflight**
   - OS selftests
   - postflight

## Acceptance criteria (behavioral)

- `hidrawd` and `touchd` probe devices and emit events correctly.
- `inputd` merges sources, applies keymaps/repeat/accel, and dispatches to windowd correctly while exposing bounded IME hook stubs.
- the live QEMU lane is backed by a real `virtio-input` driver owner and bounded
  cooperative event loop, not a permanent `selftest-client` bridge.
- Windowd cursor and IME overlay hook stubs work correctly.
- All four OS selftest markers are emitted.
- Gate E quality/perf alignment is explicit and honest:
  - deterministic live-input behavior is proven with real markers + assertions,
  - no latency-budget closure is claimed here (delegated to `TASK-0056C`).
