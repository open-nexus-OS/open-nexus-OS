// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The board GPU in gpud (TASK-0329 G1, RFC-0101 Phase 2–4 / §8b–§8d): the thin side —
//! its glue through socd, its register window, the published firmware loaded by its documented
//! layout into page-backed DMA memory and started by the open sequence, the interrupt on its
//! own notify endpoint, the firmware's MMU-cache round trip and its cleanup of objects, and
//! `GfxDevice` over it: several contexts, each its own GPU address space and firmware objects,
//! buffers bound in ranges, transfer and render passes bridged to the timeline. The logic is
//! `nexus_gfx::backend::gpu` (host-tested; like this module, its sequences follow the open
//! kernel driver, used under its MIT option — the notice is kept in `THIRD_PARTY_NOTICES`); this
//! module is the device: the window, the memory with its cache maintenance (the SoC is
//! non-coherent: every word the firmware reads is cleaned after the CPU writes it, every word it
//! writes is flushed before the CPU reads it), the waits (on the interrupt endpoint, bounded,
//! never a spin for an event), the lines a board cycle is judged from.
//!
//! - [`glue`] — the GPU node up through socd (domain 2, `gpu_clk`, the reset after the clock).
//! - [`regs`] — `start::Regs` over the window: 64-bit accesses whole, bounded polls.
//! - [`memory`] — the firmware heap's backing: one DMA VMO, page-mapped by heap offset, pages
//!   handed out and taken back; the command buffers' and page tables' memory traits over it.
//! - [`firmware`] — the image placed, the boot data and page table built, the structures the
//!   interface fixes written.
//! - [`device`] — the bring-up ladder (`gpud: gpu glue|core|fw|mmu ok (`), the interrupt
//!   handler, the kernel command buffer's send-and-wait, the firmware's cleanup of an object.
//! - [`space`] — a context's GPU address space: its catalogue and firmware memory context.
//! - [`job`] — a context's transfer queue, the null job and the fence bridge (`gpud: gpu fence
//!   ok (`).
//! - [`render`] — a context's render objects: the render context, free lists, dataset.
//! - [`contexts`], [`bos`], [`gpu_vmo`], [`device_v2`] — `GfxDevice`: the context slots, the
//!   buffers (CPU-mapped, the GPU's alone, a plane of the ring), the trait over them.
//! - [`recovery`] — the firmware's messages about a context served: a reset held against the
//!   context it blames (lost, the others untouched), the free lists rebuilt after the firmware's
//!   recovery, a growth request answered.
//! - [`scene_load`] and the `render_*` rungs — the captured scenes loaded and drawn, the device's
//!   board gates (`gpud: gpu fill|quad|render|library|quads|area|pipeline|blur radii|texel
//!   offsets|mixed|contexts|containment ok (`).
//! - [`client_context`], [`desktop`] — windowd's context (`gpud: gpu library ok (client=windowd`)
//!   and the desktop drawn in it from the frame planner's steps (`gpud: gpu desktop ok (`).
//! OWNERS: @gpu @runtime
//! STATUS: Functional (`GfxDevice` with several contexts on the board, a fault kept in its
//!   context, TASK-0215 5c-3 P2)
//! API_STABILITY: Internal
//! TEST_COVERAGE: the board ladder (`scripts/board-test.sh`): `gpud: gpu glue ok (` …
//!   `gpud: gpu containment ok (`; `gpud: FAIL gpu ` is a FAIL marker. QEMU grants no such GPU:
//!   nothing here runs there. The decisions the device takes on the firmware's messages
//!   (whom a reset blames, a list rebuilt as made) are host-tested in `nexus_gfx`.
//! INVARIANTS: the GPU reads only pages of VMOs the kernel made for its capability (`vmo_runs`
//!   with it) — the firmware heap's backing and the device's buffers — or the display's planes;
//!   a context reaches only what is bound in its own space; a fault loses only the context it
//!   is held against; nothing the firmware may still hold is freed; the firmware blob is
//!   refused before a byte is placed when its id, build or version differs; no `unwrap` on a
//!   firmware-written word.

mod bos;
pub(crate) mod client_context;
mod contexts;
mod desktop;
mod desktop_gate;
mod desktop_pass;
pub(crate) mod device;
mod device_v2;
mod firmware;
mod glue;
mod gpu_vmo;
mod job;
mod library;
mod memory;
pub(crate) mod quad;
mod recovery;
mod regs;
mod render;
mod render_blur;
pub(crate) mod render_containment;
pub(crate) mod render_contexts;
pub(crate) mod render_cost;
pub(crate) mod render_library;
mod render_mixed;
mod render_passes;
pub(crate) mod render_quad;
pub(crate) mod render_quads;
mod render_template;
mod render_texels;
mod scene_load;
mod space;

use nexus_service_topology::DEVICE_MMIO_SLOT;

pub(crate) use client_context::BoardGpu;
pub(crate) use device::Gpu;
pub(crate) use device_v2::Device;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

// A line formatted on the stack (`diag::fmt_line`): what a client can cause again and again is
// never formatted into gpud's heap, which never frees.
use crate::diag::fmt_line as emit_fmt;

/// Whether init granted gpud a GPU window beside the board's display plane — the board's GPU
/// node (`init: gpu plane from tree`). On QEMU the GPU window is the virtio one and the display
/// plane is absent, so this is false wherever `dc::granted()` is.
pub(crate) fn granted() -> bool {
    let mut info = nexus_abi::CapQuery::default();
    crate::backend::dc::granted()
        && nexus_abi::cap_query(DEVICE_MMIO_SLOT, &mut info).is_ok()
        && info.kind_tag == 2
        && info.irq != 0
}
