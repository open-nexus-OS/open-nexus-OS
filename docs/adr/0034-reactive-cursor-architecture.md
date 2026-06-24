# ADR-0034: Reactive cursor architecture — hardware overlay first, decoupled software fallback

- Status: Accepted; Phase 1 (HW overlay on the CPU/mmio scanout) landed, pending QEMU boot-verify;
  Phase 2 (correct SW fallback) + Phase 3 (observability) planned
- Created: 2026-06-16
- Plan: `~/.claude/plans/nested-stargazing-clock.md`
- Builds on: ADR-0028 (windowd present/visible-bootstrap), ADR-0032 (gpud command ring),
  ADR-0033 (soft-real-time spine). Completes the intent of the HW-cursor work (tasks #7/#16).
- Related: `deferred-windowd-present-slowdown`, RFC-0059 (retained-surface display model)

## Context

The live mouse was unusable: the cursor froze up to ~0.3s then jumped. Root cause (run
`manual--2026-06-16T15-38-14`): the cursor was **coupled to windowd's full-frame present**. gpud
always replied `CURSOR_REPLY_SW`, so windowd re-blended the cursor inside its per-present
CommandBuffer — and that CB unconditionally re-renders the glass button + sidebar GPU-blur every
time (`avg_render_us≈35ms`, `max≈288ms`, `present_hz=24`, `damage_px≈1.27M`). A cursor move only
happened when a 35ms present landed; under render-bound load that is a freeze + catch-up jump.

inputd was correct (display-space coords, no calibration rejects) — the fault was purely in how
the cursor was composited.

## Decision

The cursor is a **dedicated top-most layer composited by the display hardware cursor plane,
off the application frame scheduler** — the universal production model: macOS (IOFramebuffer HW
cursor), display-coordinator designs (the coordinator drives the HW cursor plane independently of
frame scheduling), kernel mode-setting (a hardware cursor plane + move-cursor op),
(RenderService pointer layer). For virtio-gpu the equivalent is the **cursor virtqueue**
(`UPDATE_CURSOR` to upload a 64×64 sprite once, `MOVE_CURSOR` to reposition, no response).

Two tiers plus spine integration:

### Tier 1 — Hardware cursor overlay (primary, CPU/mmio scanout) — landed

gpud arms the virtio-gpu HW cursor overlay on `OP_UPLOAD_CURSOR` (`backend.upload_cursor`, already
implemented: 64×64 resource, copy sprite, `transfer_to_host`, `UPDATE_CURSOR` on the cursor queue)
and replies `CURSOR_REPLY_HW`. `OP_MOVE_CURSOR` repositions via `backend.move_hw_cursor`
(`MOVE_CURSOR`, submit-no-response — no scanout re-render, no present, no per-move log). windowd
already handles `CURSOR_REPLY_HW`: it suppresses the software BlendCursor, does a one-time
restore-under, and sends the 9-byte fire-and-forget move per pointer update. The cursor is fully
decoupled from compositing; latency = one tiny IPC + one cursor-queue submit.

The overlay was previously disabled because gpud's *own* software save-under
(`cursor_paint`/`cursor_after_present`) raced windowd's presents (flicker) and flushed per move
(UART/loop storm). The HW overlay avoids both — QEMU composites the plane (no guest save-under)
and `MOVE_CURSOR` carries no response. The one real constraint: `upload_cursor`'s `transfer_to_host`
blanks the **virgl** GL scanout, so Tier 1 is gated to the CPU/mmio scanout
(`#[cfg(not(feature = "virgl"))]`); virgl keeps the software path, and any arm failure falls back
to it too — preserving prior behaviour.

### Tier 2 — Correct software fallback (virgl / no HW plane) — planned

When no HW plane is available, the cursor must still not trigger a full scene present. The fallback
is a **save-under of the final display plane** (not the retained Plane 1 — glass/sidebar are GPU
overlays not baked into Plane 1, so a Plane-1-only blit would corrupt them): cache the display
region under the cursor, restore old + save+blit new on move (O(cursor area)), and invalidate the
cache when a scene present overlaps the cursor rect (this fixes the historical flicker race).

### Phase 3 — Spine integration + observability — planned

The cursor move is a high-priority fire-and-forget side-channel, never a present-scheduler
(waitset) event — the split of the display coordinator from the frame scheduler. Markers
`gpud: hw cursor armed` / `windowd: hw cursor on` (and `windowd: cursor sw blit` on the fallback)
make the active path visible in a headless run; an nx cursor chain hop asserts a cursor-only move
costs ≤ cursor area / triggers no scene present.

## Consequences

- Cursor latency becomes independent of the frame rate / render cost — truly reactive, even while
  windowd's full-frame compose is still render-bound (`deferred-windowd-present-slowdown` / #8).
- The SW BlendCursor path remains intact as a gated fallback (virgl, or HW arm failure), so the
  change is safe: worst case is the prior behaviour.
- The retained-surface incremental-render work (#8) is now decoupled from cursor smoothness — it
  remains valuable for scene updates (hover/click/scroll) but is no longer on the pointer hot path.

## Alternatives considered

- **Keep the software cursor, make it incremental on Plane 1:** rejected — glass/sidebar are
  overlays not in Plane 1; a minimal Plane-1 blit corrupts them. A correct SW cursor needs a
  display-plane save-under (Tier 2), which is strictly more complex than the HW overlay.
- **gpud-side save-under (the previously-disabled path):** rejected as the primary — it raced
  windowd's presents (flicker) and stormed the UART; superseded by the HW overlay.
