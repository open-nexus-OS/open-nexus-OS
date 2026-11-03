// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OP_SURFACE_ADOPT` in the request loop (RFC-0112 Phase 3, TASK-0348 P9): a
//! restarted windowd asks, after its grant and its GPU context, for the app surfaces gpud kept
//! from the compositor before it. Every orphan a frame showed is bound in the new context where
//! the GPU beside the display needs it (the display's own binding never went), handed over with
//! the place its body had — the ghost shows the app's last pixels there until the app presents
//! again — and the orphans no frame showed (minimized, hidden) are unbound, closed and uncharged
//! now: a minimized window has no ghost. Paged; the last page says `remaining = 0`. Only windowd
//! is served (identity = the kernel's sender id).
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the table host-tested (`crate::surfaces`), the wire
//!   (`nexus_display_proto::surface_adopt`); the many-windows lane's `gpud: surfaces adopted (`
//!   and windowd's `windowd: restarted (ghosts=`

use alloc::vec::Vec;

use nexus_display_proto::surface_adopt::{self as wire, Adopted, AdoptedPage};
use nexus_ipc::{Server as _, Wait};

use crate::charges::Part;
use crate::surface_serve::{Ctx, SurfaceState};
use crate::surfaces::Surface;

/// `OP_SURFACE_ADOPT` from `sender`: one page of the placed orphans, adopted; the unplaced
/// released. A stranger's ask is dropped unanswered.
pub(crate) fn adopt(ctx: Ctx<'_>, state: &mut SurfaceState, sender: u64, frame: &[u8]) {
    if !wire::is_adopt_request(frame) || !crate::gpu_context::may_open(sender) {
        return;
    }
    let Ctx { server, display, mut frames, charges, teller } = ctx;
    let mut owners: Vec<u64> = Vec::new();
    let mut released = 0u32;
    let mut release = |s: Surface, frames: &mut Option<&mut dyn crate::gpu_context::FrameGpu>| {
        display.unbind_surface(s.buffer);
        if let Some(f) = frames.as_deref_mut() {
            f.unbind_surface(s.buffer);
        }
        let _ = nexus_abi::cap_close(s.vmo);
        released += 1;
        if !owners.contains(&s.owner) && owners.try_reserve(1).is_ok() {
            owners.push(s.owner);
        }
    };
    state.table.release_unplaced_orphans(&mut |s| release(s, &mut frames));
    let mut page = AdoptedPage::new();
    while page.has_room() {
        let Some(s) = state.table.next_orphan() else { break };
        let Some(place) = s.place() else { break };
        // The GPU beside the display reads it in the new compositor's context: bound there
        // again (the old context took the old binding with it); the display's own binding stays.
        let bound =
            frames.as_deref_mut().is_none_or(|f| f.bind_surface(s.buffer, s.vmo, &s.geometry));
        let a = Adopted {
            buffer: s.buffer,
            owner: s.owner,
            width: s.geometry.width,
            rows: s.geometry.rows,
            stride: s.geometry.stride,
            slots: s.geometry.slots,
            cache_block: state.cache_block,
            dst_x: place.dst_x,
            dst_y: place.dst_y,
            w: place.w,
            h: place.h,
            slot: place.slot,
            z: place.z,
            window_key: place.window_key,
        };
        if bound && page.push(&a) {
            state.table.adopted(s.buffer);
            state.adopted = state.adopted.saturating_add(1);
        } else {
            // Refused by the GPU or past its own object: no ghost of it.
            if let Some(gone) = state.table.remove(s.buffer) {
                release(gone, &mut frames);
            }
        }
    }
    for owner in &owners {
        teller.charge(charges, *owner, Part::Surfaces, state.table.owned_by(*owner));
    }
    let remaining = u16::try_from(state.table.orphans_placed()).unwrap_or(u16::MAX);
    if remaining == 0 && (state.adopted > 0 || released > 0) {
        let _ = nexus_abi::debug_println(&alloc::format!(
            "gpud: surfaces adopted (n={} released={released})",
            state.adopted
        ));
    }
    let restarted = state.compositors > 1;
    let _ = server.send(page.finish(restarted, remaining), Wait::Blocking);
}
