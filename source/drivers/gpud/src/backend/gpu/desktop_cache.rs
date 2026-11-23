// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The glass cache's planes (TASK-0351 P6). A glass's blurred backdrop is kept in rows
//! of a render target; one target spans a plane of `CACHE_ROWS` quarter- or half-resolution
//! rows, which holds four windows' glass. With twelve windows the board had rows for five: the
//! others were drawn without their blur and damaged their whole rectangle every frame
//! (`glass_off=81` of 32 frames, 84 ms a frame, `board--2026-11-18T10-51-11`). The cache now
//! grows with the glasses on the screen: when a plan is short of rows (`GlassCache::short`) a
//! further plane is made — a buffer of the GPU's own and its target — and the plan's glasses
//! take their rows in the same frame (`Frame::place_short`), so no glass is shown without its
//! backdrop. Planes are never given back while the compositor runs: a closed window's rows are
//! free for the next one. What bounds them is the planes' addresses in the context's map
//! (`crate::surface_va`), the targets' room in the render bundle's arenas and the GPU's memory;
//! a refusal leaves the glass off and is named once, with the device's word.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rows' rule in `nexus-gfx` `tests/frame_plan.rs` (host); the board's
//!   `gpud: glass cache (planes=…)` line and `glass_off=0` in `gpud: gpu frame us`

use alloc::vec::Vec;

use nexus_display_proto::layout::STRIDE_BYTES;
use nexus_driverkit::gpu::CacheMode;
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, GfxDevice};
use nexus_gfx::frame::{Frame, Planned};

use super::desktop::Refusal;
use super::desktop_surface::buffer;
use super::device_v2::Device;
use super::emit_fmt;
use super::render_template::{Captured, Target, Targets};
use crate::surface_va::cache_plane_va;

/// A cache plane's rows — inside the render target's 1080 (a pass draws only on the dataset's
/// screen).
pub(crate) const CACHE_ROWS: u32 = 1024;
const PLANE_BYTES: u64 = CACHE_ROWS as u64 * STRIDE_BYTES as u64;
const _: () = assert!(PLANE_BYTES <= crate::surface_va::CACHE_PLANE_SPAN);

/// The address of texel (`x`, `row`) of the cache, `row` counted through the planes; `None` for
/// a row past the planes' addresses.
pub(super) fn texel_va(x: u32, row: u32) -> Option<u64> {
    let plane = cache_plane_va(row / CACHE_ROWS)?;
    Some(plane + u64::from(row % CACHE_ROWS) * u64::from(STRIDE_BYTES) + u64::from(x) * 4)
}

/// The cache's planes: their targets, and the buffers made after the first (which the desktop
/// holds with its other buffers).
pub(super) struct CachePlanes {
    targets: Targets,
    planes: Vec<Target>,
    grown: Vec<BoId>,
    /// A refusal was named (once: a full table or the GPU's memory does not change by asking).
    refused: bool,
}

impl CachePlanes {
    /// The cache of one plane, `first`, and the targets' table further planes are placed in.
    pub(super) fn new(first: Target, targets: Targets) -> Self {
        Self { targets, planes: alloc::vec![first], grown: Vec::new(), refused: false }
    }

    /// Plane `plane`'s target.
    pub(super) fn target(&self, plane: u32) -> Option<Target> {
        self.planes.get(plane as usize).copied()
    }

    /// One more plane: its buffer bound read-write at its address, its target placed.
    fn grow(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        captured: &Captured,
    ) -> Result<(), Refusal> {
        let va = cache_plane_va(self.planes.len() as u32).ok_or("the cache planes' addresses")?;
        let mut bo = None;
        let made = buffer(
            dev,
            who,
            (PLANE_BYTES, CacheMode::GpuPrivate),
            va,
            (&mut bo, "a glass cache plane"),
        );
        // The buffer is held from the moment it exists, bound or not: it goes with the desktop.
        self.grown.extend(bo);
        made?;
        let target = self
            .targets
            .at(dev, who, captured, va, CACHE_ROWS)
            .map_err(|_| "a cache plane's target")?;
        self.planes.push(target);
        Ok(())
    }

    /// A target of the desktop's own beside the cache's planes (a run plane, `desktop_runs`),
    /// placed in the same table: the arenas' room is one bound for all of them.
    pub(super) fn target_at(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        captured: &Captured,
        (va, rows): (u64, u32),
    ) -> Result<Target, Refusal> {
        self.targets.at(dev, who, captured, va, rows).map_err(|_| "a run plane's target".into())
    }

    /// The plan `planned` of `frame` found the cache short of rows: planes are added until its
    /// glasses have theirs (each takes rows in this frame, a write of cause `new`) or the device
    /// refuses one — then those glasses stay off, as before, and the refusal is named.
    pub(super) fn fill(
        &mut self,
        dev: &mut Device,
        who: (ClientId, ContextId),
        captured: &Captured,
        frame: &mut Frame,
        planned: &mut Planned,
    ) {
        let short = frame.glass.short();
        if short == 0 || self.refused {
            return;
        }
        while frame.glass.short() != 0 {
            if let Err(what) = self.grow(dev, who, captured) {
                self.refused = true;
                emit_fmt(format_args!(
                    "gpud: glass cache FAIL (planes={} short_rows={} refused={what})",
                    self.planes.len(),
                    frame.glass.short()
                ));
                return;
            }
            planned.writes.new = planned.writes.new.saturating_add(frame.place_short());
        }
        emit_fmt(format_args!(
            "gpud: glass cache (planes={} rows={} short_rows={short})",
            self.planes.len(),
            self.planes.len() as u32 * CACHE_ROWS
        ));
    }

    /// The grown planes' buffers given back, after the context that bound them is gone.
    pub(super) fn release(&mut self, dev: &mut Device, client: ClientId) {
        for bo in self.grown.drain(..) {
            let _ = dev.destroy_bo(client, bo);
        }
    }
}
