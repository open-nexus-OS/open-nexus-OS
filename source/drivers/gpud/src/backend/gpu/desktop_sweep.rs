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

    /// `FRAMES` frames of `cmds`, every pass waited, none counted: the median frame.
    fn case(
        &mut self,
        dev: &mut Device,
        cmds: &[Command],
        target: &FrameTarget<'_>,
    ) -> Result<u64, Refusal> {
        let none = super::surface_buffers::SurfaceBuffers::new();
        let mut us = [0u64; FRAMES];
        for u in &mut us {
            let (_, t, _) =
                self.frame_of(dev, cmds, target, (true, false, false), (false, &none))?;
            *u = t;
        }
        Ok(median(&mut us))
    }

    /// The sweep, after the gate scene: its line, then the set cleared and every slot forgotten.
    pub(super) fn sweep(
        &mut self,
        dev: &mut Device,
        target: &FrameTarget<'_>,
    ) -> Result<(), Refusal> {
        let run = (|| {
            let whole = self.damage(self.display);
            let base = self.case(dev, &[whole.clone()], target)?;
            let mut quads = [0u64; 4];
            for (q, side) in quads.iter_mut().zip(QUADS) {
                *q = self.case(dev, &[self.quad(target, side, true), self.damage(side)], target)?;
            }
            let full =
                self.case(dev, &[self.quad(target, self.display, true), whole.clone()], target)?;
            // The full quad unchanged, the damage a pointer's: the pass limited to 16x16.
            let area16 = self.case(
                dev,
                &[self.quad(target, self.display, true), self.damage((16, 16))],
                target,
            )?;
            let blended =
                self.case(dev, &[self.quad(target, self.display, false), whole], target)?;
            Ok::<_, Refusal>((base, quads, full, area16, blended))
        })();
        self.frame.layers.clear();
        self.drawn = [0; 3];
        self.cleaned.forget();
        let (base, q, full, area16, blended) = run?;
        let (q16_last, area16_last) = LAST.map_or((0, 0), |l| (l.q16_us, l.area16_us));
        emit_fmt(format_args!(
            "gpud: gpu pass sweep (base_us={base} q16_us={} q240_us={} q480_us={} q960_us={} full_us={full} area16_us={area16} blended_us={blended} frames={FRAMES} last={q16_last}/{area16_last})",
            q[0], q[1], q[2], q[3]
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
