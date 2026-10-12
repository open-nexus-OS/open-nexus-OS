---
title: TASK-0216 Block 4 layer 2 — the scanout contract on the board: a ring of planes flipped at the vertical blank, the frame clock from it, damage per plane, the pointer plane whole at every border (RFC-0101 Phase 1)
status: Done 2026-10-12 (P0–P5; recut the same day from "Compositor v2.2b (OS/QEMU): gpuabst integration + async present queue + plane planner + cursor plane + color space" of 2025-12-27 — the plane planner, the async queue and the colour space are G4's (layer 5) on the GPU; this ledger is the display half, first)
owner: @gpu @ui
created: 2025-12-27
depends-on:
  - TASK-0280
  - TASK-0251
follow-up-tasks:
  - TASK-0329
  - TASK-0215
links:
  - RFC: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§8, Phase 1)
  - RFC (display protocol, reveal): docs/rfcs/RFC-0093-display-protocol-v2-routing-reveal-present-acks.md
  - RFC (compositor boundary): docs/rfcs/RFC-0067-windowd-compositor-boundary.md
  - Order: tasks/IMPLEMENTATION-ORDER.md ("Block 4 in layers", layer 2)
  - Measurements: docs/board/measurements/2026-10-12-scanout-vblank/README.md (P0), docs/board/measurements/2026-10-10-mgs-baseline/README.md (layer 0)
  - Model: userspace/nexus-gfx/src/backend/v2/scanout.rs (the ring, the frame clock, the pointer plane — TASK-0280)
  - Playbook: CLAUDE.md, .claude/skills/driver-bringup/
---

## Context

- Layer 0 measured (2026-10-10): on the board every present is a CPU composite drawn into the
  plane the controller scans (`front` = `n`), 150 ms median; the operator sees windows and
  glass half-drawn, shadows and translucent backgrounds darken with every present (the
  in-place composite blends over its own last result), and the pointer's sprite breaks apart
  in a strip at the right border. `windowd: cursor upload failed` 8 times a boot: the upload
  waits behind a present and takes stale replies for its own.
- Layer 1 (TASK-0280) wrote the contract and its model: `PlaneRing` (a flip takes effect at the
  vertical blank, never before; the scanned slot is never a render target; buffer age),
  `FrameClock`, `PointerPlane` (whole at every border).
- P0 measured the stock system (2026-10-12): the blank is 45 lines (≈ 667 µs) of 1125; the
  controller's online interrupt fires twice a frame, its raw word `0x960` holds the vsync in
  bit 0 (set at line 1080) and two line-1 sources in bits 2–3, the mask `0x910` holds the rest
  back, the driver clears by writing the raw word; the line counter `0x180c8` is readable at
  any time. Whether a plane-address write applies at once or at the blank was not observable
  there — the first board cycle measures it (H2/H3 of the measurement).
- **Must not change**: the display protocol's present ack shape (RFC-0093 §5: matched by seq,
  `STATUS_REVEALED` once), the reveal handshake, the layout (`nexus-display-proto`: plane 2 is
  slot A, plane 3 is slot B — the ring's two planes, as the layout already names them), the
  QEMU paths (virtio 2D and virgl present as before).

## Three lenses (architecture-review)

- **Scope** — in: `source/drivers/gpud/src/backend/dc/**` (the ring over slots A/B, the
  vertical-blank interrupt on gpud's control endpoint, the flip in the blank, the deferred
  present ack = the frame clock, the pending present while no plane is free, the age copy, the
  pointer plane's rectangle), `backend/display.rs` (two hooks), `service.rs` (the wake and the
  deferred ack), `backend/cpu_frame.rs` (an opaque full composite is a copy), `userspace/
  nexus-gfx/src/backend/dc/{regs,model}.rs` (the interrupt words, host-tested), windowd
  `compositor/runtime/{cursor,gpud}.rs` (the upload no longer blocks nor takes stale replies; a
  shape change with the hardware pointer queues no frame) and the deletion of windowd's dead
  slot-B/`current_display_slot` remnants, the proof manifest + `scripts/board-test.sh` (two
  rungs, one FAIL form), this ledger, the measurement folder. Out: the kernel (S2a is
  TASK-0330's package, the same layer), the wire form of damage per plane toward windowd (the
  compositor still sends whole-frame scenes; the age copy is gpud-internal until G4 sends
  damage), the GPU, any CPU blur/layer tuning (G4 deletes that code).
- **Invariant** — nothing is drawn into the plane the controller scans; a flip is written only
  inside the blank; a present is acked only once its flip landed. Proven by `front=0` in every
  present window on the board (the FAIL form `gpud: FAIL dc present front` when it is not), the
  host tests of `PlaneRing` (TASK-0280) and of the interrupt words (`model.rs`).
- **Contract** — RFC-0101 §8 (Phase 1); RFC-0093 §5 unchanged (the ack is the same bytes,
  later); the layout's slot B takes the role its name has carried since the layout was cut.

## Goal

On the board, a present is drawn into the plane the controller is NOT scanning, flipped to at
the next vertical blank by the controller's own interrupt, and acked to windowd when it is on
screen — so no frame is ever seen half-drawn, shadows stay put, and windowd's frame clock is
the display's. The pointer plane is independent of presents: its upload never waits behind one
and never takes a stale reply, a shape change costs no frame, and its sprite is whole at every
border.

## Non-Goals

Frame TIME (150 ms a present stays until G4 deletes the CPU composite); damage per plane on the
wire (G4); the kernel console (TASK-0330 S2a); EDID over DDC (TASK-0251 P2b, G4's P0); the
entry animation's first second (G4).

## Packages

- **P0 — measure** ✅ 2026-10-12: `docs/board/measurements/2026-10-12-scanout-vblank/`, four
  hypotheses with what decides each.
- **P1 — host model** ✅: `regs.rs` gains the pipeline's interrupt mask and raw words and the
  vsync bit as measured; `model.rs` gains `vblank_enable` (the raw word cleared FIRST — a vsync
  raised while masked would fire at once — then every source but the vsync held back),
  `vblank_disable`, `vblank_ack` (the vsync cleared by writing it), `in_blank(line, v_active)`
  (the 45 lines) — with recorder tests; the layout gains slot C (`RING_ROWS`: three planes, as
  RFC-0101 §8 names for the board — one scanned, one queued, one free).
- **P2 — gpud, the ring** ✅: `dc/ring.rs` binds the contract's `PlaneRing` to the three slots;
  `bind_vblank` binds the controller's online interrupt to gpud's control-reply endpoint (the
  pattern of the virtio IRQ and xhcid) and the loop's waitset joins it to the server endpoint;
  a present executes into the ring's free slot (the executor's `display_row` is the slot's),
  cleans it and queues its flip; the interrupt is enabled only while a flip waits (an idle
  display wakes nobody); the wake reads the line counter, writes the flip if inside the blank
  (else defers it to the next blank and counts `late`), acks the blank at the controller and
  the kernel, advances the ring, and the loop sends the present's ack then — the frame clock
  (`Display::present_ack_deferred` / `vblank_wake`). A present drawn while a flip is queued
  waits, drawn, for the next blank; a third finds no free slot and is refused (windowd keeps
  two in flight). A present whose damage is not the whole plane copies the frame on screen
  first (buffer age; every scene today is whole-frame). The reveal stays the one immediate
  switch; without the interrupt the ring flips at the present and acks at once (never torn
  by a frame drawn in view). The readback reads the plane on screen. Markers: `gpud: dc
  vblank armed (irq=)` at the bind, `gpud: dc vblank ok (min_period_us= line= late= n=)` at
  the 32nd served interrupt (the shortest interval between two: a frame when the vsync alone
  is live, a fraction of a millisecond if a line-1 source were too — H1's witness), `gpud: dc
  flip ok (slot= line=)` once; the FAIL forms `gpud: FAIL dc vblank (` and `gpud: FAIL dc
  present front (`.
- **P3 — the pointer plane** ✅: windowd's first upload waits for its own answer, bounded
  (`UPLOAD_EARLY_REPLIES`), every other reply handled as what it is and the answer handled by
  the drain past the bound (`on_cursor_reply`) — the cap of four early replies that produced
  `windowd: cursor upload failed` is gone; later shape changes are 2-byte selects. A shape
  change or a wait-ring step queues no frame while the hardware pointer is on; the channel's
  bounding box ends at the sprite's visible part (`cursor_move`), host-tested against
  `PointerPlane::place` at every border; the register census per upload is gone. (A first cut
  sent the upload and went on without waiting: on the 2D virtio lane the present that followed
  at once ran into the completion deadline — `ci-os-visible-2d` red, then green with the bounded
  wait; the finding is noted for G2's submission model.)
- **P4 — the base is the retained plane** ✅: every board present copies the retained plane
  (windowd's cursor-free scene, kept current by its damage writes) whole into the ring's slot
  before the scene is composited — the frame composed off-screen starts from its base. (A first
  cut made an opaque, effect-free `CompositeLayer` a copy instead of a blend: a layer's source
  may hold transparent pixels, and the 2D lane's screencap probe read black — reverted.)
- **P5 — board cycle + gates** ✅: `gpud: dc vblank armed (` in `board-headless`, `gpud: dc
  vblank ok (` and `gpud: dc flip ok (` in `board-visible`, the two FAIL forms forbidden; one
  cycle (`board--2026-10-12T13-06-02`, image `dev-78566ebe`): **`[PASS] board-visible: 49
  rungs, 7 operator marker(s), no FAIL marker`**; H1–H4 answered in the measurement folder.

## Closure (2026-10-12)

- **Delivered**: the ring over the layout's three slots (`dc/ring.rs`), the vertical-blank
  interrupt as the frame clock (enabled only while a flip waits), the flip inside the blank
  (first flip `slot=1 line=1080`), the deferred ack, the base copy, the pointer plane's bounded
  upload and whole-at-the-border rectangle, the FAIL forms; gpud's `service.rs` split
  (`present_chain.rs`); windowd's dead slot remnants deleted.
- **Proof**: `just check` + `just test-all` EXIT=0 (71 PASS); the board cycle above —
  `front=0` in all eighteen present windows, `windowd: cursor upload failed` 0 (8 before),
  `late=0`; the operator: no tearing, no flicker, shadows stable, the pointer whole at the
  right border, all seven rungs.
- **Two first cuts reverted on the 2D QEMU lane** (recorded in P3/P4): the non-waiting cursor
  upload (the present that followed at once ran into the device's completion deadline — a
  finding for G2's submission model: a command must not be queued behind an unacknowledged
  cursor-queue command without its own completion) and the opaque-layer copy (a layer's source
  may hold transparent pixels).
- **Open, with owner**: the second interrupt 21 µs after a served one (H1 half) — the next
  board cycle (layer 3, TASK-0329 G1's first cycle) counts interrupts served with nothing
  pending and reads the raw word back after the clear write; one wake per flip today, no
  effect on the flips (`late=0`). S2a (the console off the kernel lock) is TASK-0330's package
  of this layer, still open.

## Proof

- Host: `cargo test -p nexus-gfx dc` (the interrupt words, the flip sequence), `v2::scanout`
  (TASK-0280); windowd's and gpud's host tests.
- QEMU: every visible lane unchanged (`just test-all`) — the virtio paths do not take the ring.
- Board: `just board-test board-visible` with `gpud: dc vblank ok (`, `gpud: dc flip ok (` in
  the ladder; no `gpud: FAIL dc present front`; `front=0` in every `gpud: dc present us`
  window; the operator's rungs, with a dragged window and typing watched for tearing.

## Constraints / invariants (hard requirements)

- No polling for the blank: the interrupt wakes the loop; the line counter is read once per
  wake to decide, never spun on. The one bounded read loop that stays is the reveal's latch
  observation (`SCAN_POLL_READS`), which runs once a boot.
- No present is acked before its flip landed; no ack is lost: a flip deferred by a late
  interrupt lands at the next one.
- Bounded: one pending present; the ring's damage list (`MAX_DAMAGE`); the interrupt drained
  with a bounded loop (`DRAIN_MAX`, xhcid's pattern).
- No `unwrap`/`expect`; no `#[allow(dead_code)]` — the remnants are deleted, not allowed.

## Red flags / decision points

- **RED (H2)**: the controller may apply a plane address at once (no shadow latch). Then a
  flip written outside the blank tears between two complete frames; the policy "write only
  inside the blank, else defer" holds either way, and the first cycle's `line=` values say
  whether the handler gets there in time behind a 150 ms present.
- **YELLOW**: gpud is one thread; a present blocks the interrupt's handling for its duration.
  The flip then lands at the next blank after the present — one frame later, never torn.
- **GREEN**: the layout keeps its two slots; windowd's scene stays whole-frame; the QEMU paths
  are untouched.

## Definition of Done

1. P1–P4 in the tree; `just check`, `just test-host`, `just diag`, `just test-all` green.
2. One board cycle: `[PASS] board-visible` with the two rungs, no FAIL form, `front=0`; the
   operator confirms no tear and stable shadows; H1–H4 answered in the measurement folder.
3. RFC-0101 Phase 1 ✅; the order file's layer 2 (this half) ✅; CHANGELOG; this ledger Done. ✅ 2026-10-12
