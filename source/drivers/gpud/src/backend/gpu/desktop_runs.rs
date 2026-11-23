// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The run planes (RFC-0109 "Runs and group opacity", RFC-0101 §8; TASK-0351 P19,
//! TASK-0354 P4). A run is a range of the plan's layers kept drawn in a plane of the desktop's
//! own and drawn from it as one quad (`nexus_gfx::frame`'s `runs`): the wallpaper and the
//! layers below the lowest one in motion, or a fading group at rest. The planner decides; this
//! is where its planes live — `RUN_TARGETS` buffers of the GPU's own, each a display plane and
//! the transparent rows a group's pass clears its rectangle from (a pass's own clear is the
//! bundle's colour; the kernel zeroes a GPU buffer at create and no pass draws past the
//! display's rows, so those rows stay transparent). They are made the first time a layer
//! moves, counted by the ledger like every buffer, and kept while the compositor runs. A
//! plane the device refuses leaves the planner fewer targets: a want that finds none is drawn
//! as layers and named — never a refused frame. No command reaches a plane: a layer's source
//! is a surface buffer checked at intake (`surface_buffers`), bound in the window above these
//! addresses (`crate::surface_va`).
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the planner's rules in `nexus-gfx` `tests/frame_runs.rs` (host); the planes'
//!   addresses and bytes in `crate::surface_va`, `crate::frame_limits` (host); the board's
//!   `gpud: run planes (`, `gpud: runs (` and `gpud: runs short (` lines

use nexus_display_proto::layout::PLANE_ROWS;
use nexus_driverkit::gpu::CacheMode;
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, GfxDevice};
use nexus_gfx::frame::{Board, Frame, Planned, RunKind, RunLimits, RunsShort, RUN_TARGETS};

use super::desktop::Refusal;
use super::desktop_cache::CachePlanes;
use super::desktop_surface::buffer;
use super::device_v2::Device;
use super::emit_fmt;
use super::render_template::{Captured, Target};
use crate::frame_limits::{HALF_ROWS, MAX_DRAWS, PASS_BUDGET_PX, RUN_PLANE_BYTES};
use crate::surface_va::run_plane_va;

/// The run planes: their buffers (held from the moment they exist) and their targets.
pub(super) struct RunPlanes {
    bos: [Option<BoId>; RUN_TARGETS],
    targets: [Option<Target>; RUN_TARGETS],
    /// The planes were asked for (once: memory the device refused does not come by asking).
    asked: bool,
    /// Frames that drew from a run, and frames that left a want short: named at 1, 2, 4, ….
    used: u32,
    short: u32,
}

impl RunPlanes {
    pub(super) const fn new() -> Self {
        Self {
            bos: [None; RUN_TARGETS],
            targets: [None; RUN_TARGETS],
            asked: false,
            used: 0,
            short: 0,
        }
    }

    /// Run plane `slot`'s target.
    pub(super) fn target(&self, slot: u8) -> Option<Target> {
        self.targets.get(usize::from(slot)).copied().flatten()
    }

    /// Plane `slot`: its buffer bound read-write at its address, its target placed.
    fn make(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        (captured, cache): (&Captured, &mut CachePlanes),
        slot: usize,
    ) -> Result<Target, Refusal> {
        let va = run_plane_va(slot as u32).ok_or("the run planes' addresses")?;
        // Asked of the ledger first: a plane past the ceiling is a run not kept, not a fault.
        if dev.ledger.held().saturating_add(RUN_PLANE_BYTES) > dev.ledger.ceiling() {
            return Err("the GPU's memory".into());
        }
        let mode = (RUN_PLANE_BYTES, CacheMode::GpuPrivate);
        buffer(dev, who, mode, va, (&mut self.bos[slot], "a run plane"))?;
        cache.target_at(dev, who, captured, (va, PLANE_ROWS))
    }

    /// The runs of the frame `planned` decided, before its board is made (`area`: what the
    /// slot's pass draws, `None` for no pass): the planes are made the first time a layer
    /// moves, and the planner is told how many there are and the pass's budget.
    pub(super) fn take(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        own: (&Captured, &mut CachePlanes),
        (frame, planned): (&mut Frame, Planned),
        (display, layout_rows): ((u32, u32), u32),
        area: Option<(u32, u32, u32, u32)>,
    ) {
        if !self.asked && frame.moving() {
            self.asked = true;
            let (captured, cache) = own;
            for slot in 0..RUN_TARGETS {
                match self.make(dev, who, (captured, &mut *cache), slot) {
                    Ok(target) => self.targets[slot] = Some(target),
                    Err(why) => {
                        emit_fmt(format_args!(
                            "gpud: runs short (targets={slot} of={RUN_TARGETS} refused={why})"
                        ));
                        break;
                    }
                }
            }
            emit_fmt(format_args!(
                "gpud: run planes (n={} bytes={RUN_PLANE_BYTES} held={} ceiling={})",
                self.targets.iter().flatten().count(),
                dev.ledger.held(),
                dev.ledger.ceiling()
            ));
        }
        let targets = self.targets.iter().take_while(|t| t.is_some()).count();
        let limits = RunLimits { targets, budget_px: PASS_BUDGET_PX, draws: MAX_DRAWS };
        let short = frame.take_runs(planned, (display, (layout_rows, HALF_ROWS)), area, limits);
        let runs = frame.runs();
        if runs.iter().any(Option::is_some) {
            self.used += 1;
            if self.used.is_power_of_two() {
                let below = runs.iter().flatten().find(|r| r.kind == RunKind::Below);
                let px = |a: Option<(u32, u32, u32, u32)>| {
                    a.map_or(0, |(x0, y0, x1, y1)| u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1))
                };
                let groups = || runs.iter().flatten().filter(|r| r.kind != RunKind::Below);
                // R-own's cost in this frame, counted (a sample: the frames this line names):
                // the pixels its glass backs draw under another window's opaque layer.
                let uncut = area.map_or(0, |area| {
                    let b = Board::new(frame, planned, display, layout_rows, HALF_ROWS);
                    let b = b.with_area(area);
                    (0..b.pass_count()).filter_map(|p| b.pass(p)).map(|p| b.uncut_px(&p)).sum()
                });
                emit_fmt(format_args!(
                    "gpud: runs (frames={} below={} below_px={} groups={} groups_drawn={} \
                     uncut_px={uncut})",
                    self.used,
                    below.map_or(0, |r| r.to),
                    px(below.and_then(|r| r.redraw)),
                    groups().count(),
                    groups().filter(|r| r.redraw.is_some()).count()
                ));
            }
        }
        if short != RunsShort::default() {
            self.short += 1;
            if self.short.is_power_of_two() {
                emit_fmt(format_args!(
                    "gpud: runs short (frames={} below={} groups={} apart={} targets={targets})",
                    self.short,
                    u8::from(short.below),
                    short.groups,
                    short.apart
                ));
            }
        }
    }

    /// The planes' buffers given back, after the context that bound them is gone.
    pub(super) fn release(&mut self, dev: &mut Device, client: ClientId) {
        for bo in self.bos.iter_mut().filter_map(Option::take) {
            let _ = dev.destroy_bo(client, bo);
        }
    }
}
