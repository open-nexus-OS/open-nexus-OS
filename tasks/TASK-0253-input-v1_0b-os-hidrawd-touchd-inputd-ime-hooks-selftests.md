---
title: TASK-0253 Input v1.0b (OS/QEMU): hidrawd + touchd + inputd + windowd/IME hooks + `nx input` + selftests
status: Draft
owner: @ui
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Input core (host-first): tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md
  - IME keymaps baseline: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
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

The prompt proposes these services. `TASK-0056` already plans input routing (hit-test/focus) in windowd, and `TASK-0146` plans IME keymaps. This task delivers the **low-level input device drivers** and **event pipeline** that feeds into windowd's input routing and IME's keymap processing.

## Goal

On OS/QEMU:

1. **DTB updates**:
   - add DT nodes for generic I²C touch controller (stub) and USB controller (for HID mouse/keyboard in QEMU if exposed)
   - rebuild DTB
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
   - focus & dispatch: target `windowd` (cursor move, click), `systemui` (global shortcuts), `imed` (text)
   - key repeat (configurable via `settingsd`: `keyboard.repeat.delay_ms`, `keyboard.repeat.rate_hz`)
   - keymaps (US/DE/JP/KR/ZH base): table-driven mapping; IME switch key (e.g., `Ctrl+Space`)
   - pointer acceleration (simple linear curve; deterministic)
   - API (`input.capnp`): `subscribe()` → `stream:List(InputEvent)`, `setKeymap(name)`, `getKeymap()` → `name`
   - markers: `inputd: ready`, `inputd: keymap=de`, `inputd: repeat start code=…`, `inputd: dispatch windowd cursor=(x,y)`
5. **SystemUI & IME hooks**:
   - `windowd`: consume pointer/touch for cursor and focus; small hover highlight to verify
   - IME overlay hook: when `inputd` detects text focus, send `imed.show()`; on blur, `imed.hide()` (stubs ok)
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
- **No duplicate keymap authority**: `inputd` uses the keymaps library from `TASK-0252`. `TASK-0146` (IME) should share the same keymap tables to avoid drift.
- **Determinism**: HID parsing, touch normalization, keymaps, repeat, and acceleration must be stable given the same inputs.
- **Bounded resources**: keymaps are table-bounded; repeat timing is bounded.
- **Device MMIO gating**: userspace HID/touch drivers may require `TASK-0010` (device MMIO access model) or equivalent.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (input authority drift)**:
  - Do not create a parallel input service that conflicts with `inputd`. `inputd` is the single authority for input event routing.
- **RED (keymap authority drift)**:
  - Do not create parallel keymap tables. `inputd` and `imed` (`TASK-0146`) should share the same keymap library to avoid drift.
- **YELLOW (input routing vs windowd)**:
  - `TASK-0056` plans input routing (hit-test/focus) in windowd. `inputd` provides low-level event pipeline. Document the relationship explicitly: `inputd` → `windowd` → surfaces.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Input core: `TASK-0252`
- IME keymaps: `TASK-0146` (US/DE keymaps for IME)
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

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: I²C touch stub + USB controller nodes)
- `source/services/hidrawd/` (new)
- `source/services/touchd/` (new)
- `source/services/inputd/` (new)
- `source/services/windowd/` (extend: input integration, cursor, focus)
- `source/services/imed/` (extend: overlay hooks; `ime` is a legacy placeholder name)
- SystemUI (IMED show/hide hooks)
- `source/services/settingsd/` (extend: keyboard/pointer provider keys)
- `tools/nx/` (extend: `nx input ...` subcommands; no separate `nx-input` binary)
- `source/apps/selftest-client/` (markers)
- `docs/input/overview.md` (new)
- `docs/tools/nx-input.md` (new)
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

3. **windowd/IME hooks + settings + CLI**
   - windowd input integration
   - IME overlay hooks
   - settings provider
   - `nx input` CLI
   - markers

4. **OS selftests + postflight**
   - OS selftests
   - postflight

## Acceptance criteria (behavioral)

- `hidrawd` and `touchd` probe devices and emit events correctly.
- `inputd` merges sources, applies keymaps/repeat/accel, and dispatches to windowd/IME correctly.
- Windowd cursor and IME overlay hooks work correctly.
- All four OS selftest markers are emitted.
