<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Text Input

Contracts: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md` (focus, composition),
`docs/rfcs/RFC-0113-instant-input-every-event-counted-keys-never-lost-the-source-decides-the-route.md`
(the key route, the caret, editing), ADR-0090.

## Focus model (two-level, RFC-0075)

1. **Surface focus** — windowd authority (pointer/touch-down hit-test).
2. **Widget focus** — app authority: the DSL runtime focuses a TextField on tap
   (`View::focus_text_at`), tracks it across re-emits by binding identity (store + field path),
   and the host announces every transition upward via `OP_SURFACE_TEXT_FOCUS`.

## The key route (RFC-0113 §3)

The input SOURCE decides it. A layout source (de, us — dead keys included) is translated in
inputd (keymap + composer) and travels the key channel inputd → windowd → the focused field; a
composition source (jp, kr, zh) travels the same channel to windowd, which hands it to imed behind
the field's session. No hop drops a key: every send on the route waits for room, a client that is
behind gets consecutive commits joined, and a loss that still happens is a FAIL line naming its
hop. Apps never see raw key codes for text.

## Editing (TASK-0346 P2)

- **The caret** is a compositor layer (`OP_SURFACE_CARET`): the app declares its rect, windowd
  draws and blinks it (no app wake per blink), only while the surface holds the window focus.
- **The pointer** places the caret where it lands (the painter's glyph metrics); a drag,
  Shift+click, double click (word) and triple click (all) select.
- **The keyboard** moves by characters and words, selects with Shift, deletes words, undoes and
  redoes, copies, cuts and pastes — every chord from the shortcut registry
  (`docs/dev/ui/input/shortcuts.md`). Enter submits (`on Submit`), Tab and Shift+Tab walk the
  fields; a long value scrolls horizontally to keep the caret in view.
- **Measured** per focus session, never per key: `apphost: input session (chars= actions= timed=)
  key_present <verdict> (p50_us= p99_us= max_us=)`; a secure field is never measured.

## Password fields

`TextField { secure: true }` renders bullets (the real value never enters the scene tree),
reports `secure` in the focus snapshot, and downstream disables IME preview/candidates/learning
(RFC-0075 invariant).

## Deferred

Multi-line fields (↑/↓), graphemes and bidirectional text, inline composition preview,
accessibility semantics for input fields (a11y track).
