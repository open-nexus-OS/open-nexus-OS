// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! How windowd's GPU context takes its memory: a buffer made and bound whole (`buffer`, `bind`
//! — the scratches, the cache, the frame's own, the ring), and the surface object (ADR-0078,
//! TASK-0333 6b-2b). The display's lazy
//! object — the wallpaper, the retained plane, the atlas — is adopted once and bound sparse at
//! the layout's address in two ranges, its planes' rows and its atlas's, read-only; the ring's
//! rows between them are the scan-out ring's own object (bound by the desktop), counted entered
//! here so no range ever crosses them. Before a frame's first pass the desktop enters the rows it
//! samples — the base plane, each layer's band (`frame_of`) — and only the pages not entered
//! yet reach the device: naming them commits them, the GPU's tables get them. The pages entered
//! are the object's committed rows the GPU reads; `gpud: layout split ok (…)` names them once.
//! OWNERS: @gpu @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the page arithmetic and the bitmap host-tested (`crate::surface_pages`); every
//!   layout texture a frame draws is a piece of a rectangle entered (`nexus-gfx`
//!   `tests/frame_board.rs`); the board (`gpud: layout split ok (`)

use nexus_display_proto::layout::{self, PLANE_BYTES, RING_ROWS, STRIDE_BYTES};
use nexus_driverkit::gpu::{Access, CacheMode, GpuVa};
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, GfxDevice, Mapping};

use super::desktop::{Refusal, LAYOUT_VA};
use super::device_v2::Device;
use crate::surface_pages::{row_pages, Entered, PAGE};

const STRIDE: u64 = STRIDE_BYTES as u64;

/// `len` bytes of `bo` from `offset` bound at `va`; a refusal names `what` and the device's word.
pub(super) fn bind(
    dev: &mut Device,
    (client, ctx): (ClientId, ContextId),
    (bo, offset, len): (BoId, u64, u64),
    (va, access): (u64, Access),
    what: &'static str,
) -> Result<(), Refusal> {
    dev.vm_bind(client, ctx, Mapping { bo, offset, len, va: GpuVa(va), access })
        .map_err(|e| Refusal(what, Some(e)))
}

/// A buffer of `bytes` in `mode`, recorded in `taken` as soon as it exists, bound read-write at
/// `va`.
pub(super) fn buffer(
    dev: &mut Device,
    who: (ClientId, ContextId),
    (bytes, mode): (u64, CacheMode),
    va: u64,
    (taken, what): (&mut Option<BoId>, &'static str),
) -> Result<(), Refusal> {
    let bo = dev.create_bo(who.0, bytes as usize, mode).map_err(|e| Refusal(what, Some(e)))?;
    *taken = Some(bo);
    bind(dev, who, (bo, 0, bytes), (va, Access::ReadWrite), what)
}

/// The surface object bound sparse in windowd's context, and the pages entered so far.
pub(super) struct SurfaceBinding {
    rows: u32,
    entered: Entered,
    /// The ring's pages: the ring object's, counted entered here.
    hole: u64,
    /// Time spent entering pages since the frame last took it (ns) — a band's first entry,
    /// apart from the clean (TASK-0333 6b-3).
    spent_ns: u64,
}

impl SurfaceBinding {
    /// The object `(handle, rows)` adopted as `who`'s buffer (recorded in `taken`) and bound
    /// sparse at the layout's address: its planes' rows and its atlas's, read-only, nothing
    /// entered. The ring's rows lie on page boundaries between them (three whole planes).
    pub(super) fn open(
        dev: &mut Device,
        who: (ClientId, ContextId),
        (handle, rows): (u32, u32),
        taken: &mut Option<BoId>,
    ) -> Result<Self, Refusal> {
        let len = u64::from(rows) * STRIDE;
        let ring_at = layout::row_offset_bytes(RING_ROWS[0]) as u64;
        let ring_end = ring_at + (RING_ROWS.len() * PLANE_BYTES) as u64;
        if len <= ring_end {
            return Err("a layout without its atlas".into());
        }
        if ring_at % PAGE != 0 || ring_end % PAGE != 0 || len % PAGE != 0 {
            return Err("a surface object off the page grid".into());
        }
        let bo = dev
            .adopt_lazy(who.0, handle, len as usize)
            .map_err(|e| Refusal("the surfaces", Some(e)))?;
        *taken = Some(bo);
        let sparse = |offset: u64, len: u64| Mapping {
            bo,
            offset,
            len,
            va: GpuVa(LAYOUT_VA + offset),
            access: Access::ReadOnly,
        };
        dev.vm_bind_sparse(who.0, who.1, sparse(0, ring_at))
            .map_err(|e| Refusal("the planes' binding", Some(e)))?;
        dev.vm_bind_sparse(who.0, who.1, sparse(ring_end, len - ring_end))
            .map_err(|e| Refusal("the atlas's binding", Some(e)))?;
        let mut entered = Entered::new(len / PAGE);
        let hole = (ring_at / PAGE, ring_end / PAGE);
        if !entered.enter_missing(hole.0, hole.1, |_, _| true) {
            return Err("the ring's rows".into());
        }
        Ok(Self { rows, entered, hole: hole.1 - hole.0, spent_ns: 0 })
    }

    /// The time spent entering pages since the last take (µs), and none from here on.
    pub(super) fn take_spent_us(&mut self) -> u64 {
        core::mem::take(&mut self.spent_ns) / 1000
    }

    /// Enters the pages rows `[row, row + h)` span (clipped to the object) that are not entered
    /// yet: one device range per run of missing pages.
    pub(super) fn enter(
        &mut self,
        dev: &mut Device,
        (client, ctx): (ClientId, ContextId),
        (row, h): (u32, u32),
    ) -> Result<(), Refusal> {
        let end = row.saturating_add(h).min(self.rows);
        if row >= end {
            return Ok(());
        }
        let (first, last) = row_pages(STRIDE, row, end - row);
        if self.entered.all(first, last) {
            return Ok(());
        }
        let t0 = nexus_abi::nsec().unwrap_or(0);
        let mut refused = None;
        let entered = self.entered.enter_missing(first, last, |a, b| {
            let va = GpuVa(LAYOUT_VA + a * PAGE);
            match dev.vm_enter(client, ctx, va, (b - a) * PAGE) {
                Ok(()) => true,
                Err(e) => {
                    refused = Some(e);
                    false
                }
            }
        });
        self.spent_ns += nexus_abi::nsec().unwrap_or(t0).saturating_sub(t0);
        if entered {
            Ok(())
        } else {
            Err(Refusal("a band's pages", refused))
        }
    }

    /// Pages entered for the frames (the ring's not counted).
    pub(super) fn entered_pages(&self) -> u64 {
        self.entered.count().saturating_sub(self.hole)
    }

    /// Bytes the object spans.
    pub(super) fn bytes(&self) -> u64 {
        u64::from(self.rows) * STRIDE
    }
}
