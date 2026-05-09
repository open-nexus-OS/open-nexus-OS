<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display Output Service Chain

The live visible-output path is service-owned:

`hidrawd -> inputd -> windowd -> fbdevd -> ramfb`

`selftest-client` is only an out-of-band observer. It polls `fbdevd` for
`VisibleState` and emits proof markers only after the service state already
contains the required evidence.

## Authority Boundaries

- `hidrawd` owns hardware ingress and normalized HID delivery.
- `inputd` owns live pointer/keyboard routing state and delivery accounting.
- `windowd` owns scene state, hit-test, focus, compose, and present generation.
- `fbdevd` owns framebuffer capability use, `ramfb` setup, final scanout writes,
  and visible-state replies.
- `init-lite` owns capability routing and endpoint rights.

## Minimal Userspace Reactor

Long-lived display/input work runs in service-owned userspace reactors, not in
the kernel and not in `selftest-client`.

For the live cursor path, `fbdevd` drives a small budgeted display tick:

- drain bounded service requests,
- sample upstream `VisibleState` through a short bounded `inputd` RPC,
- compose/present through `windowd`,
- write the latest frame or cursor-only dirty rows to `ramfb`,
- yield cooperatively.

The reactor must not let a slow upstream poll block a display refresh for a full
frame budget. If `inputd` does not answer quickly, `fbdevd` keeps ownership of
the last service state and tries again on the next tick.

Cursor-only movement is the latency-sensitive case. It redraws only the physical
rows that contain the old and new cursor rectangles; keyboard, click, scene, or
display-bit changes still use a full service-owned present.

The coordinate contract follows normal screen-space direction:

- positive relative X moves the cursor right,
- negative relative X moves the cursor left,
- positive relative Y moves the cursor down,
- negative relative Y moves the cursor up.

`inputd` owns the canonical pointer state in physical display coordinates.
`windowd` remains hit-test/focus authority on the proof scene by transforming
that display position into its logical `64x48` route space, while the live
visible framebuffer consumes the physical display position directly.

This intentionally mirrors the OpenHarmony/OHOS split: pointer events carry a
screen/display-relative position for global routing and a window/component
relative position for delivery. Our current minimal version now keeps the
canonical state in display space, maps absolute devices across the full visible
bootstrap mode, transforms to window-space only for `windowd` delivery, and
derives hover from the routed proof-scene position instead of from a framebuffer
scale-back shortcut.

## Minimal Closure Rule

Every display-output fix must identify the first broken hop and add the smallest
service-level proof for that hop before relying on QEMU:

1. capability route/rights,
2. protocol request/reply,
3. owner service state transition,
4. downstream telemetry/output,
5. observer marker.

If QEMU reports a stable missing marker while host tests are green, the green
tests are incomplete for this hop.
