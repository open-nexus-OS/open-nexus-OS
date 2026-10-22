---
title: TASK-0342 The frame chain — one display interrupt a frame, hop markers along the input and frame chains, then the reference platforms' levers by measured cost
status: Done (2026-10-22, board cycles 60–61 — steps 1–3 and the hops; steps 4–6 handed to TASK-0335, step 7 to layer 8)
owner: @ui @runtime
created: 2026-10-22
depends-on: [TASK-0341 (surface layers), TASK-0339 (the compositor never waits on a client)]
follow-up-tasks: [frame pacing — a steady cadence and the motion computed for the time a frame is shown (cycle 61: fluid, but the content cannot be followed); the frames that change no pixel (`frames(blit=)` while typing); the scan-out underflows (`underflows=8` in cycle 61) — when they come; TASK-0335 the programs (this task's steps 4–6); layer 8 the planes for what moves (step 7); zero-copy client surfaces (G5, RFC-0101 §7b); the input's own stamp from the device (RFC-0108 Phase 1)]
links:
  - RFC: docs/rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md (§hops, amended)
  - RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§8, amended: the flip through the controller's latch)
  - Measurement: docs/board/measurements/2026-10-22-frame-chain/README.md
  - Before: docs/board/measurements/2026-10-21-gpu-vendor-per-pixel/README.md ("Cycle 59")
---

## Context (cycle 59)

~3 composed frames a second while anything moves (cycles 55–59 alike); the operator: scrolling
slow, typing slow. Cycle 59 measured why: from windowd's send to gpud's credit a frame waits
631 ms (median; worst 2.8 s) while its GPU work is 18–60 ms, and gpud served ~790 blank wakes per
flip, 85 % outside the blank — an interrupt storm. The mask built on the stock word left six
display sources live (bits 5–8, 10, 11) while each wake cleared the vsync bit alone; the level
line stayed up and gpud served it instead of its frames. Typing re-rasterized the whole page per
character (the text commit re-laid out without a span — a full repaint). No line said which stage
of a chain held the time; the operator asked for hop markers along the important chains, and for
the reference platforms' techniques (planes for what moves, the shader's fast paths, the shadow
and the blur as those platforms draw them).

## Goal

Every hop of the input and the frame chain measured on the board; the display interrupt once a
frame; then the levers by measured cost until a frame fits 16.7 ms and an input 50 ms.

## Non-Goals (this task)

- Zero-copy client surfaces (G5) — the copy is measured on windowd's loop line, its own package.
- The collections package; the input's own stamp from the device (RFC-0108 Phase 1).
- New syscalls or wire formats (none needed: the hops are lines, the interrupt words gpud's own).

## Architecture verdict (three lenses)

- **Scope** — steps 1–3 in: `userspace/nexus-gfx/src/backend/dc` (the interrupt words,
  `flip_latched`, `flip_irq_arm`, `irq_clear`), `source/drivers/gpud` (`dc/flip.rs`: the latched
  flip, its interrupt, its census and the frame hops; the ring moved to `backend/flip_ring.rs` so
  its tests run on the host), `source/libs/nexus-ux-budgets` (`Hop`), `source/services/windowd`
  (frame hops, `UxInput::owed_since`), `source/services/app-host` (the input hop; typing's
  bounded repaint), the proof vocabulary, RFC-0108 §hops, RFC-0101 §8, docs. Out: steps 4–7
  (below), each with its own board cycle; the follow-ups above.
- **Invariant** — only the flip's sources (the latch taken, the vsync) may raise gpud's line, and
  only while a flip is latched; a wake clears exactly what it saw raised in the status words and
  nothing writes a raw word (`test_reject_a_source_beyond_the_flip_or_a_write_to_a_raw_word`); a
  present is acked only once the controller took its plane, and a third present in flight is
  refused (`test_reject_a_third_present_in_flight`); a hop line carries times and counts only — no
  identity, no input's content (the keystroke privacy rule: `apphost: hops input` never says what
  was typed).
- **Contract** — RFC-0108 §hops (amended: the line shape); RFC-0101 §8 (amended: the flip through
  the controller's latch, the interrupt words).

## Plan

- **Step 1 — one interrupt a flip** (built in cycle 60 on a wrong reading; rebuilt for cycle 61):
  the vendor kernel's register header names `0x910` an ENABLE word, `0x938` the latched status
  (write-1-to-clear), `0x960` the raw word (read-only) — cycle 60's "mask" turned every source but
  the vsync on and cleared the raw word, the storm went on. Now: the status cleared whole, then the
  latch-taken and vsync sources alone enabled, only while a flip is latched; a wake clears exactly
  what it saw raised in the pipeline's and the output control's status; `gpud: dc flips` names what
  raised the wakes.
- **Step 2 — hop markers** (built): `nexus_ux_budgets::Hop`, one shape (RFC-0108 §hops) — gpud
  `recv>drawn`, `drawn>shown`; windowd `owed>sent`, `sent>acked`; app-host `rx>sent`. With it,
  typing's bounded repaint: a typed character re-renders the rows its edit changed
  (`relayout_bounded`, shared by the tap, the shell push, the text commit and the edit actions).
- **Step 3 — the flip through the latch** (built for cycle 61, pulled forward: cycle 60's
  `drawn>shown` held the frame, and one interrupt a frame would leave gpud — 35 ms in its GPU
  wait — missing the 0.67 ms blank): the plane written with the configuration latch the moment
  the frame is drawn, the controller taking it at its next frame boundary; a flip lands when the
  latch reads clear — at the interrupt's wake or the loop's next pass; the present drawn behind it
  is latched at once.
- **Step 4 — the program's fast paths**: a layer's interior a plain copy, the SDF evaluated at
  the rounded edges alone (55–68 instruction groups a pixel today, ~8 ns/px).
- **Step 5 — the shadow as a ring**: shaded outside the casting box alone (today the knocked-out
  interior is shaded too).
- **Step 6 — the blur's dual filter**: a down/up chain at half and quarter resolution in place
  of the separable Gaussian.
- **Step 7 — controller planes for what moves**: a dragged window on a plane of the display
  controller, the GPU composing the rest once.
- Steps 4–7 handed over (2026-10-22, after cycle 61): the GPU's frame is the pace now (25 ms on
  the median, 13 ns a pixel against the vendor's 1.9–3.7) — steps 4–6 are the frame-cost lane's
  programs (TASK-0335, which owns the passes, the occlusion and the glass band), step 7 is layer
  8's planes for what moves. Not planned twice.

## Proof

- Host: nexus-gfx `the_flip_interrupt_is_armed_as_the_header_names_it`,
  `test_reject_a_source_beyond_the_flip_or_a_write_to_a_raw_word`,
  `a_ring_flip_latches_without_restarting_the_scan`; nexus-ux-budgets
  `a_hop_reports_its_mean_and_worst_and_starts_afresh`; gpud `backend/flip_ring.rs` (latched at
  once, waiting behind and latched when it lands, the hops' times ride to the landing,
  `test_reject_a_third_present_in_flight`, `test_reject_a_landing_with_nothing_latched`); windowd
  `ux_input` (the owed clock keeps the first stamp).
- Board (`docs/board/measurements/2026-10-22-frame-chain/`, hypotheses there with their decision
  lines): cycle 60 read the hops and typing; cycle 61 — a flip one or two wakes, `drawn>shown`
  within two frames, the frame rate the GPU's, no tear.

## Results

- 2026-10-22: steps 1–2 built; host tests green (nexus-ux-budgets, nexus-gfx, gpud, windowd,
  app-host), OS checks clean (gpud with and without virgl), `just check` + `just diag` green, QEMU
  `headless` and `visible` green.
- 2026-10-22, board cycle 60: the hops name the frame's wait — gpud `drawn>shown` 723 ms (median)
  against `recv>drawn` 37 ms, windowd `sent>acked` 777 ms; typing bounded (a keystroke's present
  one rectangle of 88 320 px, not the window). RED: the storm untouched (`wakes=609354` for 256
  flips, the vsync raw at every wake) — the interrupt words were read backwards (vendor header:
  enable, status, raw). Found on the way: gpud's ring lived in the board-only `dc` module, so its
  tests had never run (one of them could not have passed) — moved to `backend/flip_ring.rs`, six
  tests on the host. Step 1 rebuilt with step 3 (above); board cycle 61 next.
- 2026-10-22, board cycle 61: **steps 1–3 hold.** 1.03 wakes a flip (`dc flips (n=2432 …
  wakes=2508 taken=2493 unraised=0)`), never two inside a frame (`dc vblank ok
  (min_period_us=15785`); a drawn frame on the glass 22 ms later on the median (cycle 60: 723 ms);
  windowd presents 20 a second on the median (cycles 55–60: ~3), max 52; `windowd: ux input` ok
  103 / warn 33 / FAIL 17 (cycle 60: 16 of 16 FAIL); no tear (`front=0` throughout, the operator
  saw none). The operator: "a quantum leap — scrolling fast and fluid, but I cannot follow the
  messages; now and then a flicker; typing slower". Measured: the GPU's frame is the pace (25 ms
  median, 33 ms while typing — a glass re-blurred per keystroke), frames land one and two
  refreshes apart (judder), windowd sends frames that change no pixel while typing, and 8
  scan-out underflows are the flicker's suspect. Each is a follow-up above. TASK-0342 Done.
