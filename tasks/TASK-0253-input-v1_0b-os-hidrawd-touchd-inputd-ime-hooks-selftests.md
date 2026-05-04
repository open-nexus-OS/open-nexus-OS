---
title: TASK-0253 Input v1.0b (OS/QEMU): hidrawd + touchd + inputd + windowd/IME hooks + `nx input` + selftests
status: In Progress
owner: @ui
created: 2025-12-29
depends-on:
  - TASK-0252
follow-up-tasks:
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

## Non-Goals

- Kernel changes.
- Full IME engine (handled by `TASK-0146`/`TASK-0147`).
- Real hardware (QEMU HID/touch only).

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
- `inputd: repeat start code=…`
- `inputd: dispatch windowd cursor=(x,y)`
- `systemui: imed show`
- `systemui: imed hide`
- `SELFTEST: input keymap de ok`
- `SELFTEST: input cursor ok`
- `SELFTEST: input touch ok`
- `SELFTEST: input repeat ok`

Additional closure floor:

- marker order is deterministic and profile-verified via the canonical harness,
- required reject-path tests exist for malformed HID/touch and invalid keymap/repeat/accel/routing settings,
- quality gates are green before `Done` claim:
  - `scripts/fmt-clippy-deny.sh`
  - `just test-all`
  - `just ci-network`
  - `make clean` -> `make build` -> `make test` -> `make run`
- perf/non-perf honesty:
  - 0253 provides bounded/measurable live-input behavior,
  - latency-budget closure remains explicitly owned by `TASK-0056C`.

## Touched paths (allowlist)

- `source/services/hidrawd/` (new)
- `source/services/touchd/` (new)
- `source/services/inputd/` (new)
- `source/services/windowd/` (extend: input integration, cursor, focus)
- `source/services/windowd/idl/input.capnp` (extend only if routing/event contract changes)
- `source/services/ime/` (extend: overlay hooks/stubs only)
- `source/services/systemui/` (extend: IME show/hide hook markers only)
- `source/services/settingsd/` (extend: keyboard/pointer provider keys)
- `tools/nx/` (extend: `nx input ...` subcommands; no separate `nx-input` binary)
- `source/apps/selftest-client/` (markers)
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

3. **windowd/IME hook stubs + settings + CLI**
   - windowd input integration
   - IME overlay hook stubs
   - settings provider
   - `nx input` CLI
   - markers

4. **OS selftests + postflight**
   - OS selftests
   - postflight

## Acceptance criteria (behavioral)

- `hidrawd` and `touchd` probe devices and emit events correctly.
- `inputd` merges sources, applies keymaps/repeat/accel, and dispatches to windowd correctly while exposing bounded IME hook stubs.
- Windowd cursor and IME overlay hook stubs work correctly.
- All four OS selftest markers are emitted.
- Gate E quality/perf alignment is explicit and honest:
  - deterministic live-input behavior is proven with real markers + assertions,
  - no latency-budget closure is claimed here (delegated to `TASK-0056C`).
