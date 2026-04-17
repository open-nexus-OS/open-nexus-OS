//! Phase: mmio (extracted in Cut P2-04 of TASK-0023B).
//!
//! Owns the TASK-0010 userspace MMIO capability mapping slice:
//!   mmio_map_probe (virtio-mmio magic register) +
//!   cap_query_mmio_probe ((base, len) for address-bearing MMIO caps) +
//!   cap_query_vmo_probe ((base, len) for VMO caps).
//!
//! Marker order and marker strings are byte-identical to the pre-cut body.
//! No service routing performed here; mmio probes only invoke kernel cap APIs.

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::mmio;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // TASK-0010: userspace MMIO capability mapping proof (virtio-mmio magic register).
    if mmio::mmio_map_probe().is_ok() {
        emit_line("SELFTEST: mmio map ok");
    } else {
        emit_line("SELFTEST: mmio map FAIL");
    }
    // Pre-req for virtio DMA: userland can query (base,len) for address-bearing caps.
    if mmio::cap_query_mmio_probe().is_ok() {
        emit_line("SELFTEST: cap query mmio ok");
    } else {
        emit_line("SELFTEST: cap query mmio FAIL");
    }
    if mmio::cap_query_vmo_probe().is_ok() {
        emit_line("SELFTEST: cap query vmo ok");
    } else {
        emit_line("SELFTEST: cap query vmo FAIL");
    }
    Ok(())
}
