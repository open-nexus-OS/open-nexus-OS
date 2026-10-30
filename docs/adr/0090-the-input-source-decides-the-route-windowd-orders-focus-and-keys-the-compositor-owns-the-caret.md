<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0090: The input source decides the route, windowd orders focus and keys, the compositor owns the caret

- Status: Accepted (2026-10-30, TASK-0346 — the operator's decision after comparing the models; proven on the board, cycles 78–79)
- Date: 2026-10-30
- Links:
  - Tasks: `tasks/TASK-0346-instant-input-keys-never-lost-the-caret-at-once-shortcuts-in-one-place.md`
  - RFCs: `docs/rfcs/RFC-0113-instant-input-every-event-counted-keys-never-lost-the-source-decides-the-route.md`
    (the contract), RFC-0075 (the input method service), RFC-0109 (surface layers)
  - Related ADRs: ADR-0069 (window-management chords), ADR-0075 (the compositor never waits on a
    client), ADR-0089 (lanes declared; the harness observes)

## Context

Every typed character travels inputd → imed → windowd → app today, plain German included. The
first two hops drop a frame when the receiver is full; imed waits on the state and settings
stores at every focus change, so its queue fills and keys are dropped — the board has lost typed
letters since cycle 64 (`inputd: imed send FAIL` in every cycle with a focused field). Focus
reaches imed on a second path, so a key can overtake its focus, a failed focus relay is never
repeated, and a re-created surface leaves imed without a field. The caret is part of the app's
raster: it appears only with the next repaint, does not blink, and every move repaints the window.

Compared before deciding (the operator asked for it): systems in wide use keep a layout — dead
keys included — as a cheap translation step and involve an input method only for composition
languages, either as a stage the app forwards keys through or as a central keyboard service;
none merges keys; pointer motion is batched per frame with every sample kept, and drawing
programs opt into the full rate.

## Decision

1. **The input source decides the route.** A layout source (de, us, dead keys included) is
   translated in inputd as library code; its text and editing actions reach the focused field
   through windowd on a reliable, ordered key channel. A composition source (ja, ko, zh) passes
   through imed as a filter stage behind windowd. Each source has exactly one route; a stall in
   imed can stall only composition.
2. **windowd orders focus and keys** — by one ordered channel per hop, not by a stamp: inputd
   sends the click's button edge and the keys on the same channel in the order they happened;
   windowd applies an edge the moment it drains it, so the tap reaches the app before the key
   behind it; the app receives tap and text on its one ordered event channel and routes the text
   to the field it focused (field focus is the app's, in its own event order). A key with nobody
   to take it is counted and said once; a key a living receiver could not be handed is a `FAIL`.
   imed holds no focus of its own beyond the filter session windowd opens before the keys, on the
   same ordered outbox.
3. **Keys are never dropped and never merged** on any hop; a full receiver applies backpressure.
   Pointer motion is batched per frame with every sample kept.
4. **The compositor owns the caret.** An app declares the caret (rect, colour, blink, reset);
   windowd draws and blinks it as a surface layer, holds it solid after an edit and stops it while
   the surface is neither focused nor visible. The selection highlight stays in the field's rows.

## Consequences

- **Positive:** plain typing has two hops instead of three and no input method in its way; focus
  and keys can no longer overtake each other; the caret appears in the next frame and blinks
  without waking the app; every key is accountable.
- **Negative / accepted cost:** the dead-key composer moves from imed into inputd (one engine,
  moved — not duplicated); windowd's text routing grows a filter leg to imed; composition sources
  keep a longer route (four hops), which their candidate UI already dominates.
- **Accepted gap (measured first):** a composition source's key typed in the instant between a
  click and the app's field-focus announcement reaching windowd composes under the window's
  session (Escape only); the layout route has no such window.
- **Not decided here:** the high-rate `stream` delivery class's implementation; rebinding.

## Alternatives considered

- **Every key through the input method, made lossless** — keeps two paths for focus and keys and
  the input method's stalls in every keystroke.
- **App-first: the app forwards keys to the input method** — the app would see keys before the
  system knows the field; windowd already owns focus and the text route.
- **The app blinks its caret** — a timer and a repaint per blink per field.
