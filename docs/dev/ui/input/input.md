<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Input

Input is routed through the window manager and translated into UI events.

Goals:

- deterministic delivery order,
- consistent focus model,
- accessibility semantics (actions/roles),
- cross-device affordances (touch + pointer + keyboard).

## v2a windowd routing baseline

`TASK-0056` makes `windowd` the single authority for the first functional input-routing baseline.

- Pointer hit-test walks the committed layer tree from topmost to bottommost visible surface.
- Focus follows a routed pointer-down event.
- Keyboard delivery targets the focused surface only.
- Stale or unauthorized surface references reject deterministically.
- Input event queues are bounded; overflow is a stable reject, not a silent success.

The v2a baseline intentionally does not claim cursor visuals, pointer polish, latency budgets, HID/touch device plumbing, or WM-wide shortcuts. Those remain follow-up scope.

## v2a visible input baseline

`TASK-0056B` extends the same `windowd` authority path with the smallest
deterministic QEMU-visible input proof:

- a deterministic pointer sequence updates a software cursor drawn by `windowd`,
- hover over the proof surface draws a deterministic hover affordance,
- pointer-down focus transfer draws a deterministic focused-surface affordance,
- one launcher proof surface visibly changes state after a routed click.

The marker ladder is accepted only after routed state and visible frame evidence
exist:

- `windowd: input visible on`
- `windowd: cursor move visible`
- `windowd: hover visible`
- `windowd: focus visible`
- `launcher: click visible ok`
- `SELFTEST: ui visible input ok`

Launcher and selftest remain proof consumers; they do not own hit-test, focus, or
cursor rendering. Live host mouse/keyboard input through QEMU is pulled forward
into `TASK-0252`/`TASK-0253` immediately after 56B rather than implemented as a
56B inputd-light path. This slice still does not claim the full HID/touch/keymap/IME
input stack, gestures, drag/drop, latency budgets, WM-v2 behavior, or kernel
production-grade closure.

## v1.0a host input core

`TASK-0252` implements the host-first input-core authority that later `inputd`
and IME work must reuse instead of duplicating:

- `userspace/hid/` parses USB-HID boot keyboard and mouse reports into
  deterministic logical events with stable reject classes,
- `userspace/touch/` normalizes transport-neutral touch samples into bounded
  `down -> move* -> up` sequences,
- `userspace/keymaps/` is the shared base keymap authority for `us`, `de`,
  `jp`, `kr`, and `zh`, including deterministic modifier handling and the
  shared `Ctrl+Space` IME-switch primitive,
- `userspace/key-repeat/` provides deterministic repeat scheduling over an
  injectable monotonic time source,
- `userspace/pointer-accel/` provides a bounded monotonic linear acceleration
  curve.

Host closure for this slice is behavior-first and marker-free:

- `cargo test -p input_v1_0_host -- --nocapture`

The host proof package freezes Soll vectors and `test_reject_*` behavior for HID
malformed frames, touch lifecycle rejects, invalid repeat settings, and invalid
pointer-accel configuration. This slice still does not claim live OS/QEMU input
ingestion, `inputd`/`hidrawd`/`touchd`, DTB/device wiring, `nx input`, IME/OSK
behavior, gestures, or any transfer of hit-test/focus authority away from
`windowd`; those remain follow-up scope in `TASK-0253` and later IME tasks.
