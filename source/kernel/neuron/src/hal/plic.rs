// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SiFive PLIC driver for the QEMU `virt` machine — per-hart supervisor
//! contexts (A6: ctx = 2*hart+1); v1 binding policy routes device interrupts to
//! the boot hart's context, delivered reactively to userspace drivers.
//! OWNERS: @kernel-hal-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! PUBLIC API: IrqId, plic_init, enable_source, disable_source, claim, complete
//! INVARIANTS: MMIO-only, no allocation; claim masks a source until complete
//!   (so level-triggered virtio IRQs cannot storm while a driver services them).
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use core::ptr::{read_volatile, write_volatile};

/// QEMU `virt` PLIC MMIO base.
const PLIC_BASE: usize = 0x0c00_0000;
/// Per-source priority registers (4 bytes each, source 1..=N at base + 4*source).
const PLIC_PRIORITY: usize = PLIC_BASE;
/// Per-context interrupt-enable bitmaps (0x80 bytes per context).
const PLIC_ENABLE_BASE: usize = PLIC_BASE + 0x2000;
/// Per-context priority threshold (0x1000 bytes per context).
const PLIC_THRESHOLD_BASE: usize = PLIC_BASE + 0x20_0000;
/// Per-context claim/complete register (same offset, 0x1000 bytes per context).
const PLIC_CLAIM_BASE: usize = PLIC_BASE + 0x20_0004;
const PLIC_CONTEXT_STRIDE: usize = 0x1000;
const PLIC_ENABLE_STRIDE: usize = 0x80;

/// Supervisor context of a hart on QEMU `virt`: hart N M-mode = ctx 2N,
/// hart N S-mode = ctx 2N+1. The kernel runs in S-mode.
const fn s_context(cpu_index: usize) -> usize {
    2 * cpu_index + 1
}

/// The executing hart's supervisor context (A6).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn current_s_context() -> usize {
    s_context(crate::smp::cpu_current_id().as_index())
}

/// Highest external IRQ source we manage (QEMU `virt` wires virtio-mmio[0..8] to
/// PLIC sources 1..=8). Bounds the enable bitmap we touch.
pub const MAX_IRQ: u32 = 95;

/// A PLIC interrupt source id (1..=MAX_IRQ; 0 is "no interrupt").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrqId(u32);

impl IrqId {
    /// Creates an `IrqId`, rejecting 0 (the PLIC "no interrupt" sentinel) and
    /// out-of-range sources.
    #[must_use]
    pub const fn new(raw: u32) -> Option<Self> {
        if raw == 0 || raw > MAX_IRQ {
            None
        } else {
            Some(Self(raw))
        }
    }

    /// Returns the raw source id.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline]
fn write_reg(addr: usize, val: u32) {
    // SAFETY: PLIC MMIO window is fixed by the QEMU `virt` machine and identity
    // mapped for the kernel; all offsets are bounds-checked by the callers.
    unsafe { write_volatile(addr as *mut u32, val) }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline]
fn read_reg(addr: usize) -> u32 {
    // SAFETY: see `write_reg`.
    unsafe { read_volatile(addr as *const u32) }
}

/// Initialises the PLIC supervisor context of every ONLINE hart (A6):
/// threshold 0 (accept any priority > 0) and all sources disabled. Only
/// online harts' contexts are touched — with SMP=1 the other contexts'
/// registers may not exist on the machine. Individual sources are enabled
/// lazily via [`enable_source`] when a driver binds them, so nothing can
/// fire until a handler exists. Called once by the boot hart after the
/// secondaries are online (MMIO init from one hart is fine).
pub fn plic_init() {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let online = crate::smp::cpu_online_mask();
        for cpu_idx in 0..crate::smp::MAX_CPUS {
            if online & (1 << cpu_idx) == 0 {
                continue;
            }
            let ctx = s_context(cpu_idx);
            write_reg(PLIC_THRESHOLD_BASE + ctx * PLIC_CONTEXT_STRIDE, 0);
            // Clear the S-context enable bitmap (sources 0..=MAX_IRQ).
            let enable = PLIC_ENABLE_BASE + ctx * PLIC_ENABLE_STRIDE;
            for word in 0..((MAX_IRQ as usize / 32) + 1) {
                write_reg(enable + word * 4, 0);
            }
        }
    }
}

/// Structural selftest read: a context's priority threshold (proves the
/// context registers were initialised and are addressable).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn selftest_ctx_threshold(cpu: crate::types::CpuId) -> u32 {
    read_reg(PLIC_THRESHOLD_BASE + s_context(cpu.as_index()) * PLIC_CONTEXT_STRIDE)
}

/// Structural selftest read: whether `irq` is enabled in a context's bitmap
/// (proves per-context enable isolation).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn selftest_source_enabled(irq: IrqId, cpu: crate::types::CpuId) -> bool {
    let src = irq.raw() as usize;
    let enable = PLIC_ENABLE_BASE + s_context(cpu.as_index()) * PLIC_ENABLE_STRIDE + (src / 32) * 4;
    read_reg(enable) & (1u32 << (src % 32)) != 0
}

/// Enables `irq` for `cpu`'s supervisor context with a non-zero priority,
/// so it can be delivered there. Idempotent. v1 policy: bindings target the
/// boot hart (the S_EXT trap is boot-owned); Phase B routes per affinity.
pub fn enable_source(irq: IrqId, cpu: crate::types::CpuId) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let src = irq.raw() as usize;
        // Priority must be > threshold (0) to be delivered.
        write_reg(PLIC_PRIORITY + src * 4, 1);
        let ctx = s_context(cpu.as_index());
        let enable = PLIC_ENABLE_BASE + ctx * PLIC_ENABLE_STRIDE + (src / 32) * 4;
        let bit = 1u32 << (src % 32);
        write_reg(enable, read_reg(enable) | bit);
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = (irq, cpu);
}

/// Disables `irq` for `cpu`'s supervisor context (masks it). Used to
/// quarantine an unbound source so it cannot storm.
pub fn disable_source(irq: IrqId, cpu: crate::types::CpuId) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let src = irq.raw() as usize;
        let ctx = s_context(cpu.as_index());
        let enable = PLIC_ENABLE_BASE + ctx * PLIC_ENABLE_STRIDE + (src / 32) * 4;
        let bit = 1u32 << (src % 32);
        write_reg(enable, read_reg(enable) & !bit);
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = (irq, cpu);
}

/// Claims the highest-priority pending interrupt for our context. Returns `None`
/// when there is nothing pending. The claimed source is masked by the PLIC until
/// [`complete`] is called, which prevents a level-triggered source from
/// re-firing while a driver services the device.
#[must_use]
pub fn claim() -> Option<IrqId> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // A6: claim from the EXECUTING hart's context — only sources enabled
        // for this context can be pending here.
        let raw = read_reg(PLIC_CLAIM_BASE + current_s_context() * PLIC_CONTEXT_STRIDE);
        IrqId::new(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        None
    }
}

/// Completes (acknowledges) `irq`, re-arming it at the PLIC so a future device
/// assertion can be delivered again. A driver calls this only after it has
/// cleared the device's own interrupt condition.
/// Completion must target the context that CLAIMED the source — for driver
/// completions (irq_complete syscall, possibly on another hart) that is the
/// BINDING's context, not the executing hart's.
pub fn complete(irq: IrqId, cpu: crate::types::CpuId) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        let ctx = s_context(cpu.as_index());
        write_reg(PLIC_CLAIM_BASE + ctx * PLIC_CONTEXT_STRIDE, irq.raw());
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = (irq, cpu);
}

/// Completes on the executing hart's context (for claim-then-complete drain
/// loops that never leave the hart).
pub fn complete_current(irq: IrqId) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        write_reg(PLIC_CLAIM_BASE + current_s_context() * PLIC_CONTEXT_STRIDE, irq.raw());
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    let _ = irq;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irq_id_rejects_zero_and_out_of_range() {
        assert!(IrqId::new(0).is_none());
        assert!(IrqId::new(MAX_IRQ + 1).is_none());
        assert_eq!(IrqId::new(3).unwrap().raw(), 3);
        // virtio-mmio slot N (0x10001000 + N*0x1000) maps to PLIC source 1+N on
        // QEMU virt; the input devices live at slots 2/3 => sources 3/4.
        assert_eq!(IrqId::new(1 + 2).unwrap().raw(), 3);
    }
}
