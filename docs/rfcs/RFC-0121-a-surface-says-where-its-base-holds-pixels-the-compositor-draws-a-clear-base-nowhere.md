---
schema: 1
id: RFC-0121
title: "A surface says where its base holds pixels: the compositor draws a clear base nowhere"
status: in_progress
owners: ["@ui"]
created: 2026-11-20
updated: 2026-11-22
subsystem: ui
tasks: [TASK-0354, TASK-0351]
depends_on: [RFC-0109, RFC-0114]
links:
  - label: "Surface structure"
    target: docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0121: A surface says where its base holds pixels: the compositor draws a clear base nowhere

## Why

The desktop's base (the shell, and the greeter) is painted at alpha 0 almost everywhere — the
wallpaper shows through it — yet windowd composes it as one full-screen layer, and every
full-screen frame pays for it: 2.07 Mpx at 4.86 ns/px, about 10 ms of each frame of the
fullscreen launcher's see-through fade (board 2026-11-20T15-34-30: the launcher's single pass
9.65 Mpx / ~35 ms). The base cannot be flagged opaque — that would be a lie and a hole — and
windowd cannot know where an app's pixels are without reading them. Only the app knows what it
painted. That is a wire field, hence this contract.

## Scope

- **Owns:** the `ink` declaration of a surface's base (where its base holds pixels with alpha > 0),
  its bound, its codec, the producer's duty (never under-declare) and the consumer's rule (draw the
  base only inside the ink).
- **Does not own:** the surface structure itself (RFC-0109), surfaces and their buffers (RFC-0114),
  the planner's damage (RFC-0101 §8), motion layers and glass (RFC-0109).

## Contract (normative)

- `SurfaceTree` (`nexus-display-proto`, the surface structure of RFC-0109) gains
  `ink: [Rect16; MAX_INK]` and `ink_count: u8`.
  - `MAX_INK = 8`.
  - `ink_count = INK_WHOLE (0xFF)`: not declared — the whole base, today's behaviour and every
    window's default.
  - `ink_count = 0`: the base holds no pixel; no body layer is drawn.
  - `1..=MAX_INK`: the base holds pixels only inside the union of `ink[..ink_count]`.
- **Producer (app-host).** Declared only for a surface whose base is painted at alpha 0 (the desktop
  and the greeter: `page_base_alpha() == 0`), recomputed after each relayout from the base pass's
  boxes and their painted rectangles (shadow and text halo reach, cut to the clip), merged greedily
  by the smallest area growth down to `MAX_INK`. A base box animated off identity → `INK_WHOLE`.
  Sent with the structure when it changed. The pure function lives in `app-host/src/ink.rs`.
- **Consumer (windowd).** The desktop body is one composite per ink rect (KIND_BODY, index i)
  instead of one full-screen layer; `INK_WHOLE` is the one full-screen piece of today through the
  same code. The surface's layer quota counts the pieces, all or none; short of quota → the one
  full-screen piece. A piece whose rect changes is a structure change (RFC-0101 §8 R4: old ∪ new).
  The top bar is untouched. windowd prints `windowd: body ink (n= px=)` on each change.
- **Versioning.** A structure without the field decodes as `INK_WHOLE`.

## Invariants

- Every base pixel with alpha > 0 lies inside the ink — over-declaring costs fill, under-declaring
  costs pixels, so the merge only grows the area. Proofs: `the_ink_holds_every_painted_pixel` (host
  oracle on the shell's and the greeter's scenes), `test_reject_a_merge_that_drops_a_pixel`.
- Bounded: at most `MAX_INK` rects, const-asserted against the surface's layer quota
  (`windowd scene/quotas.rs`).
- The codec is length-guarded: a count past `MAX_INK` other than `INK_WHOLE` is MALFORMED
  (`test_reject_an_ink_count_past_the_bound`); an empty rect, one past the base and two that
  overlap are refused with the structure, never clipped (`test_reject_an_ink_rect_past_the_base`); windowd never draws a body piece outside the surface
  (`test_reject_a_body_piece_outside_the_surface`).

## Failure model

- A malformed declaration is refused with the structure (as any malformed structure today).
- A producer that cannot bound its ink declares `INK_WHOLE`: the full base, never a hole.
- A quota too small for the pieces falls back to the one full-screen piece.

## Security (when relevant)

The declaration decides only how much of the app's OWN base is drawn: it can hide only that app's
own pixels, never another surface's. The count is bounded before parsing and every rect checked against
the surface. Identity stays the kernel's `sender_service_id` (no change).

## Proof

- Host: the codec tests above (proto), `ink.rs`'s oracle (app-host), the body pieces and the
  quota (windowd).
- QEMU: `just ci-os-many-apps`, `just ci-os-usb-visible` green; the desktop's pixels unchanged in
  their captures.
- Board: the launcher's single pass 8.4 → ≤ 6.4 Mpx and ~34 → ≤ 25 ms; the small launcher
  2.07 → ≤ 1.65 Mpx; `windowd: body ink (n= px=)` names the declared area.

## Alternatives considered

- **Declare where the base is clear.** The complement of a few ink rects is many clear rects; ink
  is the smaller list and fails safe by union.
- **Flag the base opaque.** A lie: the wallpaper would vanish behind it.
- **windowd reads the base's alpha.** It would read every app's pixels each change — the cost moves,
  and the compositor would inspect content it only composes by reference (RFC-0114 §6).
- **An opaque glass back for the fullscreen launcher from its first frame.** Refused by the
  operator: the fade stays see-through.
