// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The textured work of a frame window, by pass kind (TASK-0335 P0, RFC-0108): the
//! board's GPU is memory-bound at a few nanoseconds per textured pixel — a full-screen pass
//! took 12 ms in the boot trace, the raw fill 1 ns a pixel — so a frame's cost follows the
//! pixels its passes draw, not their count. `gpud: gpu work (…)` beside `gpu frame us` names
//! the slot's pixels, the glass scenes' and the blurs', the pixels by program (TASK-0335 P4: the
//! rounded and the shadow programs cost 6–8 times the opaque one) and the GPU's time per pixel;
//! every cut of the frame-cost package is judged by it. Split out of `desktop.rs` (structure
//! ratchet).
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `Board::work_px` on the host (`tests/frame_board.rs`), the write causes in
//!   `tests/frame_motion.rs`; the lines on the board

use nexus_gfx::frame::plan::Holds;
use nexus_gfx::frame::{Area, CacheStep, Frame, Planned, Target, Writes, WritesBy, PROGRAM_KINDS};

use super::contexts::Holdings;
use super::emit_fmt;
use super::series::LAST;
use crate::backend::hop_trace::FrameHop;

/// Textured pixels by pass kind: the slot's, the glass scenes' (scratch), the blurs' (half and
/// cache rows); the glass writes by cause (TASK-0336 P0: band, backdrop, new, full, the band
/// writes the base's change reached — and the glasses whose rows owed a hidden change,
/// TASK-0351 P6; the backdrop writes blurred as a band, C-c); and the pixels by program
/// (TASK-0335 P4: opaque, straight, rounded, shadow,
/// blur); and the layers the frames composed (TASK-0351 P0); and the glass writes by the origin
/// class of what caused them (TASK-0351 P6: below, glass, unknown split into zero and stray —
/// P19 W4 — moved, order, rest); and the glasses written by layer key (`(key, writes)`, the first
/// eight keys a window writes); and what the glass hold decided (TASK-0373: held, rest, drift,
/// bound, full; foreign, TASK-0351 P6); and the key misses at rest by what differs (P19 W5:
/// set, own, moved, drew, epoch; of them written as a band, P19 C-r; written whole, by the first
/// condition: base, owe, new, shape — package B 2b; the whole owe by its source: full, lost,
/// owed, displaced, and owed by the rule that made it whole: put_off, lost, placed, displaced —
/// P19-e).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Work(
    [u64; 3],
    [u64; 7],
    [u64; PROGRAM_KINDS],
    u64,
    [u64; 8],
    [u64; 7],
    [(u32, u32); GLASS_KEYS],
    Holds,
    [u64; 18],
);

/// The glass keys a window tells apart (`gpud: glass top`).
const GLASS_KEYS: usize = 8;
/// A window names its most-written glass past this many writes.
const GLASS_TOP_FROM: u32 = 4;
/// The origin classes of `gpud: glass writes by`, in their order.
const CLASSES: [&str; 7] = ["below", "glass", "zero", "stray", "moved", "order", "rest"];

impl Work {
    /// The slot pass's blended pixels by layer kind (`Board::blended_by_kind`, TASK-0351 P14);
    /// a pass into any other target counts nothing and `px` is never asked.
    pub(super) fn note_kinds(&mut self, target: Target, px: impl FnOnce() -> [u64; 8]) {
        if target != Target::Slot {
            return;
        }
        for (acc, n) in self.4.iter_mut().zip(px()) {
            *acc += n;
        }
    }

    /// A pass into `target` drew `px` textured pixels by program (`Board::work_by_program`).
    pub(super) fn note(&mut self, target: Target, px: [u64; PROGRAM_KINDS]) {
        let k = match target {
            Target::Slot => 0,
            // A run's pass draws the scene's layers, as a write's scene pass does.
            Target::Scratch | Target::Run(_) => 1,
            Target::Half | Target::Cache(_) => 2,
        };
        self.0[k] += px.iter().sum::<u64>();
        for (acc, n) in self.2.iter_mut().zip(px) {
            *acc += n;
        }
    }

    /// A frame's plan: its glass writes by cause and by origin class, the layers it composes and
    /// what the hold decided for its glasses.
    pub(super) fn note_plan(&mut self, w: Writes, by: WritesBy, layers: usize, holds: Holds) {
        self.7.add(holds);
        let causes = [w.band, w.backdrop, w.new, w.full, w.band_base, w.hidden, w.backdrop_band];
        for (acc, n) in self.1.iter_mut().zip(causes) {
            *acc += u64::from(n);
        }
        let classes = [by.below, by.glass, by.zero, by.stray, by.moved, by.order, by.rest];
        for (acc, n) in self.5.iter_mut().zip(classes) {
            *acc += u64::from(n);
        }
        let (m, w, o) = (by.misses, by.whole, by.whole.owe_by);
        let counts = [
            m.set,
            m.own,
            m.moved,
            m.drew,
            m.epoch,
            m.band,
            w.base,
            w.owe,
            w.new,
            w.shape,
            o.full,
            o.lost,
            o.owed,
            o.displaced,
            o.owed_put_off,
            o.owed_lost,
            o.owed_placed,
            o.owed_displaced,
        ];
        for (acc, n) in self.8.iter_mut().zip(counts) {
            *acc += u64::from(n);
        }
        self.3 += layers as u64;
    }

    /// A frame wrote the glass of layer `key`.
    pub(super) fn note_glass_key(&mut self, key: u32) {
        if let Some(slot) = self.6.iter_mut().find(|(k, n)| *k == key && *n > 0) {
            slot.1 = slot.1.saturating_add(1);
        } else if let Some(slot) = self.6.iter_mut().find(|(_, n)| *n == 0) {
            *slot = (key, 1);
        }
    }

    /// Folds a frame's work into the window's.
    pub(super) fn add(&mut self, frame: Work) {
        for (acc, w) in self.0.iter_mut().zip(frame.0) {
            *acc += w;
        }
        for (acc, w) in self.1.iter_mut().zip(frame.1) {
            *acc += w;
        }
        for (acc, w) in self.2.iter_mut().zip(frame.2) {
            *acc += w;
        }
        self.3 += frame.3;
        for (acc, w) in self.4.iter_mut().zip(frame.4) {
            *acc += w;
        }
        for (acc, w) in self.5.iter_mut().zip(frame.5) {
            *acc += w;
        }
        for (acc, w) in self.8.iter_mut().zip(frame.8) {
            *acc += w;
        }
        for (key, n) in frame.6.into_iter().filter(|(_, n)| *n > 0) {
            for _ in 0..n {
                self.note_glass_key(key);
            }
        }
        self.7.add(frame.7);
    }

    /// The window's lines: pixels per frame by kind and by program, the GPU's wait per pixel and
    /// the layers per frame; then the bindings the live contexts hold (TASK-0351 P0: the board's
    /// context binds every app surface — `of` is the room their tables have before growing, P2).
    pub(super) fn emit(&self, frames: u64, wait_ns: u64, held: Holdings) {
        let total = self.0.iter().sum::<u64>().max(1);
        let p = self.2;
        emit_fmt(format_args!(
            "gpud: gpu work (slot_px={} scene_px={} blur_px={} ns_per_px={} opaque_px={} straight_px={} rounded_px={} shadow_px={} layers={} last={})",
            self.0[0] / frames,
            self.0[1] / frames,
            self.0[2] / frames,
            wait_ns / total,
            p[0] / frames,
            p[1] / frames,
            p[2] / frames,
            p[3] / frames,
            self.3 / frames,
            LAST.map_or(0, |l| l.ns_per_px)
        ));
        // TASK-0351 P14: which layers the slot pass blends — the pixels the cut aims at.
        let k = self.4;
        emit_fmt(format_args!(
            "gpud: gpu blend kinds (keyless={} body={} region={} slice={} wash={} solid={} bar={} caret={})",
            k[0] / frames,
            k[1] / frames,
            k[2] / frames,
            k[3] / frames,
            k[4] / frames,
            k[5] / frames,
            k[6] / frames,
            k[7] / frames
        ));
        emit_fmt(format_args!(
            "gpud: bindings (in_use={} of={} contexts={})",
            held.bindings, held.binding_room, held.live
        ));
        // TASK-0336 P0: why the glasses were blurred again — the cause each cut aims at.
        let w = self.1;
        emit_fmt(format_args!(
            "gpud: glass writes (band={} backdrop={} new={} full={} band_base={} hidden={} backdrop_band={})",
            w[0], w[1], w[2], w[3], w[4], w[5], w[6]
        ));
        // TASK-0351 P6: what the glass writes came from — an unknown origin, named apart (P19
        // W4): `zero` a change windowd queued without its layer (`windowd: damage zero`),
        // `stray` one of a key no layer carries (a culled or gone layer's); their sum was
        // `unknown`, the share the damage origin aims to bring to 0.
        let b = self.5;
        emit_fmt(format_args!(
            "gpud: glass writes by (below={} glass={} zero={} stray={} moved={} order={} rest={})",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6]
        ));
        // TASK-0373: what the hold decided for the window's glasses — counters, never the rule.
        let h = self.7;
        emit_fmt(format_args!(
            "gpud: glass holds (held={} rest={} drift={} bound={} full={} foreign={})",
            h.held, h.rest, h.drift, h.bound, h.full, h.foreign
        ));
        // TASK-0351 P19 (W5): why a glass's key missed when its hold ended — the first of its
        // prints that differs from its rows' (layers came or went, its own window's moved,
        // another's moved, drawn otherwise, a new wallpaper) — and how many of them were
        // written as the band of what changed below during the hold (C-r); written whole, by
        // the first condition, and a whole owe by its source and the rule that made it whole
        // (P19-e). Counters, never the rule.
        let m = self.8;
        emit_fmt(format_args!(
            "gpud: glass rest misses (set={} own={} moved={} drew={} epoch={} band={} \
             whole={}/{}/{}/{} owe={}/{}/{}/{} owed_by={}/{}/{}/{})",
            m[0],
            m[1],
            m[2],
            m[3],
            m[4],
            m[5],
            m[6],
            m[7],
            m[8],
            m[9],
            m[10],
            m[11],
            m[12],
            m[13],
            m[14],
            m[15],
            m[16],
            m[17]
        ));
        // TASK-0351 P6: the glass written most in a window that wrote more than a few — the
        // glass a transition's halved cadence comes from — and the window's dominant class.
        let writes: u32 = self.6.iter().map(|(_, n)| *n).sum();
        if let Some(&(key, n)) =
            self.6.iter().max_by_key(|(_, n)| *n).filter(|_| writes > GLASS_TOP_FROM)
        {
            let class = (0..CLASSES.len()).max_by_key(|&k| b[k]).map_or("none", |k| CLASSES[k]);
            emit_fmt(format_args!(
                "gpud: glass top (key={key:#x} n={n} of={writes} class={class})"
            ));
        }
    }
}

/// RFC-0108: the window judged — frames past the budget are dropped frames; `last=` the slowest
/// frame of the last recorded board cycle (TASK-0351 P8).
pub(super) fn emit_ux(ux: &nexus_ux_budgets::FrameWindow) {
    emit_fmt(format_args!(
        "gpud: ux frame {} (dropped={} of={} max_us={} budget_us={} last={})",
        ux.verdict().as_str(),
        ux.dropped,
        ux.frames,
        ux.max_us,
        nexus_ux_budgets::FRAME_US,
        LAST.map_or(0, |l| l.ux_max_us)
    ));
}

/// The glasses a frame blurred again (`glass_idx`: by step, of its first `layers`), named with
/// the present's damage and its origins — what touched them (`gpud: gpu glass damage (`,
/// `gpud: gpu glass written (`, K3 — moved here from `desktop.rs`, the module-size ratchet).
pub(super) fn name_glass_writes(frame: &Frame, layers: usize, glass_idx: u32) {
    let (named, count, full) = frame.layers.last_damage();
    let mut line = alloc::string::String::new();
    for (r, o) in named.iter().take(count) {
        use core::fmt::Write as _;
        if let Area::Rect { x0, y0, x1, y1 } = r {
            let _ = write!(line, " {x0},{y0},{}x{}@{o}", x1 - x0, y1 - y0);
        }
    }
    emit_fmt(format_args!("gpud: gpu glass damage (full={full} rects={line})"));
    for i in 0..layers.min(32) {
        if glass_idx & (1 << i) == 0 {
            continue;
        }
        if let Some(l) = frame.step(i).map(|s| s.layer) {
            emit_fmt(format_args!(
                "gpud: gpu glass written (i={i} rect={},{},{}x{} key={} id={} blur={} src_row={} in_motion={})",
                l.dst_x,
                l.dst_y,
                l.width,
                l.height,
                l.key,
                l.layer_id,
                l.backdrop_blur,
                l.src_row_abs,
                u8::from(frame.layers.in_motion(l.layer_id))
            ));
        }
    }
}

/// What a frame hands the display's timeline (TASK-0373): its stamps and waits (`at`), the bytes
/// written into its pass memory, its passes, draws and glass writes (`counts`) — of them the
/// writes of a whole glass (band `Full`) with their pixels (`Glass.at`) — and the writes by
/// origin class. A walk over the plan's steps, no allocation.
pub(super) fn frame_hop(
    frame: &Frame,
    planned: &Planned,
    (at, waits): ([u64; 4], (u32, u64)),
    (passes, draws, gw): (usize, usize, u32),
) -> FrameHop {
    let (mut whole, mut wpx) = (0u32, 0u64);
    for g in (0..planned.layers).filter_map(|i| frame.step(i)?.glass) {
        if matches!(g.cache, CacheStep::Write(_)) && g.band == Area::Full {
            whole += 1;
            wpx += u64::from(g.at.2) * u64::from(g.at.3);
        }
    }
    let b = planned.by;
    FrameHop {
        at,
        waits,
        copied: super::desktop_pass::take_uploaded(),
        passes: u32::try_from(passes).unwrap_or(u32::MAX),
        draws: u32::try_from(draws).unwrap_or(u32::MAX),
        gw,
        whole,
        wpx,
        by: [b.below, b.glass, b.unknown, b.moved, b.order, b.rest],
    }
}
