<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0110: Frame timing — when a frame reaches the glass, the cadence the driver holds, the motion computed for the time a frame is shown

- Status: In Progress
- Owners: @ui @gpu
- Created: 2026-10-22
- Last Updated: 2026-10-22
- Links:
  - Tasks: `tasks/TASK-0343-frame-pacing-the-shown-time-the-cadence-the-motion-for-its-frame.md`
    (execution + proof)
  - Wire SSOT: `source/libs/nexus-display-proto/src/frame_time.rs`
  - Related RFCs: `docs/rfcs/RFC-0093-display-handoff-and-boot-stage-contract.md` (§5: the
    present ack this extends), `docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md`
    (§8: the flip through the controller's latch, whose landing this times),
    `docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md` (the frame budget,
    the hops), `docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md`
    (the app's scroll physics this paces)

## Status at a Glance

- **Phase 0 (the wire, gpud's shown time and cadence, windowd's timeline, the app's physics at
  the frame's time)**: ⏳ board cycle 62 2026-10-22 (TASK-0343) — the operator follows the
  scroll, gpud holds `cadence=1` while scrolling; open: windowd's prediction, 62–72 % on time
  while scrolling, the misses mostly early
- **Phase 1 (the frame budget's verdict judges paced frames: `gpud: ux frame` against the
  cadence's step)**: ⬜

"Complete" means the board's `windowd: frame pace` reads on time for the operator's scrolling
and the operator follows the content with the eye.

## Scope boundaries (anti-drift)

- **This RFC owns**: the time a present reached the glass on the present ack, the cadence the
  display driver holds, the time a frame pulse names, and what each end does with them.
- **This RFC does NOT own**: the present ack's head and its credit rules (RFC-0093 §5), the flip
  itself (RFC-0101 §8), the frame budget and its lines (RFC-0108), the scroll physics' shape
  (RFC-0109), a frame's GPU cost (TASK-0335).

### Relationship to tasks (single execution truth)

- TASK-0343 proves Phase 0 on the board; Phase 1 follows from the frame-cost lane.

## Context

Board cycle 61 (2026-10-22): with flips through the controller's latch the desktop composes ~20
frames a second while something moves, and the operator reads it as fluid — "but I cannot follow
the messages with my eyes". A frame costs the GPU 25 ms on the median against a refresh of
16.7 ms, so the frames land one and two refresh periods apart in turn, while the app stepped its
scroll by the moments it happened to run — 40–80 ms before its frame was shown, by a varying
amount. The content moves in uneven steps (judder). The reference platforms pace the frames: the
display's driver reports when each frame reached the glass, the compositor names the time the
next frame will be shown, and the animation is computed for that time; when frames cannot come
every refresh, they come on a steady cadence (every second refresh) instead of alternating.

## Goals

- The motion on the glass moves by the time between the frames shown — not by the time between
  the moments a client ran.
- Frames evenly apart on the glass: every refresh while the frames' draws fit one, every second
  (third) while they need more.
- Every prediction judged by the landing it predicted (a line, no verdict word).

## Non-Goals

- A frame's cost (TASK-0335) or planes for what moves (layer 8).
- The input's own timestamp from the device (RFC-0108 Phase 1).
- A presentation time a client requests (the compositor alone paces; a client only reads).

## Constraints / invariants (hard requirements)

- **No fake success**: a time is never invented where a clock exists — the board names the frame
  boundary its flip was taken at, read from the scan line counter; a display without a scan
  clock (QEMU's virtio GPU) names the time its present completed and says nothing more.
- **Bounded**: a timing tail decodes only with a period of 1–100 ms, a cadence of 1–4 and a step
  within; anything else is a plain message (`test_reject_*`).
- **Determinism**: the cadence follows the second longest of the last eight draws (one slow frame
  keeps the pace, two change it); a frame is never held past its slot and never at cadence one.
- **Monotonic**: a client's and windowd's physics never step back in time.
- **No identity on the wire**: times and counts only.

## Proposed design

### Contract / interface (normative)

**The present ack's tail (gpud → windowd; amends RFC-0093 §5).** A present ack is the ack of
RFC-0093 §5 followed by the time its frame reached the glass:

```
[status:u8, seq:u32, at_ns:u64, period_ns:u32, cadence:u8]      (18 bytes, little-endian)
```

- `at_ns` — the frame boundary the present's flip was taken at, on the `nsec` clock (the board:
  the boundary computed from the scan line counter at the landing, `now − line × line_ns`, moved
  back to the first boundary after the latch was written when the landing is found late);
- `period_ns` — the display's refresh period (the mode's totals at its pixel clock);
- `cadence` — the periods between frames on the glass the driver holds (`1..=4`).
- A decoder of the plain ack reads its first five bytes alike; the tail is
  `nexus_display_proto::frame_time::{encode,decode}_present_ack_shown`.

**The cadence (gpud on the board).** `cadence` is the fewest periods the second longest of the
last eight draws (a present received → drawn) fits with a 2 ms margin, at most three. A frame
drawn while its slot (the last landing plus `cadence` periods) is more than a period away is
held — its plane latched in the ring, not yet written — with the flip interrupt armed; the first
wake at a boundary from which the next boundary is the slot writes it. A landing more than half a
period past its slot is counted late.

**The timeline and the pulse (windowd).** windowd keeps the last timing gpud named and predicts
when the present it sends next is shown: the last landing plus one step (`cadence × period`) for
every present in flight and one for itself — or, the display having idled, the first boundary at
least half a period ahead of now. It keeps the prediction of each present it sends and judges the
landing gpud reports for it: on time (within half a period), late, early
(`windowd: frame pace (on_time= late= early= cadence= period_us=)` each second with traffic).
The frame pulse (`OP_SURFACE_FRAME`, the Choreographer contract) names that prediction:

```
[header, surface_id:u32, target_ns:u64, step_ns:u32]     (the plain pulse + 12 bytes)
```

`nexus_display_proto::frame_time::{encode,decode}_surface_frame_at`; the plain pulse stays the
answer before gpud named a timing. windowd's own transitions step to the same prediction.

**The client (app-host).** On a timed pulse the scroll layers' physics and the DSL animations
advance to `target_ns` — never back in time; on a plain pulse, by the client's own clock.

### Phases / milestones (contract-level)

- **Phase 0**: the wire, gpud's shown time and cadence, windowd's timeline and pulse, the client's
  physics at the frame's time — host tests, the QEMU lanes (the timed ack of a display without a
  scan clock), the board (`windowd: frame pace (`, `gpud: dc flips (… cadence= paced= late=)`).
- **Phase 1**: `gpud: ux frame` judges a paced frame against its cadence's step.

## Security considerations

- The tails are bounded and validated (period, cadence, step); a malformed tail is a plain
  message, never a pacer on a nonsense clock. The pulse's time steers only the receiving client's
  own animation; a client clamps a step to 100 ms as before. No identity, no content.

## Failure model (normative)

- No timed ack yet (boot, a display without the interrupt): no prediction, plain pulses, clients
  step by their own clock — the behaviour before this RFC.
- A frame late for its slot: it lands at the next boundary, counted `late=` (gpud) and `late=`
  (windowd); the next prediction starts from that landing.
- A stale timing (the display idled): the prediction moves to the first boundary half a period
  ahead of now.

## Proof / validation strategy (required)

### Proof (Host)

- `nexus-display-proto`: `a_shown_ack_is_the_plain_ack_and_its_frame_time`,
  `test_reject_a_plain_ack_or_a_nonsense_clock_as_a_frame_time`,
  `a_timed_pulse_is_the_plain_pulse_and_the_time_its_frame_is_shown`,
  `test_reject_a_plain_pulse_or_a_step_no_display_has_as_a_frame_time`.
- `gpud` (`backend/frame_pace.rs`): the scan clock of a mode, the cadence's window rule, the hold
  to the boundary before the slot, `test_reject_a_hold_at_cadence_one`, a landing timed at its
  boundary, a late landing counted.
- `windowd` (`frame_clock.rs`): the prediction a step after those in flight, after an idle
  display, each landing judged once, `test_reject_judging_a_landing_without_a_prediction`.

### Proof (OS/QEMU)

- `headless` and `visible` green with the timed ack of a display without a scan clock (QEMU's
  acks name the time a present completed): windowd's timeline predicts and judges in both lanes
  (`windowd: frame pace (`; an emulated lane's landings are not the contract — late or early as
  the emulation runs). The client's side of the timed pulse is proven by the host tests and the
  board.

### Deterministic markers (if applicable)

- Measurement lines, no verdict word: `windowd: frame pace (`, `gpud: dc flips (` (extended),
  `gpud: dc underflow (` (the flicker's suspect, the same package).

## Alternatives considered

- **The client sleeps to the next vsync (a timer)**: a client clock guesses the display's; the
  reference platforms hand the time down the chain instead.
- **windowd holds presents for the cadence**: windowd has no event at the boundaries between
  landings; gpud has the scan's interrupt and the draws' times.
- **Cadence from the last draw alone**: one slow frame (a glass re-blurred) would halve the rate
  for a window; two in eight decide.

## Open questions

- Whether windowd's own compose time should join the cadence's draws (a compositor slower than
  the GPU) — decided when the board's `late=` names it.
