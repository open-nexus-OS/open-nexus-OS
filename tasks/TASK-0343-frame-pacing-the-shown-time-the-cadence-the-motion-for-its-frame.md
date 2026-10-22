---
title: TASK-0343 Frame pacing — when a frame reaches the glass, the cadence the driver holds, the motion computed for the time a frame is shown; typing's frames without pixels; the underflows timed
status: In Progress (board cycle 62 2026-10-22 — the operator follows the scroll, typing's frames carry pixels, the underflows timed; open: the prediction lands early — 62–72 % on time while scrolling against the 90 % line)
owner: @ui @gpu
created: 2026-10-22
depends-on: [TASK-0342 (the frame chain: flips through the latch, the hops)]
follow-up-tasks: [RFC-0110 Phase 1 (the frame budget judges a paced frame against its cadence's step); TASK-0335 P4's rest (the blur's dual filter); layer 8 (planes for what moves); the input's own stamp from the device (RFC-0108 Phase 1); the legacy proof cards' scene (`systemui_shell` hover/click/keyboard cards — nothing shows them where the GPU composes; the windowd clean-up map's MOVE)]
links:
  - RFC: docs/rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md
  - RFC: docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md (§5 the present ack, amended)
  - Measurement: docs/board/measurements/2026-10-22-frame-pace/README.md
  - Together with: tasks/TASK-0335-frame-cost-fewer-textured-pixels-per-frame-glass-banded-occlusion.md (P4: a scroll frame computes no corner and no shadow) — one board cycle
  - Before: docs/board/measurements/2026-10-22-frame-chain/README.md (cycle 61)
---

## Context (cycle 61)

With flips through the controller's latch (TASK-0342) the board composes ~20 frames a second
while something moves; the operator: "a quantum leap — scrolling is fast, fluid, but I cannot
follow the messages with my eyes; now and then a flicker, not repeatable; the keyboard input was
slower". After the commit: "the stock system scrolls cleanly, even in the browser — so it is
possible", and "when scrolling no rounded corners need to be computed, only the scroll content".
Measured in cycle 61:

- A frame costs the GPU 25 ms on the median against a refresh of 16.7 ms: frames land one and two
  periods apart in turn, and each shows the scroll offset of the moment the app ran, 40–80 ms
  before it is shown, by a varying amount — the content moves in uneven steps (judder).
- While typing windowd sends 10–21 frames a second that change no pixel (`frames(blit=)`,
  `damage(queued_px=0)`), each a frame in the ring the keystroke's own frame may queue behind.
  Named for this package: every key press and release flips inputd's `keyboard_visible`, which
  started a spring on the keyboard proof card — ~0.4 s of ticks, each a blit of the legacy proof
  panel — and nothing shows that card where the GPU composes.
- 8 scan-out underflows in the session (`underflows=8`), the flicker's suspect; when they come is
  not known.
- The scroll frame's cost (TASK-0335 P4, same cycle): ~17 ms of GPU for 0.95 Mpx of damage — the
  glass's cached backdrop drawn with the rounded program over the whole viewport (8.69 ns a pixel
  against the opaque program's 1.34), a shadow program run inside the window where its knockout
  leaves nothing, and 2.5 ms of CPU a frame cleaning the whole viewport's atlas rows again.

## Goal

The motion on the glass moves by the time between the frames shown and the frames come evenly
apart — the operator follows a scrolled message with the eye; typing's frames carry pixels only;
the underflows timed.

## Non-Goals

- A frame's GPU cost (TASK-0335 — its P4 cuts ride this board cycle), planes (layer 8).
- A presentation time a client requests; the compositor alone paces.
- The proof cards' scene deleted (follow-up above): this task stops their springs.

## Architecture verdict (three lenses)

- **Scope** — in: `source/libs/nexus-display-proto` (`frame_time.rs`: the ack's tail, the
  pulse's tail; the pulse's frames moved there from `client_surface.rs`), `source/drivers/gpud`
  (`backend/frame_pace.rs`: the scan clock, the cadence, the hold, the landing's time;
  `dc/flip.rs`, `dc/present.rs`, `display.rs`, `reply.rs`, `service.rs`; the underflow line),
  `source/services/windowd` (`frame_clock.rs`, `runtime/frame_pulse.rs` out of `app_window.rs`,
  `runtime/gpud.rs`, `loop_telemetry.rs`; the proof cards' springs in `runtime/input.rs`),
  `source/services/app-host` (`probe/scroll.rs`, `probe/anim.rs`, `probe/mod.rs`), RFC-0110,
  RFC-0093 §5, the proof manifest, docs. Out: the GPU programs (TASK-0335), the input's device
  stamp, windowd's compose time in the cadence (RFC-0110's open question).
- **Invariant** — a timing tail decodes only with a sane clock
  (`test_reject_a_plain_ack_or_a_nonsense_clock_as_a_frame_time`,
  `test_reject_a_plain_pulse_or_a_step_no_display_has_as_a_frame_time`); a frame is never held at
  cadence one (`test_reject_a_hold_at_cadence_one`) nor past its slot; a landing is judged once,
  never without its prediction (`test_reject_judging_a_landing_without_a_prediction`); a client's
  and windowd's physics never step back in time; the wire carries times and counts only.
- **Contract** — RFC-0110 (new: the ack's tail, the cadence, the timeline, the pulse's tail),
  RFC-0093 §5 (the present ack's row amended).

## Plan

1. **The shown time (gpud).** A landing is timed at the frame boundary its flip was taken at:
   the scan line counter at the landing (`now − line × line_ns`; the mode's totals at its pixel
   clock: 16 666 666 ns a frame, 14 814 ns a line at 1080p60), moved back to the first boundary
   after the latch was written when the landing is found late. The ack carries it with the period
   and the cadence; a present acked at once (QEMU's virtio GPU, before the flip interrupt) names
   the time it completed.
2. **The cadence (gpud).** The fewest periods the second longest of the last eight draws fits
   with 2 ms to spare (at most three); a frame drawn more than a period before its slot is held,
   the flip interrupt armed, and written at the first wake from which the next boundary is the
   slot. `gpud: dc flips (… cadence= paced= late=)`.
3. **The timeline and the pulse (windowd).** The last timing gpud named; the next present's
   landing predicted (the last landing plus a step per present in flight and one; the first
   boundary half a period ahead after an idle display), kept per present and judged at its ack
   (`windowd: frame pace (on_time= late= early= cadence= period_us=)`). The frame pulse names the
   prediction; windowd's transitions step to it.
4. **The physics at the frame's time (app-host).** On a timed pulse the scroll layers and the DSL
   animations advance to its time, never back.
5. **Typing's frames without pixels.** The click and keyboard proof cards take no spring.
6. **The underflows timed.** `gpud: dc underflow (n= at_ms= line= taken= flips=)`, the first 16.
- **Proof** — host tests; `just check`, `just diag`; QEMU `headless` and `visible`; board cycle 62
  with the operator's series (scroll chat and settings, type a sentence, drag a window) read into
  the measurement folder against its decision lines.

## Results

- **Built 2026-10-22 (board cycle 62 pending).** Host: `nexus-display-proto` 83 (4 new:
  `frame_time`), gpud 48 (6 new: `frame_pace`), windowd 218 (4 new: `frame_clock`), app-host and
  nexus-gfx green; `just check` and `just diag` green (the ratchet's baselines shrank:
  `client_surface.rs` 727 → 699, `app_window.rs` 781 → 708); QEMU `headless` and `visible` green
  — windowd's timeline runs on QEMU's timed acks (`windowd: frame pace (` in both lanes, late or
  early as the emulation runs; the lanes' FAIL lines are the known ones of the run before).
- **Board cycle 62 2026-10-22** (`docs/board/measurements/2026-10-22-frame-pace/README.md`). The
  operator: "everything great except the keyboard and hangs" — the scroll is followed with the
  eye (H6). While scrolling gpud holds `cadence=1` and `gpud: ux frame ok (dropped=0 of=32)` in
  every scroll window; typing's windows show `frames(blit=0)` (cycle 61: 10–21); 2 underflows
  (cycle 61: 8), both in the blanking lines right after a frame boundary, the second with a flip
  taken there; `front=0` throughout. **Open:** `windowd: frame pace` reads 62–72 % on time while
  scrolling (the session 55 %), the misses mostly *early* — a frame shown a period before the
  prediction; to measure next: whether windowd counts a present in flight whose landing gpud has
  already reported.
- **The keyboard and the hangs are not this task's** — measured in the same cycle: the DSL
  runtime's program reader spends one capnp traversal budget over an app's whole life
  (`Malformed` after 28 menu toggles in settings, 482 keystrokes in the greeter; linear in the
  budget), and a failed emit leaves its scene to the arena's next reset (the chat's crash in
  `focus::subtree_is_secure`). The next package.
