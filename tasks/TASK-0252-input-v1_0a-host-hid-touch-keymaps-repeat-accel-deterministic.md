---
title: TASK-0252 Input v1.0a (host-first): HID/touch event core + keymaps + key repeat + pointer acceleration + deterministic tests
status: Done
owner: @ui
created: 2025-12-29
depends-on:
  - TASK-0056B
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC (contract seed): docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md
  - Production gates: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Visible input baseline: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - Later IME consumer: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic input stack foundation:

- HID event parsing (USB-HID boot protocol),
- touch event normalization (I²C touch stub),
- keymaps (US/DE/JP/KR/ZH base),
- key repeat (configurable delay/rate),
- pointer acceleration (simple linear curve).

This task is pulled directly after `TASK-0056B` so live input can be built on a
proper reusable core instead of a 56B-only inputd-light path. It delivers the
**host-first core** (HID/touch parsing, keymaps, repeat, accel) that can be
reused by both `inputd` (low-level routing) and later `imed` work (`TASK-0146`).

It contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) by
closing the deterministic host-side input-core contract that `TASK-0253`
consumes for live OS/QEMU input.

## Goal

Deliver on host:

1. **HID event parser library** (`userspace/hid/`):
   - parse HID reports for keyboard and mouse (boot protocol subset)
   - event structure: `HidEvent { tsNs, kind, code, value }` where `kind` is "key", "rel", "btn"
   - deterministic parsing (no host-specific behavior)
2. **Touch event normalizer library** (`userspace/touch/`):
   - normalize touch events from I²C touch controller
   - event structure: `TouchEvent { tsNs, x, y, type }` where `type` is "down", "move", "up"
   - deterministic normalization
3. **Keymaps library** (`userspace/keymaps/`):
   - table-driven mapping for US/DE/JP/KR/ZH base
   - modifiers handling (including AltGr for DE)
   - IME switch key (e.g., `Ctrl+Space`)
   - deterministic mapping (no host locale leakage)
4. **Key repeat library** (`userspace/key-repeat/`):
   - configurable delay and rate (defaults: delay_ms, rate_hz)
   - deterministic timing (injectable time source in tests)
5. **Pointer acceleration library** (`userspace/pointer-accel/`):
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

- **No duplicate keymap authority**: This task provides the base keymaps library. `TASK-0146` must reuse/extend it for IME behavior to avoid drift.
- **No duplicate input authority**: this task is an event-source/core library layer only; routing/hit-test/focus authority remains in `windowd` and later `inputd` integration (`TASK-0253`).
- **Determinism**: HID parsing, touch normalization, keymaps, repeat, and acceleration must be stable given the same inputs.
- **Bounded resources**: keymaps are table-bounded; repeat timing is bounded.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- **Rust safety/quality floor**:
  - use newtypes for domain IDs/codes/ranges (avoid primitive obsession),
  - avoid unsafe `Send`/`Sync` impls; keep ownership explicit and thread guarantees compiler-driven,
  - annotate fallible/value-returning APIs with `#[must_use]` where dropping results would hide bugs.
- **Maintainable structure**: no monolith crate entrypoints; split into focused modules and keep crate public APIs small and explicit.

## Security / authority invariants

- Fail-closed parsing: malformed HID/touch frames must return stable reject errors.
- No host-locale leakage: keymap resolution must be table-driven and deterministic.
- Bounded repeat and accel math: reject invalid configuration ranges instead of clamping silently.
- No raw event payload dumping in success markers/log lines; only bounded metadata.
- This task must not claim live device trust; OS/QEMU source authenticity is owned by `TASK-0253`.

## Red flags / decision points

- **RED (keymap authority drift)**:
  - Do not create parallel keymap tables that conflict with `TASK-0146` (IME keymaps). Share the same keymap library or explicitly document the relationship.
- **YELLOW (key repeat determinism)**:
  - Key repeat timing must use injectable time source in tests (not `std::time::SystemTime`).
- **YELLOW (scope drift)**:
  - Do not absorb OS/QEMU service integration, DTB wiring, or `nx input` CLI behavior (all are `TASK-0253` scope).

Red-flag mitigation now:

- codify one shared keymap crate API for `inputd` and future `imed`,
- add explicit reject tests for malformed reports, invalid repeat configs, and accel bound violations,
- keep host-first proofs as the authority for parser/mapping/repeat/accel behavior,
- document non-claims so 0252 cannot be interpreted as live input closure.

## Contract sources (single source of truth)

- RFC contract seed: `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md`
- Testing contract: `scripts/qemu-test.sh`
- Later IME consumer: `TASK-0146` (US/DE IME behavior built on the shared keymap base)
- Gate quality mapping: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E, production-floor)

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p input_v1_0_host` green (new):

- keymap mapping EN/DE for umlauts and symbols
- key repeat timing (delay/rate) with simulated time
- pointer acceleration curve monotonic & bounded
- touch synthetic sequence → press/move/up deliver correct order
- malformed HID/touch frames reject deterministically
- invalid repeat/accel configs reject deterministically

Test-first order (required):

- write Soll-behavior tests and reject tests first (or in the same change before behavior markers/docs),
- then implement the minimal code to satisfy those tests,
- no success markers in this task; host assertions are the authoritative proof.

## Touched paths (allowlist)

- `userspace/hid/` (new; HID event parser)
- `userspace/touch/` (new; touch event normalizer)
- `userspace/keymaps/` (new shared base keymap library)
- `userspace/key-repeat/` (new)
- `userspace/pointer-accel/` (new)
- `tests/input_v1_0_host/` (new)
- `docs/dev/ui/input/input.md` (extend with host-core contract notes)

## Plan (small PRs)

0. **Proof-first test scaffold**
   - create `tests/input_v1_0_host/` suites for Soll behavior + `test_reject_*` paths
   - freeze expected behavior vectors before implementation

1. **HID + touch event libraries**
   - HID event parser
   - touch event normalizer
   - satisfy host tests

2. **Keymaps + repeat + accel**
   - shared keymaps library
   - key repeat library
   - pointer acceleration library
   - satisfy host tests

3. **Docs**
   - host-first docs

## Acceptance criteria (behavioral)

- HID event parsing works correctly.
- Touch event normalization works correctly.
- Keymaps mapping works correctly for EN/DE.
- Key repeat timing is correct.
- Pointer acceleration curve is monotonic & bounded.
- Required reject paths are covered with deterministic `test_reject_*` behavior.
