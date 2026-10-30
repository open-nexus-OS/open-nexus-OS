---
title: TASK-0346 Instant input — keys never lost, the caret at once, the editing a person expects, every shortcut in one place
status: Done (2026-10-30 — board cycles 78–79, `[PASS]` on every rung and on every act the operator did; test-all green)
owner: @runtime @ui
created: 2026-10-30
depends-on: []
follow-up-tasks: [the key→present p99 on the board (the present path — RFC-0108, Block-4 layer 8), Ctrl+Space switching the input source, a controller-side time per USB report, the next package: the address space + app heap with the compositor's restart (6g) and dynamic window slots]
links:
  - RFC: docs/rfcs/RFC-0113-instant-input-every-event-counted-keys-never-lost-the-source-decides-the-route.md
  - ADR: docs/adr/0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md
  - Evidence: board cycles 72–77 (`inputd: imed send FAIL` with every focused field), build/logs/board--2026-10-30T08-29-17/
  - Queued by: tasks/TASK-0333 (the keyboard package after 6f), tasks/IMPLEMENTATION-ORDER.md row 6
---

# TASK-0346 — Instant input

## The operator's ask (2026-10-30)

"Instant": a click into a text field shows the blinking caret at once, typed letters appear at
once, the caret moves with the arrows, text is selected with the mouse, with Shift+arrows, words
with Ctrl+Shift+arrows — measured, with warnings and errors when a signal does not arrive. Toward
the endgame: high-rate input (up to 1000 Hz) only for programs that need it (drawing, games),
everything else grouped. A shortcuts page under General management › Input with categories on
the left and the shortcuts in the main area, every shortcut we already have in one place —
prepared for rebinding, rebinding not yet possible. One package; the architecture chosen after a
comparison of the common models: **the input source decides the route** and **the compositor owns
the caret** (ADR-0090).

## Found before the cut (2026-10-30, read end to end)

1. Letters go inputd → imed → windowd → app; inputd → imed and imed → windowd are non-blocking
   sends that drop when the receiver is full (imed's queue: 8). imed runs waited exchanges with
   statefsd/settingsd on every focus change → its queue fills → `inputd: imed send FAIL` (cycles
   72, 73, 75, 77 — every cycle with a focused field). Chords take the blocking visible-state push.
2. Silent losses: imed → windowd (no line); a failed focus relay recorded as sent; a re-created
   surface clears the text focus without telling imed; a key can overtake its focus.
3. No event has a sequence number; nothing measures key → present; auto-repeat never runs on the
   OS (the repeat engine is armed but never ticked).
4. The caret: a static 2-px bar in the app raster, no blink (an unused blink helper exists), a
   click focuses without a repaint and always puts the caret at the end, a caret move repaints the
   window. Tap frames carry no modifiers and no click count.
5. Editing: arrows, Home/End, Shift selection, Ctrl+A/C/X/V, Backspace/Delete work; word moves and
   selection, Ctrl+Backspace/Delete, undo/redo, mouse selection, horizontal scroll are missing; the
   keymap refuses every other Ctrl combination.
6. Shortcuts: ~30 bindings, matched by a hard-coded `match` in inputd (`key_facts.rs`) and the
   keymaps crate; hand-copied into five language files and documents (one copy wrong); loose
   modifier match; `ui.tile.chords` gates only part of the chords.
7. Settings: General management exists; "Input" is a group there; sub-pages exist (OpenSub/Back);
   the window is already two-pane.
8. In the way, taken along: the chat field's Stack sends the message on a tap; a field inside a
   modal can dismiss it; app-host's 64-byte event buffer truncates commits over 57 bytes; Enter
   and Tab are ignored; Esc closes only `.overlay(modal)` layers; QEMU's default virtio input path
   maps no arrows, Home, End or Delete.

## Phases (each with its stop condition and proof)

- **P0 — every event counted.** `seq`/`t_ns` from hidrawd on (RFC-0113 §1); counters per hop;
  session aggregates in app-host (§6); a typing scenario that types faster than a person (QMP,
  press→release 10–30 ms) on the usb-visible lane. Stop: the counters name today's losses.
- **P1 — keys never lost.** The reliable key channel inputd → windowd → field; layout sources in
  inputd (keymap + the dead-key composer moved from imed); composition sources via imed behind
  windowd; windowd's focus order (one ordered channel per hop); imed's stores off the key path;
  auto-repeat on a kernel timer while a key is held; the virtio key map completed. Stop: the
  typing scenario's counts whole (`chars=28 actions=1 timed=29`), no loss line.
- **P2 — the instant field.** The caret declared to windowd and blinked there (RFC-0113 §5); a
  click places the caret (x→index with the painter's metrics); tap/drag frames with modifiers and
  click count; drag, double/triple click, Shift+click selection; Ctrl/Ctrl+Shift word moves,
  Ctrl+Backspace/Delete, undo/redo; horizontal scroll; the bugs of finding 8. Stop: click→caret in
  the first frame after the tap reached the field; host tests for the editing core.
- **P3 — one shortcut registry.** `userspace/shortcuts` (id, category, chord, owner, action,
  active); inputd matches from it with exact modifier sets; the settings page General management ›
  Input › Keyboard & input › Keyboard shortcuts — categories left, the list right, the rebinding
  controls shown disabled; the language files' chord copies replaced by the registry's names.
- **P4 — delivery classes.** The `frame` class carries pointer history in the format; the `stream`
  class is declared and refused (`test_reject_stream_class_without_a_grant`).
- **P5 — proof.** Gates, lanes, test-all, board cycle (typing, caret, selection, the shortcuts
  page), docs sweep, one commit proposal.

## Progress

### P0 + P1 — built and host-proven (2026-10-30); the usb-visible lane green; the board pending

- **Identity:** hidrawd numbers every wire chunk per device stream (`input_live_protocol::
  SeqStreams`, `seq_first` in wire v3); inputd checks every batch (`inputd: FAIL input gap (`, a
  restart said plainly); a key keeps its event's `seq`/`t_ns` to the field (`TextStamp` in
  `OP_SURFACE_TEXT`); a repeat carries its held key's.
- **The key channel** (`OP_PUSH_KEY_INPUT`, input family op 6): inputd translates a layout source
  (keymap + the dead-key composer, moved from imed — `inputd::key_channel`), marks a composition
  source's keys for imed, and sends ≤12 entries per frame BLOCKING on the same channel as the
  visible state (the click's edge always first); windowd delivers to the window-focused surface,
  the app to its field in its own event order; composition keys go to imed behind its session on
  windowd's ordered outbox. Decisions in `windowd::key_gate` (host-tested `test_reject_*`). The
  seed's "focus generation" became one ordered channel per hop — no stamp needed.
- **No hop drops a key:** xhcid holds refused keyboard report frames in order (`HELD_MAX` 8, a
  pointer's 4; past them `xhcid: FAIL keyboard reports lost (`); hidrawd → inputd blocking (inputd
  answers batches without waiting: no cycle); inputd → imed's layout relay blocking; imed's
  storage work runs when its queue is empty; imed → windowd blocking on the declared leg.
- **Auto-repeat** on inputd's one-shot timer only while a key is held; one repeat per wake, never
  a burst after a stall (`key_repeat`: a stall past ~2 s used to stop the repeat until release).
- **The virtio key map** completed (arrows, Home/End, Delete, Page Up/Down, Insert, F1–F12, the
  keypad, the ISO `<>|` key) — QEMU's arrows did nothing before.
- **Measurement:** `apphost: input session (chars= actions= timed=) key_present <verdict>
  (p50_us= p99_us= max_us=)` once per focus session, never for a secure field; loss lines fail a
  lane and a board run (`qemu-test.sh`, `board-test.sh`); per-second rate lines (xhcid, hidrawd,
  inputd) count pointer traffic only — keyboard batches per second were typing rhythm in the log.
- **The lane:** usb-visible types `fastlettersarrivewithoutloss` at 30 ms a key (the scenario's
  declared `pace_ms`) and expects `apphost: input session (chars=28 actions=1 timed=29)`.
- **The first lane run (2026-10-30) did what the counters are for:** every step to the typing
  passed; the fast typing lost 4 of 28 keys and said so (`WINDOWD: FAIL key input (reason=
  undeliverable lost=4)`) — the shell needed 1.4 s per keystroke under emulation (`apphost: hops
  input` 1.4 s mean, 3.8 s max: one re-emit and one service query per key; the clipboard round's
  session `key_present` p50 2.8 s), so windowd's 16 ordered slots for it overflowed. Fixed at
  the cause of the loss: a client behind gets consecutive commits JOINED in its outbox (every
  character, in order, the first key's stamp) and applies a run as one edit. The per-keystroke
  cost itself (a synchronous service query before the frame that shows the letter) is measured
  here and judged on the board (QEMU timing is not the contract).
- Open from P1: the board's typing rung (P5).

### P2 — built and host-proven (2026-10-30); the usb-visible lane green; the board pending

- The word commands (Ctrl/Ctrl+Shift+arrows, Ctrl+Backspace/Delete) and undo/redo (Ctrl+Z,
  Ctrl+Shift+Z, Ctrl+Y — by the layout's letter; a typed run one step, a paste one step) through
  keymap → inputd → windowd → app-host → `textedit_core` (`words`, `history`).
- The pointer's caret: a press places it where it landed (the painter's glyph metrics),
  Shift+click and a drag extend, a double click takes the word, a triple everything — the tap
  frame carries the press's modifiers and click count (`surface_input::PointerFacts`; inputd's
  visible state carries the modifiers held, windowd counts clicks: 500 ms, 4 px); a re-tap of the
  focused field keeps its caret.
- The caret is a compositor layer (`OP_SURFACE_CARET`): app-host declares its rect after a
  present or a motion (hidden while a selection shows); windowd draws one solid patch only while
  the surface holds the window focus and shows, clipped to it, and blinks it on its own one-shot
  timer (530 ms halves, solid after a reset, at rest after 10 s) — no app wake per blink. The
  timer's endpoint is drained on every loop pass (a declared endpoint's EOF latch spun windowd
  in the first try).
- A field scrolls horizontally to keep its caret in view (`textedit_core::view::follow`; the
  focused run painted whole, shifted, clipped to its box); Enter submits (`on Submit`), Tab and
  Shift+Tab walk the fields.
- The usb-visible lane (2026-10-30, `build/logs/usb-visible--2026-10-30T13-01-18`): every step
  green — `windowd: caret layer on`, the fast typing `chars=28 actions=1 timed=29` with no loss
  line. `key_present` reads FAIL there (p50 4.5 s under emulation: the shell's per-keystroke
  query and re-emit) — QEMU timing is not the contract; the board judges it.
- Open: the board's caret and selection acts (P5).

### P3 — built and host-proven (2026-10-30); the board pending

- **The registry** `userspace/shortcuts` (no_std): 35 entries in four categories (windows 10,
  screenshots 3, text 21, input 1) — `id`, `category`, `action` (its owner follows), `chords`
  (the first the primary; alternates act the same), `active`. A chord is an exact modifier set
  and a key by its place or by the active layout's letter. Its tests keep the chords unique, the
  matching exact (`test_reject_a_shortcut_with_extra_modifiers`, `…_with_a_missing_modifier`),
  plain typing free of the letter lookup, and every id named in five languages and on the page.
- **Matched from it, in one place each:** inputd's window and capture chords
  (`shortcuts::system_action`; `key_facts::wm_chord_for`/`capture_key_for` deleted — the old
  match let any extra modifier through); the keymap's field commands and the input-source switch
  (`shortcuts::lookup`; its three hand tables deleted; `keymaps::EditKey` IS the registry's;
  `Modifiers` carries Alt and Super so Alt+← moves no caret). windowd pins the registry's window
  codes to `zones::CODE_*`.
- **Listed from it:** Settings › Allgemeine Verwaltung › Eingabe › Tastaturkurzbefehle —
  categories left, the category's shortcuts right (`svc.shortcuts.list(category, locale)`, an
  in-process app-host verb; key caps German for `de*`, Latin otherwise), each row's "Ändern" and
  the "Standard wiederherstellen" button drawn disabled; Ctrl+Space listed dimmed ("Noch nicht
  verfügbar": matched, nothing switches on it yet). The language files' hand copy of the window
  chords (one of its lines wrong) became a pointer to the page; the tiling and capture docs point
  at the registry.
- Proof: `cargo test -p shortcuts` (11), `-p input_v1_0_host --test keymaps_contract` (12),
  `-p inputd` (`tests/key_facts.rs` 7), `-p windowd --test tile_zones` (4),
  `-p dsl_apps_conformance --test settings` (14, `general_shortcuts_row_opens_the_registry_page`);
  `just diag` clean; app-host's OS build clean.

### P4 — built and host-proven (2026-10-30); the usb-visible lane green; the board pending

- **The `frame` class's history:** `nexus_display_proto::surface_motion::MotionBatch` — at most 16
  samples (x, y, `t_ns`, `seq`), the two oldest folding past the bound; compact on the wire (i16
  positions, ages in µs) so a full batch fits a client event frame. inputd keeps a sample per
  report (the cursor's summed, accelerated step unchanged; each report placed along it by its
  share of the raw motion; the last IS the cursor) and the update push carries them
  (input-live wire v4). windowd stages them with the motion (an edge keeps its own), sends them
  on the MOVE or DRAG in the surface's space, keeps what the frame pace held back and what a full
  queue refused, and its client outbox joins a replaced MOVE or DRAG's samples into the next.
  app-host decodes them and counts a drag's history on its trace line. hidrawd stamps a delivery
  once, so reports it receives together fold into one sample (a controller time per report is
  the USB driver's to add — not in this task).
- **The `stream` class:** `nexus.permission.INPUT_STREAM`, declared and refused — nxb-pack for
  every bundle type, policyd's publish as the backstop.
- **On the way:** `inputd: push hz=` counted every push — a push a key caused too, so typing
  rhythm reached the log; it counts pointer pushes only now.
- Proof: `cargo test -p nexus-display-proto` (the batch, the frame, the join),
  `-p input-live-protocol` (the push), `-p inputd --test pointer_motion` (3),
  `-p windowd --test input_staging` and its outbox tests, `-p nxb-pack --test repo_bundles`,
  `-p policyd` (`test_reject_stream_class_at_publish`); host and OS builds clean.

### P5 — gates, lanes and test-all green (2026-10-30); the board cycle pending

- `just check`, `just diag`, `just dep-gate`, `just test-host` green; `just ci-os-usb-visible` green
  (`build/logs/usb-visible--2026-10-30T15-30-13`): the fast typing whole, the window chords from the
  registry (left half, return, minimize, restore, close), `windowd: caret layer on`, the selection
  drag with its history (`frames=10 samples=10 max=1` — QEMU's injected moves are one per frame).
- `just test-all` green (17 QEMU lanes, `testall_exit=0`, 2026-10-30).
- Board acts declared for the cycle: `caret` (`windowd: caret layer on` → `apphost: input session (`
  with editing actions, no loss line) and `shortcuts` (`apphost: shortcuts listed (`), besides
  `typed` and `clipboard`.

### The first board cycle (2026-10-30, `build/logs/board--2026-10-30T17-18-32`) and its fixes

- **Measured:** typing whole and fast (`apphost: input session (chars=29 actions=7 …) key_present
  warn (p50_us=14013 p99_us=56035 …)`, no loss line); the pointer's history real at the device
  rate (`apphost: drag … history(frames=36 samples=80 max=6 folded=0)`); the window chords and the
  modal, tile, lifecycle and restore acts.
- **The caret did not show** although windowd said `windowd: caret layer on`: the frame's layer
  shares counted the surface's declared layers only, so its share was spent before the caret —
  `windowd: layers over the frame bound (dropped=1)` right behind it, on the board AND on the
  QEMU lane (which proved the line, not the pixels). Fixed: the caret counts in its surface's
  demand and its layer is set aside before the surface spends the share (`LayerQuota::set_aside`,
  host test) — a tight frame drops the surface's own last layer, never the caret.
- **Settings crashed opening the shortcuts page:** a `[USER-PF]` store just below the app's stack
  — app-host walks its scene recursively and the page was ~7 KiB deeper than the 16 pages
  abilitymgr asked for (host probe: general 91 KiB, updates 89 KiB, the shortcuts sub-page
  97 KiB). Fixed at the cause: execd reserves every app-host's stack itself, 8 MiB of address
  space, LAZY (ADR-0077: a page costs a frame only when reached; the memory authority decides
  RAM) — no small fixed budget an app outgrows. The shortcuts became a window on the page root
  (the operator's model, the reference desktop's) — 91 KiB on the probe.

### Delivered (2026-10-30) — and what moves on, honestly

- **Board cycle 79** (`build/logs/board--2026-10-30T18-10-02`, dev-49e60617): `[PASS]` ladder 99/99,
  manifest clean, no FAIL line beyond the nine known board reds; acks desktop, typed, pointer,
  modal, tile, clipboard, screenshot, gpu-desktop, **caret** (`windowd: caret layer on`, no layer
  dropped behind it, sessions with 14 and 4 editing actions), **shortcuts** (`apphost: shortcuts
  listed (rows=10)`); lifecycle and restore not done in this boot (the operator's choice — their
  packages proved them in cycles 72b/77). The operator: "sehr cool, recht performant".
- **Measured on the board:** typing p50 14–15 ms key→present, no key lost, no loss line; the
  pointer's history at the device's rate (`history(frames=195 samples=618 max=16 folded=350)` on a
  fast drag — the bound folds as designed).
- **Moves on (recut):** the key→present **p99** is 56–95 ms on the board (`warn`, budget 50 ms) —
  the present path's (`gpud: ux frame` 56 ms mean on this board; RFC-0108, Block-4 layer 8), not
  the input path's. Ctrl+Space is matched, nothing switches yet. hidrawd stamps a delivery once
  (a controller time per report is the USB driver's). Found on the way, not this package's: the
  compositor's FOUR window slots — a fifth window's resize re-create was refused and its app-host
  exited (`WINDOWD: surface create FAIL (window slots exhausted)`) → the next package (the address
  space and the app heap with the compositor's restart, operator 2026-10-30).

## Touched paths

userspace/input-live-protocol · source/libs/nexus-wire (imed filter ops) ·
source/libs/nexus-display-proto (tap modifiers/clicks, the caret op) ·
source/libs/nexus-service-topology (the legs) · source/libs/nexus-ux-budgets ·
source/services/{hidrawd, inputd, imed, windowd, app-host, settingsd (no new keys)} ·
userspace/{keymaps, ime-core, ui/textedit_core, dsl/runtime, shortcuts (new)} ·
userspace/apps/{settings, chat} · source/apps/selftest-client (markers, the typing scenario) ·
scripts/qemu-test.sh · docs.

## Not doing (the scope firewall)

The `stream` class's ring and the 1000 Hz path · resampling and prediction · rebinding (shown
disabled) · inline composition preview in the field · multi-line fields (↑/↓) · graphemes and
bidi · touch, stylus, game controllers · Ctrl+Space source cycling (listed, inactive) ·
accessibility shortcuts.

## Definition of Done

- Host: the editing core, the registry matcher, the route and focus model, the caret scheduler,
  the codecs — with the `test_reject_*` of RFC-0113.
- QEMU: the typing scenario `lost=0`; key→present and click→caret within the budgets measured in
  P0; the usb-visible and lowmem lanes green; test-all.
- Board: the operator types, places the caret, selects by mouse and keyboard, opens the shortcuts
  page — each act with its lines; the session aggregates `lost=0`.
- Docs: RFC-0113, ADR-0090, docs/dev/ui/input, the settings README, CHANGELOG.
