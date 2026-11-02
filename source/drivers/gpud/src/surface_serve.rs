// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OP_SURFACE_ALLOC` / `OP_SURFACE_FREE` in the request loop (RFC-0114 §2, ADR-0092):
//! windowd asks for an app surface's object; gpud makes it for the device that samples it (the
//! display's word: lazy for a GPU behind an MMU, eager for a device given runs), binds it where
//! the frames read it (the display's own compositor, the GPU beside it), charges the app it is
//! for and answers with the object's capability — all at allocation, never per frame. Only
//! windowd is served (identity = the kernel's sender id). A freed object is unbound and closed
//! when the last present that could read it retired (`retired`, at every ack).
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the table and the wire host-tested (`crate::surfaces`,
//!   `nexus_display_proto::surface_alloc`); the display lanes' `gpud: surface alloc ok (`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use nexus_display_proto::surface_alloc::{self as wire, Allocated};
use nexus_display_proto::{STATUS_DENIED, STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
use nexus_display_proto::{STATUS_QUOTA, STATUS_UNSUPPORTED};

use crate::backend::display::Display;
use crate::charges::{Charges, Part, Teller};
use crate::gpu_context::FrameGpu;
use crate::surfaces::{Geometry, Surface, Surfaces};

/// What the request loop keeps for the app surfaces.
pub(crate) struct SurfaceState {
    pub(crate) table: Surfaces,
    /// Every object together stays within the GPU's ceiling (two thirds of RAM, ADR-0078).
    ceiling: u64,
    /// The last present received: a freed object is read by no frame after it.
    pub(crate) last_present: u32,
}

impl SurfaceState {
    pub(crate) fn new() -> Self {
        let ram = nexus_abi::mm_stats().map_or(0, |s| s.total.saturating_mul(4096));
        Self { table: Surfaces::new(), ceiling: crate::gpu_share::ceiling_of(ram), last_present: 0 }
    }
}

/// The loop's parts an allocation touches.
pub(crate) struct Ctx<'a> {
    pub server: &'a nexus_ipc::KernelServer,
    pub display: &'a mut dyn Display,
    pub frames: Option<&'a mut dyn FrameGpu>,
    pub charges: &'a mut Charges,
    pub teller: &'a Teller,
}

/// `OP_SURFACE_ALLOC` from `sender`: the object made, bound, charged and its capability moved
/// to windowd with the answer — or the refusal answered by name.
pub(crate) fn alloc(ctx: Ctx<'_>, state: &mut SurfaceState, sender: u64, frame: &[u8]) {
    let server = ctx.server;
    let answer = |a: Allocated, cap: Option<u32>| answer(server, a, cap);
    let Some(req) = wire::decode_alloc_request(frame) else {
        return answer(refused(0, STATUS_MALFORMED), None);
    };
    if !crate::gpu_context::may_open(sender) {
        return answer(refused(req.cookie, STATUS_DENIED), None);
    }
    let geometry = Geometry::for_request(&req);
    let len = geometry.len();
    if !state.table.admits_more(len, state.ceiling) {
        say_refused(req.owner_sid, "the gpu's ceiling");
        return answer(refused(req.cookie, STATUS_QUOTA), None);
    }
    let Some((device, lazy, needs_gpu)) = ctx.display.surface_device() else {
        return answer(refused(req.cookie, STATUS_UNSUPPORTED), None);
    };
    if needs_gpu && ctx.frames.is_none() {
        say_refused(req.owner_sid, "no gpu context to bind in");
        return answer(refused(req.cookie, STATUS_DEVICE_ERROR), None);
    }
    let made = if lazy {
        nexus_abi::vmo_create_for_lazy(device, len)
    } else {
        nexus_abi::vmo_create_for(device, len)
    };
    let Ok(vmo) = made else {
        say_refused(req.owner_sid, "no memory");
        return answer(refused(req.cookie, STATUS_QUOTA), None);
    };
    let Some(buffer) = state.table.insert(req.owner_sid, vmo, geometry) else {
        let _ = nexus_abi::vmo_destroy(vmo);
        return answer(refused(req.cookie, STATUS_QUOTA), None);
    };
    let Ctx { display, mut frames, charges, teller, .. } = ctx;
    let bound = display.bind_surface(buffer, vmo, &geometry)
        && frames.as_deref_mut().is_none_or(|f| f.bind_surface(buffer, vmo, &geometry));
    if !bound {
        display.unbind_surface(buffer);
        if let Some(f) = frames.as_deref_mut() {
            f.unbind_surface(buffer);
        }
        state.table.remove(buffer);
        let _ = nexus_abi::vmo_destroy(vmo);
        say_refused(req.owner_sid, "the device refused its binding");
        return answer(refused(req.cookie, STATUS_QUOTA), None);
    }
    teller.charge(charges, req.owner_sid, Part::Surfaces, state.table.owned_by(req.owner_sid));
    let a = Allocated {
        cookie: req.cookie,
        status: STATUS_OK,
        buffer,
        stride: geometry.stride,
        width: geometry.width,
        rows: geometry.rows,
        slots: geometry.slots,
        cache_block: cache_block(device),
    };
    let _ = nexus_abi::debug_println(&alloc::format!(
        "gpud: surface alloc ok (buffer={buffer} owner={:#x} {}x{} slots={} stride={} bytes={len} lazy={})",
        req.owner_sid,
        a.width,
        a.rows,
        a.slots,
        a.stride,
        u8::from(lazy)
    ));
    answer(a, nexus_abi::cap_clone(vmo).ok());
}

/// `OP_SURFACE_FREE` from `sender`: the object is read by no present after the last received;
/// it goes when that one retired.
pub(crate) fn free(state: &mut SurfaceState, sender: u64, frame: &[u8]) {
    let Some(buffer) = wire::decode_free(frame) else { return };
    if crate::gpu_context::may_open(sender) {
        let _ = state.table.free_after(buffer, state.last_present);
    }
}

/// The present `seq` retired (its ack went out): every freed object no later frame reads is
/// unbound from the display and the GPU, closed, and its app's charge lowered.
pub(crate) fn retired(ctx: Ctx<'_>, state: &mut SurfaceState, seq: u32) {
    let Ctx { display, mut frames, charges, teller, .. } = ctx;
    let mut owners = [0u64; 8];
    let mut n = 0;
    state.table.retire(seq, &mut |s: Surface| {
        display.unbind_surface(s.buffer);
        if let Some(f) = frames.as_deref_mut() {
            f.unbind_surface(s.buffer);
        }
        let _ = nexus_abi::cap_close(s.vmo);
        if n < owners.len() && !owners[..n].contains(&s.owner) {
            owners[n] = s.owner;
            n += 1;
        }
    });
    for owner in &owners[..n] {
        teller.charge(charges, *owner, Part::Surfaces, state.table.owned_by(*owner));
    }
}

/// A new compositor runs (RFC-0112: windowd's framebuffer request — every instance asks once,
/// first): no frame of the old one is presented again. The layers it left are composed no more,
/// and every app surface it composed is unbound from the display and the GPU, closed and
/// uncharged at once — the apps make theirs again with the new compositor.
pub(crate) fn compositor_restarted(ctx: Ctx<'_>, state: &mut SurfaceState) {
    let Ctx { display, mut frames, charges, teller, .. } = ctx;
    display.forget_layers();
    if let Some(f) = frames.as_deref_mut() {
        f.forget_layers();
    }
    let mut owners: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    state.table.release_all(&mut |s: Surface| {
        display.unbind_surface(s.buffer);
        if let Some(f) = frames.as_deref_mut() {
            f.unbind_surface(s.buffer);
        }
        let _ = nexus_abi::cap_close(s.vmo);
        if !owners.contains(&s.owner) && owners.try_reserve(1).is_ok() {
            owners.push(s.owner);
        }
    });
    for owner in &owners {
        teller.charge(charges, *owner, Part::Surfaces, 0);
    }
    if !owners.is_empty() {
        let _ = nexus_abi::debug_println(&alloc::format!(
            "gpud: compositor restarted (owners={} surfaces released)",
            owners.len()
        ));
    }
}

const fn refused(cookie: u32, status: u8) -> Allocated {
    Allocated { cookie, status, buffer: 0, stride: 0, width: 0, rows: 0, slots: 0, cache_block: 0 }
}

/// The answer on gpud's response route, with the object's capability when there is one; a
/// capability that cannot travel is closed and the answer says the device failed.
fn answer(server: &nexus_ipc::KernelServer, a: Allocated, cap: Option<u32>) {
    use nexus_ipc::{Server as _, Wait};
    if let Some(cap) = cap {
        let send = nexus_service_topology::slots::gpud::SERVER.send;
        if nexus_ipc::exchange::send_with_cap(send, &wire::encode_allocated(&a), cap).is_ok() {
            return;
        }
        let _ = nexus_abi::cap_close(cap);
        let failed = Allocated { status: STATUS_DEVICE_ERROR, ..refused(a.cookie, 0) };
        let _ = server.send(&wire::encode_allocated(&failed), Wait::Blocking);
        return;
    }
    let _ = server.send(&wire::encode_allocated(&a), Wait::Blocking);
}

/// The cache line a writer of an object for `device` cleans in (`vmo_clean`); 0 = coherent.
fn cache_block(device: u32) -> u16 {
    match nexus_abi::device_dma_coherence(device) {
        Ok(nexus_abi::DmaCoherence::Maintained { block }) => u16::try_from(block).unwrap_or(0),
        _ => 0,
    }
}

fn say_refused(owner: u64, why: &str) {
    let _ = nexus_abi::debug_println(&alloc::format!(
        "gpud: FAIL surface alloc (owner={owner:#x} {why})"
    ));
}
