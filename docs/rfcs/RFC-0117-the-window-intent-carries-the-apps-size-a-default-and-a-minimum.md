---
schema: 1
id: RFC-0117
title: "The window intent carries the app's size: a default and a minimum"
status: draft
owners: ["@ui"]
created: 2026-11-09
subsystem: ui
tasks: [TASK-0353]
depends_on: [RFC-0116]
links:
  - label: "Design doc"
    target: "`docs/dev/ui/patterns/windowing/window-intent.md` (`chrome = intent ⟂ policy`; the intent vocabulary this RFC extends)"
  - label: "Related RFCs"
    target: "`docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md` (the surface create the intent rides in)"
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0117: The window intent carries the app's size — a default and a minimum

## Why

An app cannot say how large its window should be. windowd opens every freeform window at ¾ of the
display's width and ⅚ of its work area and lets a resize shrink it to 204 × 120 px, whatever the app
is. The calculator, designed as a 340 × 600 window, opens at 960 × 625 and can be dragged down to a
size where its keypad no longer fits; the operator's rule (2026-11-09) is that a calculator window is
never smaller than a phone-sized layout and grows from there. The window-intent design doc already
names a `defaultSize`; this RFC makes the size part of the intent the app declares and the window
manager honours.

## Scope

- **Owns:** the size fields of the window intent (DSL, Scene IR, the intent bytes of the surface
  create and `OP_SURFACE_INTENT`), and the rule by which windowd applies them.
- **Does not own:** how an app lays itself out inside the size it gets (the app, RFC-0116's rules),
  window positions (windowd places windows), a maximum size or an aspect lock (not needed by any app
  yet), the windowing policy (systemui).

## Contract (normative)

**DSL.** `Window { … }` takes four optional integer fields, in px at desktop scale:
`width`, `height` (the default content size of a freeform window) and `minWidth`, `minHeight`
(the smallest content size the app tolerates). Each is `1..=8192`; a minimum larger than the default
on the same axis is a compile error; a field absent is "the window manager decides".

**Scene IR.** The program's `window` struct gains `width`, `height`, `minWidth`, `minHeight`
(`UInt16`, 0 = absent), appended after `resizable`.

**Wire.** `OP_SURFACE_INTENT` and `SURFACE_CREATE` each gain a `WindowSize` appended at the
end of the frame: `width:u16, height:u16, min_width:u16, min_height:u16` (little-endian,
0 = absent; `nexus-display-proto::surface_intent`). The frames grow by 8 bytes and stay
length-exact: a frame without the size is malformed, never read as "no size". Every in-tree
sender and receiver changes in the same commit (no mixed-version peers exist). The intent's size
drives the opening rect windowd answers; the create's minimum is what windowd keeps for every
later resize, tile and restore.

**windowd** composes the size from the intent, clamped by the policy and the display:
1. The window's minimum = the larger of the app's minimum and the window manager's own minimum,
   but never more than the work area (a minimum the display cannot hold is the work area).
2. A freeform window opens at the app's default size (clamped to `[minimum, work area]`); without
   one, at the window manager's default (clamped to the minimum).
3. An interactive resize, a tile and a restore never make the window smaller than its minimum; a
   tile zone smaller than the minimum gives the window its minimum on that axis.
4. Fullscreen and maximized windows take the work area as before.

## Invariants

- A window's content size is never below its minimum and never beyond the work area —
  `test_reject_a_resize_below_the_app_minimum` (windowd `window_size`), `test_reject_a_minimum_beyond_the_display`
  (the minimum clamps to the work area).
- The DSL refuses a default smaller than the minimum and out-of-range sizes —
  `test_reject_a_default_below_the_minimum`, `test_reject_a_window_size_out_of_range` (dsl-core).
- The intent's size fields are bounded on decode (`u16`, then clamped to the work area): an app
  cannot make windowd allocate a surface larger than the display —
  `test_reject_an_intent_without_its_size` (nexus-display-proto) keeps the frame length-exact.

## Failure model

An absent field is the window manager's choice (today's behaviour). An out-of-range or inverted
size never reaches the wire (compile error). A peer sending sizes the display cannot hold gets the
work area, not an error.

## Proof

```bash
cargo test -p nexus-dsl-core window       # parse, bounds, inverted min/default
cargo test -p nexus-display-proto intent  # the 8 intent bytes round-trip
cargo test -p windowd --test window_size # default, minimum, clamp to the work area, tile
just test-os usb-visible                  # the calculator opens at its default size
```

## Alternatives considered

- **A per-app size table in windowd** — puts app knowledge into the compositor (the boundary rule:
  window UI and app facts belong to the app); rejected.
- **The app resizes itself after the first rect** — races the window manager, which owns geometry;
  rejected.
- **A maximum size and aspect lock now** — no app needs them; they stay out until one does.
