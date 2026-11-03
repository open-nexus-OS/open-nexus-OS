---
schema: 1
id: RFC-0113
title: "Instant input — every event counted, keys never lost, the input source decides the route, the compositor owns the caret"
status: done
status_note: "2026-10-30 — TASK-0346, board cycles 78–79; recut: the key→present p99 on the board is the present path's, RFC-0108 / Block-4 layer 8 — see Closure"
owners: ["@runtime", "@ui"]
created: 2026-10-30
updated: 2026-10-30
subsystem: ui
tasks: [TASK-0346]
links:
  - label: ADRs
    target: "`docs/adr/0090-the-input-source-decides-the-route-windowd-orders-focus-and-keys-the-compositor-owns-the-caret.md`"
  - label: "Related RFCs"
    target: "RFC-0075 (the input method service), RFC-0099 (the USB host — xhcid, the HID boot class), RFC-0108 (the UX latency contract), RFC-0109 (surface layers: washes the compositor composes without a raster), RFC-0078 (settings keys and their watch)"
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0113: Instant input — every event counted, keys never lost, the input source decides the route, the compositor owns the caret

## Status at a Glance

- **Phase 0 (every event counted: identity, per-hop accounting, session aggregates, the fast-typing lane)**: ✅ lane (`chars=28 actions=1 timed=29`, no loss line) and board (sessions whole, no loss line)
- **Phase 1 (keys never lost: the reliable key channel, the source decides the route, windowd orders focus and keys, auto-repeat)**: ✅ lane and board (typing p50 14–15 ms key→present)
- **Phase 2 (the instant field: the compositor's caret, the pointer places it, selection by mouse and keyboard, word editing, undo)**: ✅ lane and board (`board-visual: caret` — after the layer-share fix of cycle 78)
- **Phase 3 (one shortcut registry: matched in one place, listed on one settings page)**: ✅ host, lane and board (`board-visual: shortcuts`, the shortcuts window)
- **Phase 4 (delivery classes: `frame` with history; `stream` declared and refused)**: ✅ host, lane and board (`history(… max=16 folded=350)` on a fast drag at the device's rate; `stream` refused at pack and at publish)

Closure (2026-10-30): every goal holds on the board — no key lost or merged, the caret in the
next frame where the click landed and blinking without the app, the editing a person expects,
one registry matched and listed, the history in the format. Recut: the key→present **p99** on
the board is 56–95 ms (`warn` against the 50 ms budget) while its p50 is 14–15 ms — the frames
themselves take that long on this board (`gpud: ux frame`, 56 ms mean): the present path's, not
the input path's (RFC-0108, Block-4 layer 8).

Definition: "Complete" = the contract below holds and its gates are green — zero keys lost on a
lane that types faster than a person, the key→present and click→caret budgets met as measured,
the caret visible in the first frame after the click reached the field, every shortcut matched
from the registry and listed on the settings page.

## Scope boundaries (anti-drift)

- **This RFC owns**: the identity of an input event (sequence + timestamp) and its accounting on
  every hop; the delivery guarantees per event class (keys: ordered, never dropped, never merged;
  pointer motion: batched per frame with every sample kept); the route of a key by its input
  source; the focus ordering of keys; the caret as a compositor-owned layer; the pointer
  frame's modifiers and click count; the shortcut registry's shape and its matching rule; the
  delivery classes `frame` and `stream`.
- **This RFC does NOT own**: composition engines (RFC-0075 keeps them), the on-screen keyboard,
  the window manager's chord semantics (ADR-0069 keeps them — this RFC only moves where they are
  matched), the shortcut editor (rebinding is a later phase of its own), touch, stylus and game
  controllers, multi-line text areas, grapheme clusters and bidirectional text.

### Relationship to tasks

TASK-0346 holds the stop conditions and proof commands of every phase.

## Context

Measured 2026-10-30 (board cycles 72–77, the repo's input chain read end to end):

- **The board loses typed letters, never chords.** A letter travels inputd → imed → windowd →
  app; the first two hops are non-blocking sends that DROP the frame when the receiver's queue
  is full (imed's request queue holds 8). imed runs waited exchanges with the state and settings
  stores on every focus change; while it waits, its queue fills and inputd drops keys —
  `inputd: imed send FAIL` in every cycle with a focused field (72, 73, 75, 77), none in the
  others. Chords leave inputd on the visible-state push, a blocking send, and arrive.
- **Silent losses besides:** imed → windowd drops without a line; windowd records the focus it
  relayed to imed as sent even when the send failed; a re-created surface clears the text focus
  without telling imed (typing dies after a tile or resize until the field is tapped again); a
  key typed right after a click can reach imed before the focus does.
- **Nothing is counted.** No event carries a sequence number; no hop knows it lost one; no line
  measures a key to the presented frame. `windowd: ux input` measures windowd alone (board:
  p50 57 ms, p99 240 ms, cycle 74).
- **The caret is not there at once.** A click focuses the field but repaints nothing — the caret
  appears with the first key; it always sits at the end of the text; it does not blink; a caret
  move repaints the whole window.
- **Shortcuts are hard-coded** in inputd (a `match`) and copied by hand into five language files
  and several documents; one copy is wrong; the modifier match is loose (any extra modifier still
  matches).

## Goals

1. No key is ever lost or merged between the device and the field; a loss that still happens is
   counted and named by its hop.
2. A click into a field shows the caret in the next frame, where the click was; a typed glyph
   appears in the next frame; the caret blinks without the app.
3. The editing a person expects: words by Ctrl+arrows, selection by Shift and Ctrl+Shift, by drag,
   double and triple click and Shift+click, deletion by words, undo and redo.
4. Every shortcut lives in one registry: matched there, listed from there.
5. The event contract already carries what a high-rate consumer needs, so a later stream class
   adds a delivery mode, not a format.

## Non-Goals

- The `stream` class's implementation (a shared ring per subscriber, delivery at the device rate).
- Resampling and prediction of pointer samples.
- Rebinding shortcuts (the settings page shows the controls disabled).
- Inline composition preview inside the field (the input method's strip keeps it).

## Constraints / invariants (hard requirements)

- **Keys are never dropped and never merged.** Every hop on the key path either delivers in order
  or applies backpressure to its sender (the sender parks reactively — no retry loop, no clock).
  (Joining committed text for a client that is behind is no merge: every character stays, in
  order — see §3.)
- **Focus order:** a key is delivered to the field that was focused when windowd received it.
- **Identity:** only inputd injects keys into windowd's text path; only windowd feeds the input
  method's filter; only the input method answers it — each checked by the kernel's
  `sender_service_id`, never a payload field.
- **Privacy:** no line is written per keystroke, per key-caused frame or per key-caused service
  call; accounting leaves a component only as aggregates; a secure (password) field is never
  measured.
- **Bounded:** every queue, batch and frame has a declared limit; a commit's length is bounded and
  every receiver's buffer holds the largest valid frame.
- **No fake green:** an `ok` aggregate only after the counted events were applied.

## Proposed design

### 1. Event identity (normative)

Every input event carries `seq: u32` — consecutive per source stream (one stream per device and
event class), wrapping — and `t_ns: u64`, the earliest time the system saw it (the host
controller's completion when the driver has it, else the ingress drain). Both travel unchanged to
the point where the field applies the event. A hop that receives `seq` not equal to its last
`seq + 1` from the same stream counts the gap.

As built (Phases 0–1): hidrawd is the first hop that numbers — every wire chunk of a device's
events gets `seq_first` from that device's stream (`input_live_protocol::SeqStreams`: eight
devices with a stream of their own, any further one sharing the overflow stream on both ends),
numbered before the send, so a chunk that never arrives is a gap. inputd checks every batch:
a gap is `inputd: FAIL input gap (device= missing= total=)`, a stream that starts over (hidrawd
restarted) a plain `inputd: input stream restart (device=)`. `t_ns` is the HID event's own
timestamp (hidrawd's drain); a key keeps its event's `seq` and `t_ns` on the key channel, and
windowd hands both to the field in the text frame's stamp (`surface_text::TextStamp`). A repeat
carries its held key's `seq` and the time it came due.

### 2. Delivery per class (normative)

| Class | Merged? | On a full receiver | Order |
|---|---|---|---|
| key press / release / repeat | never | backpressure (the sender parks) | per stream, preserved |
| text commit, preedit, editing action | never | backpressure | per field, preserved |
| pointer button, click | never | backpressure | preserved |
| pointer motion | per frame into one batch that keeps every sample (`seq`, `t_ns`, position) up to a bound; beyond it, the oldest samples of the batch fold into one | the batch grows | preserved |
| wheel | summed per frame, the count of notches kept | — | — |

As built (Phase 4): the batch is `nexus_display_proto::surface_motion::MotionBatch` — at most
16 samples (x, y, `t_ns`, `seq`), the two oldest folding into one past the bound (the newer
stands for both, the fold counted); on the wire i16 positions and ages in µs before the
newest, so a full batch fits a client event frame. One type on both hops: inputd keeps a
sample per report (the run of events with one timestamp) — the batch's summed, accelerated
step stays the cursor's, each report placed along it by its share of the raw motion, the last
at the cursor; the update push carries the samples since the last push (input-live wire v4).
windowd stages them with the motion (an edge leaves with its own), hands them to the MOVE of
the surface under the pointer or the DRAG of the press's surface in that surface's space, keeps
what the frame pace held back for the next MOVE and what a full queue refused for the DRAG's
retry, and a client's outbox joins a replaced MOVE or DRAG's samples into its successor. app-host
counts a drag's history on its trace line (`apphost: drag … history(frames= samples= max=
folded=)`). hidrawd stamps a delivery once: reports that reach it together share that time and
fold into one sample — a controller-side time per report is the USB driver's to add.

### 3. The input source decides the route (normative; ADR-0090)

- A **layout source** (de, us — dead keys included) is translated in inputd by the keymap and the
  dead-key composer as library code; the resulting text, editing action or key goes to windowd on
  the reliable key channel, and windowd delivers it to the focused field.
- A **composition source** (ja, ko, zh) is routed inputd → windowd → the input method (a filter
  stage: key in; commit, preedit or action out) → windowd → the focused field. The input method
  never performs a waited exchange while a key is queued for it; its stores are written outside
  the key path.
- **windowd owns focus** and therefore the order — by ONE ordered channel per hop, not by a
  stamp: inputd sends the visible state (the click's button edge) and the key frames on the same
  channel, in the order they happened; windowd applies a button edge the moment it drains it, so
  the tap reaches the app before the key behind it; the app gets the tap and the text on its one
  ordered event channel and routes the text to the field it focused. A layout source's text
  therefore goes to the surface that holds WINDOW focus — the app's field focus is the app's own,
  in its own event order. For a composition source windowd opens the input method's session for
  the field it was told of (else the focused window's, which takes Escape only) and sends the
  keys after it on one ordered outbox.
- **Who takes a key** (`windowd::key_gate`, host-tested): the key channel takes inputd's frames
  only; a key goes to the window-focused surface; a key with nobody to take it (no window yet, a
  window without its channel) is counted and said once (`windowd: key input without a receiver
  (first)`), never a FAIL — there was nobody to type into; a key a living receiver could not be
  handed is LOST: counted, `WINDOWD: FAIL key input (reason= lost=)`.
- **A client behind a fast typist** (measured 2026-10-30, the usb-visible lane's first run: the
  shell needed 1.4 s per keystroke under emulation — one re-emit and one service query each —
  so 28 keys in 1.7 s overran windowd's 16 ordered slots and 4 were lost, `WINDOWD: FAIL key
  input`): a committed text queued right behind another one in the client's outbox is APPENDED
  to it while the joined text fits a text frame (64 bytes). Not a character lost or reordered;
  the frame keeps its first key's stamp (each character is timed from it — an upper bound);
  an editing action is never joined. The app applies the run as one edit — one re-emit, one
  query — so a slow app catches up instead of falling further behind.
- **Every hop holds instead of dropping** (as built): xhcid keeps a keyboard's report frames the
  full client refused in order (eight places, a pointer's frames four of them; past them a frame
  is dropped and named — `xhcid: FAIL keyboard reports lost (`); hidrawd → inputd, inputd →
  windowd and the layout relay inputd → imed are blocking sends (no hop waits back on its sender:
  inputd answers hidrawd's batches without waiting); windowd → app and windowd → imed are the
  ordered outbox (ADR-0075); imed → windowd blocks on its declared leg. The input method's
  storage work for a focus change runs when its queue is empty, never between a focus and its
  keys.
- **Auto-repeat** is inputd's: a kernel timer armed only while a key is held, one repeat per
  wake — a consumer that fell behind gets one repeat and the rhythm resumes from it, never a
  burst of the repeats it slept through (`key_repeat::RepeatEngine::tick`).

### 4. Pointer frames to apps

The tap frame gains `modifiers` (a bit set: Shift, Ctrl, Alt, AltGr, Super) and `clicks` (1–3,
the click count within the double-click interval and slop); drag frames carry the same. Older
frames stay decodable (the frame's version byte selects the layout).

As built (Phase 2): `OP_SURFACE_INPUT` carries `PointerFacts { modifiers, clicks }`
(`nexus_display_proto::surface_input`, `MOD_*` bits — one definition for inputd's visible state,
which now carries the modifiers held, and the surface frame); a tap carries its press's facts,
the other kinds none. windowd counts clicks where it sees every press (`windowd::click_count`:
500 ms, 4 px, three at most). The frame's length changed with it (all clients are built from the
one codec — no older layout is in the field). app-host turns a press on the focused field into
the caret where it landed (the painter's glyph metrics), Shift+click and a drag into a
selection, a double click into the word, a triple click into everything.

### 5. The caret is the compositor's (normative; ADR-0090)

An app declares the caret of its focused field: `rect` (surface pixels, inside the surface),
`color`, `blink` (on / off) and `reset` (restart the phase — sent on every edit). windowd draws it
as a layer of the surface (RFC-0109's washes carry it — no app raster, no app wake), blinks it with
a fixed period, holds it solid for one period after a reset, and stops it while the surface is
neither focused nor visible. A caret move is one small frame from the app, not a repaint. The
selection highlight stays in the field's own rows of the app raster (behind the glyphs).

As built (Phase 2): `OP_SURFACE_CARET` (37, `nexus_display_proto::surface_caret`): surface,
`x y w h` (u16), BGRA, flags `VISIBLE | BLINK | RESET`; a caret is thin (`w` ≤ 8, `h` ≤ 256) or
refused. windowd draws it as ONE solid layer — a 16×16 atlas patch of its colour spread over the
rectangle, the window's transform applied — only while its surface holds window focus and is
visible, clipped to the surface (a caret outside its surface is refused, `windowd::caret_rule`).
The blink: 530 ms halves, solid first after a reset (a move resets it too), resting solid ten
seconds after the last one — a caret nobody types at costs no wake. windowd's one-shot timer
(declared slots `0x44/0x45`) is armed only while a caret blinks; without a timer the caret stands
steady. app-host declares it after a present or a motion when it changed (`interaction/
text_caret.rs`) — hidden while a selection shows, without focus, or when the field scrolled out
of its view — and paints no caret into its raster. The trust level is `OP_SURFACE_TEXT_FOCUS`'s:
windowd's server endpoint names no app sender, so a wrong surface id draws a thin bar in the
focused surface and nothing more; an unknown surface is refused. The caret's layer counts in its
surface's share of the frame's layers and is set aside before the surface spends it
(`LayerQuota::set_aside`): a tight frame drops the surface's own last layer, never the caret (the
first board cycle drew none — the share ended at the declared layers).

### 6. Measurement (normative)

- Every hop on the key path keeps counters per stream: received, delivered, backpressured, lost
  (by reason). They leave the component as one line per window of activity (never per event).
- The field keeps, per focus session, the key→present and click→caret latencies (from `t_ns` to
  the present that showed them) and prints one aggregate when the session ends — never for a
  secure field. As built (`nexus_ux_budgets::session`):
  `apphost: input session (chars= actions= timed=) key_present <verdict> (p50_us= p99_us= max_us=)`
  — the counts first (a lane reads them whole), the verdict the p99 against RFC-0108's input
  budget (`ok|warn|FAIL`, `untimed` when no key carried a stamp — an input method's commit does
  not). The session ends at the first present after the focus left (the key that moved it is
  timed by the frame that shows it), or before the next key. The field cannot know a loss: a loss
  is a `FAIL` on the hop that lost it. The click→caret latency joins with Phase 2's caret.
- A gap, a failed send and a key windowd could not hand on are `FAIL` lines naming the hop and the
  count, and fail a lane and a board run (`scripts/qemu-test.sh`, `scripts/board-test.sh`).
  Backpressure is no loss: a sender that holds is the contract.
- **No keystroke rhythm in any line:** the per-second rate lines of xhcid (`reports hz`), hidrawd
  (`wake/rx/ev/tx hz`, the `fps:` dump) and inputd (`hid rx hz`) count POINTER traffic only — a
  keyboard's batches per second are the length and rhythm of what was typed, a password's too.
- Budgets come from Phase 0's measurement (RFC-0108's input→photon 50 ms the outer bound).

### 7. The shortcut registry (normative)

One table (`userspace/shortcuts`) — `id`, `category`, `action` (its `owner`, the component that
acts, follows from it), `chords` (one or more, each an exact modifier set and one key; the first
is the primary, the others act the same), `active` — compiled into the components that need it.

- **Matching.** A chord matches only with exactly its modifiers (Shift, Ctrl, Alt, AltGr, Super —
  the surface wire's bits); at most one entry matches a press. A key is named by its place (HID
  usage: arrows, Home, Print) or by the letter the active layout types on it (Ctrl+Z is the key
  that types `z`; a layout without a Latin letter there takes the key's US letter). inputd
  matches the window and capture chords before a key enters the text route (events on the key
  channel, §3 — TASK-0348 P17 replaced the visible state's one-shot facts); the keymap matches
  the field commands, the focus moves (Tab, Shift+Tab: `text.focusNext`, `text.focusPrevious`,
  2026-11-03) and the input-source switch. Plain keys a layout produces (Enter, Backspace,
  Escape) are layout data, not registry entries.
- **`active`.** Whether an owner acts on the match. A matched chord is consumed either way (it
  types nothing); an inactive one is listed dimmed. Every window, capture and field entry is
  active; the input-source switch (Ctrl+Space) is matched but not yet acted on.
- **Listing.** The settings app lists it by category in its keyboard-shortcuts window (DSL,
  app-modal over the settings window; `svc.shortcuts.list(category, locale)`, in process): the
  chords named for the language, the names from the app's language files by `id` (`sc.<id>`).
  Rebinding is drawn, disabled.

### 8. Delivery classes

- `frame` (the default): one wake per frame; a pointer batch with its history.
- `stream` (declared): a capability an app names in its manifest and policyd grants; refused
  until its phase lands. The format is the same — only the wake policy differs.

As built (Phase 4): the `frame` class is §2's batch on every MOVE and DRAG. The `stream` class is
the permission `nexus.permission.INPUT_STREAM`: nxb-pack refuses it for every bundle type
(`test_reject_stream_class_without_a_grant`) and policyd does not recognize it, so a manifest
that names it never publishes (`test_reject_stream_class_at_publish`).

### Phases / milestones

- **Phase 0** — identity + accounting + aggregates; a lane that types faster than a person proves
  the counters (it is red where today's chain drops).
- **Phase 1** — the reliable key channel, the source route, windowd's focus order, auto-repeat:
  the same lane green with zero lost; the board's typing rung.
- **Phase 2** — the caret layer, pointer placement, selection, word editing, undo/redo.
- **Phase 3** — the registry, inputd matching from it, the settings page.
- **Phase 4** — the `frame` class with history in the format; `stream` refused.

## Security considerations

- **Threat model:** a process injecting keys or text into another's field; a process reading
  another's keystrokes; timing side channels in logs; an app painting a caret outside its surface.
- **Mitigations:** sender identity on every key-path hop; text only to windowd's focused field;
  aggregates only, secure fields unmeasured; the caret rect clipped and refused outside its
  surface; the `stream` class a policyd capability.
- **Negative tests:** `test_reject_text_for_an_unfocused_surface`, `test_reject_key_from_a_stranger`,
  `test_reject_imed_commit_from_a_stranger`, `test_reject_caret_outside_its_surface`,
  `test_reject_stream_class_without_a_grant`, `test_reject_a_shortcut_with_extra_modifiers`,
  `test_reject_an_oversized_commit`.
- **As built (Phases 0–1):** windowd `tests/key_gate.rs` (`test_reject_key_from_a_stranger`,
  `test_reject_text_for_an_unfocused_surface`, `test_reject_imed_commit_from_a_stranger`,
  `test_reject_text_to_a_window_without_a_field`); the key channel's wire
  (`test_reject_an_empty_or_oversized_burst`, `test_reject_a_frame_of_the_wrong_length_or_count`,
  `test_reject_an_unknown_kind_flag_or_a_non_scalar`); the session
  (`test_reject_a_summary_of_a_secure_field`,
  `test_reject_ok_for_a_session_without_a_timed_key_or_past_the_budget`); xhcid
  (`test_reject_a_keyboard_frame_past_the_held_places_is_counted_as_keys_lost`,
  `test_reject_a_keyboard_report_in_the_rate_window`); hidrawd
  (`test_reject_a_key_code_outside_the_table`); the scenario schema
  (`test_reject_a_pace_without_typing_or_out_of_range`).
- **As built (Phase 3):** the registry's `tests/registry.rs`
  (`test_reject_a_shortcut_with_extra_modifiers`, `test_reject_a_shortcut_with_a_missing_modifier`);
  the keymap (`test_reject_field_chords_under_alt_or_super`); inputd `tests/key_facts.rs`
  (`test_reject_a_chord_with_extra_modifiers`); windowd pins the registry's window codes to its
  zone codes (`registry_window_codes_are_the_zone_codes`).
- **As built (Phase 4):** the batch (`test_reject_a_malformed_batch`), the surface frame
  (`test_reject_a_motion_frame_of_another_kind_or_with_garbage`), the push
  (`test_reject_a_push_whose_motion_or_length_does_not_fit`); the stream class at pack time
  (`test_reject_stream_class_without_a_grant`) and at publish (`test_reject_stream_class_at_publish`).

## Failure model (normative)

- A receiver that never drains holds its sender — the sender's own clients see backpressure, not
  loss; a dead receiver is an EOF (RFC-0079) and the route is re-resolved by declaration.
- A key whose focus generation is gone is dropped with `focus-gone` and counted.
- No silent fallback: a route that cannot be used is a counted, named failure.

## Proof / validation strategy

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p inputd -p imed -p windowd -p shortcuts -p textedit-core
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just ci-os-usb-visible
```

### Deterministic markers

- `apphost: input session (chars=28 actions=1 timed=29)` — the usb-visible scenario's
  fast-typing step (28 letters at 30 ms a key, then Escape) reads the counts whole: none lost,
  every key timed. (The seed's harness count `SELFTEST: input keys ok (` is not needed: the field's
  own count against the scenario's typed word is the proof, and the harness never sees text.)
- `inputd: key channel to windowd open` — the first key frame windowd's queue took.
- FAIL (fail the lane): `inputd: FAIL input gap (`, `inputd: FAIL key send (`,
  `WINDOWD: FAIL key input (`, `xhcid: FAIL keyboard reports lost (`.

## Alternatives considered

- **Every key through the input method** (a central keyboard service in the path of all text) —
  today's topology; a stall in the input method stalls all typing, and focus travels a second
  path. Rejected: a layout needs no input method.
- **App-first with an input-method stage inside the app** — the app's text system forwards keys to
  the input method. Rejected for this system: windowd owns focus and the text route, so the stage
  sits behind windowd instead, and an app never sees keys that are not for its focused field.
- **The app blinks its caret** (a timer and a repaint per blink) — wakes the app twice a second per
  focused field and repaints rows. Rejected: the compositor composes a layer for free.

## Open questions

- Device timestamps from xhcid's completion versus hidrawd's drain (Phase 0 measures the gap).
- A composition source's key typed in the instant between a click and the app's field-focus
  announcement reaching windowd composes under the window's session (Escape only). The layout
  route has no such window (the app orders tap and text itself); closing it for composition means
  the announcement travels with the tap's answer — measured first, on a composition layout.
- The blink period and the double-click interval (defaults in the registry of settings later).
- The `stream` class's ring (layout, notification) — its own phase.

---

## Implementation Checklist

- [ ] **Phase 0**: identity, counters, session aggregates, the fast-typing lane — proof: TASK-0346 P0
- [ ] **Phase 1**: reliable key channel, source route, focus order, auto-repeat — proof: TASK-0346 P1
- [ ] **Phase 2**: caret layer, placement, selection, word editing, undo — proof: TASK-0346 P2
- [ ] **Phase 3**: shortcut registry + settings page — proof: TASK-0346 P3
- [ ] **Phase 4**: `frame` history, `stream` refused — proof: TASK-0346 P4
- [ ] Task linked with stop conditions + proof commands.
- [ ] QEMU markers in `scripts/qemu-test.sh`.
- [ ] Security negative tests (`test_reject_*`) exist.
