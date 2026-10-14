// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The board GPU in gpud (TASK-0329 G1, RFC-0101 Phase 2 / §8b): the thin side —
//! its glue through socd, its register window, the published firmware loaded by its documented
//! layout into page-backed DMA memory and started by the open sequence, the interrupt on its
//! own notify endpoint, the firmware's MMU-cache round trip, one GPU address space with a page
//! bound and unbound, and a null job whose timeline write is bridged to a kernel fence. The
//! logic is `nexus_gfx::backend::gpu` (host-tested); this module is the device: the window,
//! the memory with its cache maintenance (the SoC is non-coherent: every word the firmware
//! reads is cleaned after the CPU writes it, every word it writes is flushed before the CPU
//! reads it), the waits (on the interrupt endpoint, bounded, never a spin for an event), the
//! lines a board cycle is judged from.
//!
//! - [`glue`] — the GPU node up through socd (domain 2, `gpu_clk`, the reset after the clock).
//! - [`regs`] — `start::Regs` over the window: 64-bit accesses whole, bounded polls.
//! - [`memory`] — the firmware heap's backing: one DMA VMO, page-mapped by heap offset; the
//!   command buffers' and page tables' memory traits over it.
//! - [`firmware`] — the image placed, the boot data and page table built, the structures the
//!   interface fixes written.
//! - [`device`] — the bring-up ladder (`gpud: gpu glue|core|fw|mmu ok (`), the interrupt
//!   handler, the kernel command buffer's send-and-wait.
//! - [`job`] — the null job and the fence bridge (`gpud: gpu fence ok (`).
//! OWNERS: @gpu @runtime
//! STATUS: Functional (bring-up; `GfxDevice` on this device is layer 4)
//! API_STABILITY: Internal
//! TEST_COVERAGE: the board ladder (`scripts/board-test.sh`): `gpud: gpu glue ok (`,
//!   `gpud: gpu core ok (`, `gpud: gpu fw ok (`, `gpud: gpu mmu ok (`, `gpud: gpu fence ok (`;
//!   `gpud: FAIL gpu ` is a FAIL marker. QEMU grants no such GPU: nothing here runs there.
//! INVARIANTS: the GPU reads only pages of the one DMA VMO the kernel made for its capability
//!   (`vmo_runs` with it); the firmware blob is refused before a byte is placed when its id,
//!   build or version differs; no `unwrap` on a firmware-written word.

pub(crate) mod device;
mod device_v2;
mod firmware;
mod glue;
mod job;
mod memory;
pub(crate) mod quad;
mod regs;
mod render;
pub(crate) mod render_quad;
pub(crate) mod render_quads;

use nexus_service_topology::DEVICE_MMIO_SLOT;

pub(crate) use device::Gpu;
pub(crate) use device_v2::Device;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

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
