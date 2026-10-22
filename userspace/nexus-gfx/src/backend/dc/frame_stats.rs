// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What a present costs on the controller path, by the kind of command that spent
//! it — the window `gpud: dc present us (…)` prints — and what it asked for, by kind: the
//! window `gpud: dc present mix (…)` (layers, glass and shadowed layers, copies, blurs and their
//! pixels — the GPU frame's inputs, TASK-0215 5c-3). One accumulator, alloc-free; the driver
//! feeds it each command with the time around it and the time around the cache clean, and
//! whether the frame was drawn into the plane the controller was scanning (the tear witness:
//! every such present shows its intermediate states on the monitor).
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 unit tests (this file)
//!
//! INVARIANTS: a window is sums over its presents, never a line per present or per command;
//!   it closes after [`FrameStats::WINDOW`] presents and starts empty.
//!
//! ADR: docs/adr/0028-nexusgfx-architecture.md

use core::fmt;

use crate::command::buffer::Command;

/// The kind of work a command is, for the cost split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// A backdrop blur on its own (`BlurBackdrop`).
    Blur,
    /// A composited layer with its own shadow and backdrop blur (`CompositeLayer`).
    Layer,
    /// A drop shadow on its own (`DropShadow`).
    Shadow,
    /// A copy between planes or atlas rows.
    Blit,
    /// Fills: tiles, rounded rects, gradients.
    Fill,
    /// Everything that draws nothing.
    Other,
}

impl Stage {
    /// Number of stages.
    pub const COUNT: usize = 6;
    const NAMES: [&'static str; Self::COUNT] = ["blur", "layer", "shadow", "blit", "fill", "other"];

    /// The stage of `command`.
    pub fn of(command: &Command) -> Self {
        match command {
            Command::BlurBackdrop { .. } => Self::Blur,
            Command::CompositeLayer { .. } => Self::Layer,
            Command::DropShadow { .. } => Self::Shadow,
            Command::BlitSurface { .. } | Command::BlitAbsolute { .. } => Self::Blit,
            Command::DrawTiles { .. }
            | Command::FillSdfRoundedRect { .. }
            | Command::FillSdfGradient { .. } => Self::Fill,
            _ => Self::Other,
        }
    }
}

/// One closed window: averages per present in microseconds, the slowest present, and how
/// many of the window's presents were drawn into the scanned plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub presents: u32,
    pub avg_us: u32,
    pub max_us: u32,
    pub clean_us: u32,
    pub stage_us: [u32; Stage::COUNT],
    /// Average damaged pixels per present.
    pub damage_px: u32,
    /// Presents drawn into the plane the controller was scanning.
    pub front: u32,
    /// What the window's presents asked for, summed (the mix line divides by `presents`).
    pub mix: Mix,
}

/// What presents asked for, by kind: commands and the pixels they cover.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mix {
    /// Composited layers; those with a backdrop blur (glass) and those with a shadow.
    pub layers: u32,
    pub layer_px: u64,
    pub glass: u32,
    pub glass_px: u64,
    pub shadowed: u32,
    /// Copies between planes or atlas rows, and blurs on their own.
    pub blits: u32,
    pub blit_px: u64,
    pub blurs: u32,
    pub blur_px: u64,
}

impl Mix {
    fn add(&mut self, command: &Command) {
        let px = |w: u32, h: u32| u64::from(w) * u64::from(h);
        match command {
            Command::CompositeLayer { width, height, shadow_blur, backdrop_blur, .. } => {
                self.layers += 1;
                self.layer_px += px(*width, *height);
                if *backdrop_blur > 0 {
                    self.glass += 1;
                    self.glass_px += px(*width, *height);
                }
                self.shadowed += u32::from(*shadow_blur > 0);
            }
            Command::BlitSurface { width, height, .. }
            | Command::BlitAbsolute { width, height, .. } => {
                self.blits += 1;
                self.blit_px += px(*width, *height);
            }
            Command::BlurBackdrop { rect, .. } => {
                self.blurs += 1;
                self.blur_px += px(rect.width, rect.height);
            }
            _ => {}
        }
    }
}

/// The window's mix line: counts per present to a tenth, pixels per present.
pub struct MixLine<'a>(&'a Window);

impl fmt::Display for MixLine<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (n, m) = (u64::from(self.0.presents.max(1)), &self.0.mix);
        let count = |c: u32| {
            let tenths = u64::from(c) * 10 / n;
            (tenths / 10, tenths % 10)
        };
        let (layers, glass, shadowed, blits, blurs) =
            (count(m.layers), count(m.glass), count(m.shadowed), count(m.blits), count(m.blurs));
        write!(
            f,
            "gpud: dc present mix (n={} layers={}.{} layer_px={} glass={}.{} glass_px={} \
             shadowed={}.{} blits={}.{} blit_px={} blurs={}.{} blur_px={})",
            self.0.presents,
            layers.0,
            layers.1,
            m.layer_px / n,
            glass.0,
            glass.1,
            m.glass_px / n,
            shadowed.0,
            shadowed.1,
            blits.0,
            blits.1,
            m.blit_px / n,
            blurs.0,
            blurs.1,
            m.blur_px / n
        )
    }
}

impl Window {
    /// The window's `gpud: dc present mix (…)` line.
    pub fn mix(&self) -> MixLine<'_> {
        MixLine(self)
    }
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "gpud: dc present us (n={} avg={} max={} clean={}",
            self.presents, self.avg_us, self.max_us, self.clean_us
        )?;
        for (name, us) in Stage::NAMES.iter().zip(self.stage_us) {
            write!(f, " {name}={us}")?;
        }
        write!(f, " damage_px={} front={})", self.damage_px, self.front)
    }
}

/// The accumulator of the current window.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    presents: u32,
    total_ns: u64,
    max_ns: u64,
    clean_ns: u64,
    stage_ns: [u64; Stage::COUNT],
    damage_px: u64,
    front: u32,
    mix: Mix,
}

impl FrameStats {
    /// Presents per window. The controller path runs at ten to thirty presents a second
    /// (measured 2026-10-06 … 2026-10-10), so a window is one to three seconds.
    pub const WINDOW: u32 = 32;

    /// One command of `stage` took `ns`.
    pub fn command(&mut self, stage: Stage, ns: u64) {
        let slot = &mut self.stage_ns[stage as usize];
        *slot = slot.saturating_add(ns);
    }

    /// One command a present asked for, into the mix.
    pub fn count(&mut self, command: &Command) {
        self.mix.add(command);
    }

    /// One present is out: its commands took `exec_ns`, its cache clean `clean_ns`, over
    /// `damage_px` pixels; `front` if it was drawn into the scanned plane. The window, once
    /// it is full.
    pub fn present(
        &mut self,
        exec_ns: u64,
        clean_ns: u64,
        damage_px: u64,
        front: bool,
    ) -> Option<Window> {
        let total = exec_ns.saturating_add(clean_ns);
        self.presents += 1;
        self.total_ns = self.total_ns.saturating_add(total);
        self.max_ns = self.max_ns.max(total);
        self.clean_ns = self.clean_ns.saturating_add(clean_ns);
        self.damage_px = self.damage_px.saturating_add(damage_px);
        self.front += u32::from(front);
        if self.presents < Self::WINDOW {
            return None;
        }
        let n = u64::from(self.presents);
        let avg_us = |ns: u64| (ns / n / 1000).min(u64::from(u32::MAX)) as u32;
        let window = Window {
            presents: self.presents,
            avg_us: avg_us(self.total_ns),
            max_us: (self.max_ns / 1000).min(u64::from(u32::MAX)) as u32,
            clean_us: avg_us(self.clean_ns),
            stage_us: self.stage_ns.map(avg_us),
            damage_px: (self.damage_px / n).min(u64::from(u32::MAX)) as u32,
            front: self.front,
            mix: self.mix,
        };
        *self = Self::default();
        Some(window)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::core::types::TileRect;

    #[test]
    fn a_window_closes_after_its_presents_and_starts_empty() {
        let mut stats = FrameStats::default();
        for _ in 1..FrameStats::WINDOW {
            stats.command(Stage::Layer, 60_000_000);
            stats.command(Stage::Blur, 20_000_000);
            assert_eq!(stats.present(90_000_000, 1_000_000, 1920 * 1080, true), None);
        }
        stats.command(Stage::Layer, 60_000_000);
        stats.command(Stage::Blur, 20_000_000);
        let window = stats.present(300_000_000, 1_000_000, 1920 * 1080, false).unwrap();
        assert_eq!(window.presents, FrameStats::WINDOW);
        assert_eq!(window.max_us, 301_000);
        assert_eq!(window.clean_us, 1_000);
        assert_eq!(window.stage_us, [20_000, 60_000, 0, 0, 0, 0]);
        assert_eq!(window.damage_px, 1920 * 1080);
        assert_eq!(window.front, FrameStats::WINDOW - 1, "one present was drawn off-screen");
        assert_eq!(stats.present(1, 1, 1, false), None, "the next window starts at one");
    }

    #[test]
    fn the_line_names_every_stage_once() {
        let window = Window {
            presents: 32,
            avg_us: 100_364,
            max_us: 325_535,
            clean_us: 6_706,
            stage_us: [1, 2, 3, 4, 5, 6],
            damage_px: 2_073_600,
            front: 32,
            mix: Mix::default(),
        };
        assert_eq!(
            alloc::format!("{window}"),
            "gpud: dc present us (n=32 avg=100364 max=325535 clean=6706 blur=1 layer=2 \
             shadow=3 blit=4 fill=5 other=6 damage_px=2073600 front=32)"
        );
    }

    #[test]
    fn a_command_is_counted_under_the_work_it_does() {
        let rect = TileRect { x: 0, y: 0, width: 8, height: 8 };
        assert_eq!(
            Stage::of(&Command::BlurBackdrop { rect, radius: 8, saturation_percent: 100 }),
            Stage::Blur
        );
        assert_eq!(
            Stage::of(&Command::BlitAbsolute {
                src_x: 0,
                src_y_abs: 0,
                dst_x: 0,
                dst_y_abs: 0,
                width: 1,
                height: 1
            }),
            Stage::Blit
        );
        assert_eq!(
            Stage::of(&Command::BlendCursor { x: 0, y: 0, width: 1, height: 1 }),
            Stage::Other
        );
    }

    #[test]
    fn the_mix_counts_what_presents_asked_for() {
        let mut stats = FrameStats::default();
        let rect = TileRect { x: 0, y: 0, width: 100, height: 50 };
        let layer = |w: u32, h: u32, shadow: u32, blur: u32| Command::CompositeLayer {
            src_row_abs: 0,
            src_x: 0,
            width: w,
            height: h,
            dst_x: 0,
            dst_y: 0,
            opacity: 255,
            corner_radius: 0,
            shadow_blur: shadow,
            shadow_offset_y: 0,
            shadow_alpha: 0,
            backdrop_blur: blur,
            content_w: 0,
            content_h: 0,
            scroll_band_top_abs: 0,
            scroll_band_h: 0,
            layer_id: 0,
            content_epoch: 0,
            flags: 0,
            key: 0,
        };
        let mut window = None;
        for _ in 0..FrameStats::WINDOW {
            // Per present: two layers (one glass with a shadow), one copy, one blur.
            stats.count(&layer(200, 100, 0, 0));
            stats.count(&layer(40, 30, 12, 20));
            stats.count(&Command::BlitSurface {
                src_x: 0,
                src_y: 0,
                dst_x: 0,
                dst_y: 0,
                width: 10,
                height: 10,
            });
            stats.count(&Command::BlurBackdrop { rect, radius: 20, saturation_percent: 140 });
            stats.count(&Command::BlendCursor { x: 0, y: 0, width: 1, height: 1 });
            window = stats.present(1, 1, 1, false);
        }
        let window = window.expect("the window closed");
        assert_eq!(window.mix.layers, 2 * FrameStats::WINDOW);
        assert_eq!(
            alloc::format!("{}", window.mix()),
            "gpud: dc present mix (n=32 layers=2.0 layer_px=21200 glass=1.0 glass_px=1200 \
             shadowed=1.0 blits=1.0 blit_px=100 blurs=1.0 blur_px=5000)"
        );
        assert_eq!(stats.present(1, 1, 1, false), None, "the next window starts empty");
    }
}
