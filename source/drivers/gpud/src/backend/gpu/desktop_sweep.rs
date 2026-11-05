// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pass sweep (TASK-0351 P5, ADR-0094's series): the stock stack's sweep of
//! 2026-10-10 (`docs/board/measurements/2026-10-10-gpu-stock-baseline/`, `sweep.py` — one draw
//! a frame at growing sizes; its intercept the frame's fixed cost, 0.64 ms, its slope the
//! hardware's 1.8 ns a pixel) run through OUR frame path at boot, after the gate scene: the base
//! alone, an opaque quad of 16x16 to 960x540 over it with the pass limited to the quad, the
//! opaque and the blended full-screen quad, and the full-screen quad unchanged under a 16x16
//! damage — whether the pass's area reaches the hardware. One line of medians in microseconds,
//! `gpud: gpu pass sweep (…)`, read against the stock's numbers by the cycle's reader: the
//! frame's fixed cost is what a driver owns (the board's hover frame cost 12.7–13.5 ms a pass in
//! every cycle of 2026-11-03 against the stock's 8.3 ms, and its 16x16 frame 0.64 ms).
//! OWNERS: @gpu
//! STATUS: Functional (a measurement; no verdict, no `ok`)
//! API_STABILITY: Internal
//! TEST_COVERAGE: the line on the board (`board-headless`); `median` on the host
//! INVARIANTS: the sweep leaves no layer, no drawn slot and no cleaned row behind — the first
//!   frame redraws every slot whole, as after the gate scene.

use nexus_gfx::command::buffer::{Command, LAYER_OPAQUE};
use nexus_gfx::core::types::TileRect;

use nexus_gfx::backend::types::Rect;

use super::desktop::{Desktop, Refusal};
use super::device_v2::Device;
use super::emit_fmt;
use super::series::LAST;
use crate::gpu_context::FrameTarget;

/// Frames a case runs; the line carries their median.
const FRAMES: usize = 5;
/// The quads' sides (the stock sweep's), widest last.
const QUADS: [(u32, u32); 4] = [(16, 16), (240, 135), (480, 270), (960, 540)];

/// The median of `v` (sorted in place).
pub(super) fn median(v: &mut [u64; FRAMES]) -> u64 {
    v.sort_unstable();
    v[FRAMES / 2]
}

/// A case's median frame in microseconds, and the area and passes its last frame drew.
#[derive(Clone, Copy, Debug)]
struct Case {
    us: u64,
    rect: Rect,
    passes: usize,
}

impl Default for Case {
    fn default() -> Self {
        Self { us: 0, rect: Rect { x: 0, y: 0, width: 0, height: 0 }, passes: 0 }
    }
}

impl core::fmt::Display for Case {
    /// `WxH+X+Y/passes` — the drawn area and the passes of the case's last frame.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let r = &self.rect;
        write!(f, "{}x{}+{}+{}/{}", r.width, r.height, r.x, r.y, self.passes)
    }
}

impl Desktop {
    /// A quad of `(w, h)` centred on the display, sampling the base plane, `opaque` or blended.
    fn quad(&self, target: &FrameTarget<'_>, (w, h): (u32, u32), opaque: bool) -> Command {
        let (dw, dh) = self.display;
        Command::CompositeLayer {
            src_row_abs: target.base_row,
            src_x: 0,
            width: w.min(dw),
            height: h.min(dh),
            dst_x: dw.saturating_sub(w) / 2,
            dst_y: dh.saturating_sub(h) / 2,
            opacity: 255,
            corner_radius: 0,
            shadow_blur: 0,
            shadow_offset_y: 0,
            shadow_alpha: 0,
            backdrop_blur: 0,
            content_w: 0,
            content_h: 0,
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            layer_id: 1,
            content_epoch: 1,
            flags: if opaque { LAYER_OPAQUE } else { 0 },
            key: 1,
            source: 0,
        }
    }

    /// A damage rectangle of `(w, h)` centred on the display.
    fn damage(&self, (w, h): (u32, u32)) -> Command {
        let (dw, dh) = self.display;
        let rect = TileRect {
            x: dw.saturating_sub(w) / 2,
            y: dh.saturating_sub(h) / 2,
            width: w.min(dw),
            height: h.min(dh),
        };
        Command::Damage { rect, origin: 0 }
    }

    /// `FRAMES` frames of `cmds`, every pass waited, none counted: the median frame, and what
    /// the last frame drew (its area and passes — TASK-0351 P12: whether a case's area is the
    /// damage's). `trace` names the case whose last frame's pass is traced (`gpu pass fw`).
    fn case(
        &mut self,
        dev: &mut Device,
        cmds: &[Command],
        target: &FrameTarget<'_>,
        trace: Option<&'static str>,
    ) -> Result<Case, Refusal> {
        let none = super::surface_buffers::SurfaceBuffers::new();
        let mut us = [0u64; FRAMES];
        let mut drawn = (Rect { x: 0, y: 0, width: 0, height: 0 }, 0);
        for (i, u) in us.iter_mut().enumerate() {
            let traced = trace.filter(|_| i + 1 == FRAMES);
            if let Some(of) = traced {
                self.trace_of = of;
            }
            let (rect, t, passes) =
                self.frame_of(dev, cmds, target, (true, traced.is_some(), false), (false, &none))?;
            *u = t;
            drawn = (rect, passes);
        }
        self.trace_of = "hover";
        Ok(Case { us: median(&mut us), rect: drawn.0, passes: drawn.1 })
    }

    /// The sweep, after the gate scene: its line, then the set cleared and every slot forgotten.
    pub(super) fn sweep(
        &mut self,
        dev: &mut Device,
        target: &FrameTarget<'_>,
    ) -> Result<(), Refusal> {
        let run = (|| {
            let whole = self.damage(self.display);
            let base = self.case(dev, &[whole.clone()], target, None)?;
            let mut quads = [Case::default(); 4];
            for (i, (q, side)) in quads.iter_mut().zip(QUADS).enumerate() {
                let trace = (i == 0).then_some("q16");
                *q = self.case(
                    dev,
                    &[self.quad(target, side, true), self.damage(side)],
                    target,
                    trace,
                )?;
            }
            let full = self.case(
                dev,
                &[self.quad(target, self.display, true), whole.clone()],
                target,
                None,
            )?;
            // The full quad unchanged, the damage a pointer's: the pass limited to 16x16.
            let area16 = self.case(
                dev,
                &[self.quad(target, self.display, true), self.damage((16, 16))],
                target,
                Some("area16"),
            )?;
            let blended =
                self.case(dev, &[self.quad(target, self.display, false), whole], target, None)?;
            Ok::<_, Refusal>((base, quads, full, area16, blended))
        })();
        self.frame.layers.clear();
        self.drawn = [0; 3];
        self.cleaned.forget();
        let (base, q, full, area16, blended) = run?;
        let (q16_last, area16_last) = LAST.map_or((0, 0), |l| (l.q16_us, l.area16_us));
        emit_fmt(format_args!(
            "gpud: gpu pass sweep (base_us={} q16_us={} q240_us={} q480_us={} q960_us={} full_us={} area16_us={} blended_us={} frames={FRAMES} last={q16_last}/{area16_last})",
            base.us, q[0].us, q[1].us, q[2].us, q[3].us, full.us, area16.us, blended.us
        ));
        // What each case's last frame drew: its area (the slot's lack and the damage) and its
        // passes — a q16 drawn whole is a measurement of the base, not of the quad.
        emit_fmt(format_args!(
            "gpud: gpu pass sweep drawn (base={} q16={} q240={} q480={} q960={} full={} area16={} blended={})",
            base, q[0], q[1], q[2], q[3], full, area16, blended
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_is_the_middle_frame() {
        assert_eq!(median(&mut [9, 1, 5, 7, 3]), 5);
        assert_eq!(median(&mut [4, 4, 4, 100, 4]), 4);
    }
}
