---
title: TASK-0252 Input v1.0a (host-first): HID/touch event core + keymaps + key repeat + pointer acceleration + deterministic tests
status: Draft
owner: @ui
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IME keymaps baseline: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic input stack foundation:

- HID event parsing (USB-HID boot protocol),
- touch event normalization (I²C touch stub),
- keymaps (US/DE/JP/KR/ZH base),
- key repeat (configurable delay/rate),
- pointer acceleration (simple linear curve).

The prompt proposes HID/touch event core and keymaps. `TASK-0146` already plans IME keymaps (US/DE) for IME engine. This task delivers the **host-first core** (HID/touch parsing, keymaps, repeat, accel) that can be reused by both inputd (low-level routing) and imed (IME engine).

## Goal

Deliver on host:

1. **HID event parser library** (`userspace/libs/hid/`):
   - parse HID reports for keyboard and mouse (boot protocol subset)
   - event structure: `HidEvent { tsNs, kind, code, value }` where `kind` is "key", "rel", "btn"
   - deterministic parsing (no host-specific behavior)
2. **Touch event normalizer library** (`userspace/libs/touch/`):
   - normalize touch events from I²C touch controller
   - event structure: `TouchEvent { tsNs, x, y, type }` where `type` is "down", "move", "up"
   - deterministic normalization
3. **Keymaps library** (`userspace/libs/keymaps/`):
   - table-driven mapping for US/DE/JP/KR/ZH base
   - modifiers handling (including AltGr for DE)
   - IME switch key (e.g., `Ctrl+Space`)
   - deterministic mapping (no host locale leakage)
4. **Key repeat library** (`userspace/libs/key-repeat/`):
   - configurable delay and rate (defaults: delay_ms, rate_hz)
   - deterministic timing (injectable time source in tests)
5. **Pointer acceleration library** (`userspace/libs/pointer-accel/`):
   - simple linear curve (deterministic)
   - monotonic and bounded
6. **Host tests** proving:
   - keymap mapping EN/DE for umlauts and symbols
   - key repeat timing (delay/rate) with simulated time
   - pointer acceleration curve monotonic & bounded
   - touch synthetic sequence → press/move/up deliver correct order

## Non-Goals

- OS/QEMU integration (deferred to v1.0b).
- Full IME engine (handled by `TASK-0146`/`TASK-0147`).
- Real hardware (QEMU HID/touch only).

## Constraints / invariants (hard requirements)

- **No duplicate keymap authority**: This task provides keymaps library. `TASK-0146` uses keymaps for IME engine. Both should share the same keymap tables to avoid drift.
- **Determinism**: HID parsing, touch normalization, keymaps, repeat, and acceleration must be stable given the same inputs.
- **Bounded resources**: keymaps are table-bounded; repeat timing is bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (keymap authority drift)**:
  - Do not create parallel keymap tables that conflict with `TASK-0146` (IME keymaps). Share the same keymap library or explicitly document the relationship.
- **YELLOW (key repeat determinism)**:
  - Key repeat timing must use injectable time source in tests (not `std::time::SystemTime`).

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`
- IME keymaps baseline: `TASK-0146` (US/DE keymaps for IME)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p input_v1_0_host` green (new):

- keymap mapping EN/DE for umlauts and symbols
- key repeat timing (delay/rate) with simulated time
- pointer acceleration curve monotonic & bounded
- touch synthetic sequence → press/move/up deliver correct order

## Touched paths (allowlist)

- `userspace/libs/hid/` (new; HID event parser)
- `userspace/libs/touch/` (new; touch event normalizer)
- `userspace/libs/keymaps/` (new; or extend `userspace/libs/ime-keymaps/` from `TASK-0146`)
- `userspace/libs/key-repeat/` (new)
- `userspace/libs/pointer-accel/` (new)
- `tests/input_v1_0_host/` (new)
- `docs/input/overview.md` (new, host-first sections)

## Plan (small PRs)

1. **HID + touch event libraries**
   - HID event parser
   - touch event normalizer
   - host tests

2. **Keymaps + repeat + accel**
   - keymaps library (or extend existing from `TASK-0146`)
   - key repeat library
   - pointer acceleration library
   - host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- HID event parsing works correctly.
- Touch event normalization works correctly.
- Keymaps mapping works correctly for EN/DE.
- Key repeat timing is correct.
- Pointer acceleration curve is monotonic & bounded.
