// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The gate scene (TASK-0215 5c-4, split out of `desktop.rs` under the module-size ratchet):
//! the stock stack's `glass-quarter` — the base plane and a screen-sized glass at radius 40 —
//! drawn by windowd's executor before the compositor's first frame, cold and warm, against the
//! stock's 23.3 ms — and against the last recorded board cycle (TASK-0351 P8): the line carries
//! `last=cold/pipelined/hover@<cycle>` from `series.rs`, and a value worse than the last by
//! more than its margin is the FAIL form naming the cycle (`slower than last: …`). "Slower than before" has a number
//! and a date, never a feeling.

use alloc::string::String;

use nexus_gfx::command::buffer::Command;
use nexus_gfx::frame::layers::ORIGIN_ABOVE;

use super::desktop::{Desktop, Refusal};
use super::device_v2::Device;
use super::emit_fmt;
use super::series::{LAST, MARGIN_PERCENT};
use crate::gpu_context::FrameTarget;

/// The stock stack's bar for its `glass-quarter` scene (the wallpaper and a screen-sized glass
/// blurred at quarter size): 23.3 ms a frame (`docs/board/measurements/2026-10-10-gpu-stock-
/// baseline/`), the gate of TASK-0215 5c-4.
const GATE_BAR_US: u64 = 23_300;
/// The bar for a hover frame (TASK-0215 5d): a pointer-sized change over the cached glass —
/// the stock stack's smallest frame, 8.3 ms (the same folder).
const HOVER_BAR_US: u64 = 8_300;

/// `v` worse than the last cycle's `last` by more than the margin (`0` = never recorded).
pub(super) fn regressed(v: u64, last: u64) -> bool {
    last != 0 && v > last + last * MARGIN_PERCENT / 100
}

/// The names of the gate's values slower than the last cycle's, `cold pipelined hover`.
fn slower(cold_us: u64, piped_us: u64, hover_us: u64) -> String {
    let Some(l) = LAST else { return String::new() };
    let mut out = String::new();
    for (name, v, last) in [
        ("cold", cold_us, l.cold_us),
        ("pipelined", piped_us, l.pipelined_us),
        ("hover", hover_us, l.hover_us),
    ] {
        if regressed(v, last) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(name);
        }
    }
    out
}

impl Desktop {
    /// The gate scene (TASK-0215 5c-4, 5d): the stock stack's `glass-quarter` — the base plane
    /// and a screen-sized glass at radius 40 — drawn into the slot before the compositor's first
    /// frame: cold (the glass blurred and cached) and warm (a pointer-sized change, the glass
    /// read — the hover frame); `gpud: gpu gate scene (…)`, `ok` when the cold frame is below
    /// the stock's 23.3 ms and the best warm frame below its 8.3 ms. The set is cleared
    /// afterwards and every slot redrawn whole by the next frame.
    pub(super) fn gate(
        &mut self,
        dev: &mut Device,
        target: &FrameTarget<'_>,
    ) -> Result<(), Refusal> {
        use nexus_gfx::core::types::TileRect;
        // The gate's scene samples the layout alone: no app buffer.
        let none = super::surface_buffers::SurfaceBuffers::new();
        let (w, h) = self.display;
        let glass = Command::CompositeLayer {
            src_row_abs: target.base_row,
            src_x: 0,
            width: w,
            height: h,
            dst_x: 0,
            dst_y: 0,
            opacity: 255,
            corner_radius: 0,
            shadow_blur: 0,
            shadow_offset_y: 0,
            shadow_alpha: 0,
            backdrop_blur: 40,
            content_w: 0,
            content_h: 0,
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            layer_id: 0,
            content_epoch: 1,
            flags: 0,
            key: 0,
            source: 0,
        };
        let whole = TileRect { x: 0, y: 0, width: w, height: h };
        let cold = [glass, Command::Damage { rect: whole, origin: 0 }];
        let point = TileRect { x: w / 2, y: h / 2, width: 1, height: 1 };
        let warm = [Command::Damage { rect: point, origin: ORIGIN_ABOVE }];
        // The base is cleaned once, for the cold frame; the warm frames (three: the glass read,
        // a pointer-sized change — the frame's floor) find it clean.
        self.pass_witness = [0; 3];
        // TASK-0351 P13: the cold frame draws the slot whole; from then on the slot is the
        // GPU's own and the base clean — a warm frame's pass is limited to its damage, as the
        // desktop's frames are (a target that stayed stale drew every warm frame full-screen:
        // cycle 39's "fragment latency" was the fragment stage shading the whole screen).
        let warm_target = FrameTarget { base_dirty: false, slot_stale: false, ..*target };
        let drawn = self
            .frame_of(dev, &cold, target, (true, false, false), (target.base_dirty, &none))
            .and_then(|(_, cold_us, cold_passes)| {
                let mut warm_us = [0u64; 3];
                let mut warm_passes = 0;
                for (i, w) in warm_us.iter_mut().enumerate() {
                    // The last warm frame's pass is traced: its wait iteration by iteration.
                    let (_, us, passes) = self.frame_of(
                        dev,
                        &warm,
                        &warm_target,
                        (true, i == 2, false),
                        (false, &none),
                    )?;
                    (*w, warm_passes) = (us, passes);
                }
                // The cold frame again, its passes pipelined (the base counted dirty so the
                // glass is blurred again): one wait for four passes.
                let (_, piped_us, _) =
                    self.frame_of(dev, &cold, &warm_target, (true, false, true), (true, &none))?;
                Ok((cold_us, cold_passes, warm_us, warm_passes, piped_us))
            });
        self.frame.layers.clear();
        self.drawn = [0; 3];
        self.cleaned.forget();
        let (cold_us, cold_passes, warm_us, warm_passes, piped_us) = drawn?;
        // The last warm frame's one pass: its wait, the wait's iterations, the interrupts.
        let [wait_us, iters, irqs] = self.pass_witness;
        let hover_us = warm_us.iter().copied().min().unwrap_or(u64::MAX);
        let scene_met = cold_us.min(piped_us) < GATE_BAR_US;
        let hover_met = hover_us < HOVER_BAR_US;
        // TASK-0351 P8: the last recorded cycle beside the values, a regression its FAIL. The
        // line stays under the console's 256 bytes (`DEBUG_LINE_CAP`): the FAIL form carries
        // neither the bars (the marker doc's) nor the wait's iterations (the trace line's).
        let (last, cycle) = LAST
            .map_or(((0, 0, 0), "none"), |l| ((l.cold_us, l.pipelined_us, l.hover_us), l.cycle));
        let slower = slower(cold_us, piped_us, hover_us);
        if scene_met && hover_met && slower.is_empty() {
            emit_fmt(format_args!(
                "gpud: gpu gate scene ok (cold_us={cold_us} pipelined_us={piped_us} hover_us={hover_us} warm_us={}/{}/{} passes={cold_passes}/{warm_passes} wait_us={wait_us} iters={iters} irqs={irqs} bar={GATE_BAR_US}/{HOVER_BAR_US} last={}/{}/{}@{cycle})",
                warm_us[0], warm_us[1], warm_us[2], last.0, last.1, last.2
            ));
        } else {
            let above = match (scene_met, hover_met) {
                (false, false) => "scene and hover above their bars",
                (false, true) => "scene above its bar",
                (true, false) => "hover above its bar",
                (true, true) => "",
            };
            let (and, regression) = match (above.is_empty(), slower.is_empty()) {
                (false, false) => (" and ", "slower than last: "),
                (true, false) => ("", "slower than last: "),
                _ => ("", ""),
            };
            emit_fmt(format_args!(
                "gpud: FAIL gpu gate scene (cold_us={cold_us} pipelined_us={piped_us} hover_us={hover_us} last={}/{}/{}@{cycle}: {above}{and}{regression}{slower}; warm_us={}/{}/{} passes={cold_passes}/{warm_passes} wait_us={wait_us})",
                last.0, last.1, last.2, warm_us[0], warm_us[1], warm_us[2]
            ));
        }
        // TASK-0351 P5: the stock sweep through our path, one line of medians — a measurement
        // whose refusal costs the desktop nothing.
        if let Err(e) = self.sweep(dev, &warm_target) {
            emit_fmt(format_args!("gpud: gpu pass sweep skipped ({e})"));
        }
        Ok(())
    }
}
