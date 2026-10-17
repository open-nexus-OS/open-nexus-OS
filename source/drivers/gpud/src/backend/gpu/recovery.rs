// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The firmware's messages about a context, served (TASK-0215 5c-3 P2c, RFC-0101 §6). A context
//! reset is held against the context it blames ([`fwif::ContextReset::blame`]): after a page
//! fault the one whose page catalogue it names, otherwise the one it numbers unless the reason
//! calls it innocent. That context is lost ([`Contexts`]): nothing new runs in it, its points
//! count as reached, its client destroys it; the others run on untouched. After its hardware
//! recovery the firmware asks for the free lists the reset cut off: each is rebuilt — its
//! entries written again, its struct as made, the render-target data on it in the recovery's
//! state — and the request is answered with the ids it named, so the firmware runs on (the open
//! kernel driver's handler, under its MIT option). A growth request is answered with no pages:
//! the device never grows a list (a pass that outgrows one renders in parts). The messages are
//! served where the device waits, between the firmware's busy answers to a cleanup, and on the
//! GPU's interrupt in the service loop. Each event is one line, formatted on the stack — a
//! client that faults again and again takes nothing of gpud's heap, which never frees; a reset
//! that blames no context of the device, or a list no context holds, is a FAIL line.

use nexus_gfx::backend::gpu::fwif::{self, Blame, ContextReset, KccbCmd, Reconstruction};

use super::bos::Bos;
use super::contexts::Contexts;
use super::device::Gpu;
use super::emit_fmt;

/// Serves every message the firmware has written, at most its buffer's worth at once.
pub(super) fn serve(gpu: &mut Gpu, contexts: &mut Contexts, bos: &mut Bos) {
    for _ in 0..fwif::fwccb::NUM_CMDS {
        let Some(cmd) = gpu.next_fwccb() else { return };
        if !cmd.has_magic() {
            Gpu::name(&cmd);
        } else if let Some(r) = cmd.context_reset() {
            reset(contexts, &r);
        } else if let Some(req) = cmd.reconstruction() {
            reconstruct(gpu, contexts, bos, &req);
        } else if let Some(id) = cmd.freelist_grow() {
            grow(gpu, contexts, id);
        } else {
            Gpu::name(&cmd);
        }
    }
}

/// A reset: the context it blames lost, or an innocent one counted.
fn reset(contexts: &mut Contexts, r: &ContextReset) {
    contexts.recovered.resets = contexts.recovered.resets.saturating_add(1);
    let (lost, catalogue) = match r.blame() {
        Blame::Catalogue(pc) => (contexts.lose_catalogue(pc), pc),
        Blame::Context(id) => (contexts.lose(id), 0),
        Blame::Innocent(id) => {
            contexts.recovered.innocent = contexts.recovered.innocent.saturating_add(1);
            return emit_fmt(format_args!(
                "gpud: gpu context reset innocent (ctx={id:#x} reason={} dm={} job={})",
                r.reason, r.dm, r.job_ref
            ));
        }
    };
    match lost {
        Some(ctx) => {
            contexts.recovered.last = Some((ctx, catalogue, r.fault_address));
            emit_fmt(format_args!(
                "gpud: gpu context lost (ctx={:#x} reason={} dm={} job={} flags={:#x} pc={:#x} fault={:#x})",
                ctx.0, r.reason, r.dm, r.job_ref, r.flags, r.pc_address, r.fault_address
            ));
        }
        None => {
            contexts.recovered.unattributed = contexts.recovered.unattributed.saturating_add(1);
            emit_fmt(format_args!(
                "gpud: FAIL gpu context reset (no context of the device: ctx={:#x} reason={} dm={} job={} flags={:#x} pc={:#x} fault={:#x})",
                r.context_id, r.reason, r.dm, r.job_ref, r.flags, r.pc_address, r.fault_address
            ));
        }
    }
}

/// A reconstruction request: each list named rebuilt, the request answered with its ids — also
/// one the device could not rebuild, since the firmware waits for the answer. A list whose
/// entries could not be written (its client unbound the list buffer under its own render
/// objects) is that context's own doing; a list no context holds is the device's failure.
fn reconstruct(gpu: &mut Gpu, contexts: &mut Contexts, bos: &mut Bos, req: &Reconstruction) {
    let block = gpu.mem.block();
    let (mut rebuilt, mut unwritten, mut unknown) = (0u32, 0u32, 0u32);
    for &id in req.ids() {
        let Some((index, list)) = contexts.list_owner(id) else {
            unknown += 1;
            continue;
        };
        let slot = contexts.slot(index);
        let struct_side = slot.render.as_ref().is_some_and(|r| r.rebuild(gpu, list));
        if struct_side && slot.write_entries(bos, list, block) {
            rebuilt += 1;
        } else {
            unwritten += 1;
        }
    }
    let sent = gpu.kccb_send(&KccbCmd::reconstruction_update(req.ids())).is_ok();
    let rec = &mut contexts.recovered;
    rec.reconstructions = rec.reconstructions.saturating_add(1);
    rec.rebuilt = rec.rebuilt.saturating_add(rebuilt);
    rec.unknown_lists = rec.unknown_lists.saturating_add(unknown);
    let fail = if unknown == 0 && sent { "" } else { "FAIL " };
    emit_fmt(format_args!(
        "gpud: {fail}gpu free lists rebuilt (n={rebuilt} unwritten={unwritten} unknown={unknown} answered={sent} hwr={} first_id={:#x})",
        req.hwr_counter,
        req.ids().first().copied().unwrap_or(0)
    ));
}

/// A growth request: answered with no pages added, the list's pages as they are.
fn grow(gpu: &mut Gpu, contexts: &mut Contexts, id: u32) {
    let list = contexts
        .list_owner(id)
        .and_then(|(index, list)| contexts.slot(index).render.as_ref()?.list(list));
    let Some((fw_addr, args)) = list else {
        return emit_fmt(format_args!(
            "gpud: FAIL gpu free-list growth (id={id:#x}: no such list)"
        ));
    };
    let answer = KccbCmd::freelist_grow_update(fw_addr, 0, args.initial_pages, args.ready_pages());
    let sent = gpu.kccb_send(&answer).is_ok();
    contexts.recovered.grows = contexts.recovered.grows.saturating_add(1);
    let fail = if sent { "" } else { "FAIL " };
    emit_fmt(format_args!(
        "gpud: {fail}gpu free list not grown (id={id:#x} pages={} answered={sent})",
        args.initial_pages
    ));
}
