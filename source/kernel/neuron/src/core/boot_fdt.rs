// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's copy of the firmware registers and its first read of the
//! device tree (RFC-0098 C3, TASK-0244 P3). The boot wrapper records `a0`/`a1`
//! right after BSS is zeroed; `map_kernel_segments` identity-maps the tree's pages
//! into the kernel address space; once that space is active, [`report`] parses the
//! tree with `nexus-fdt` and prints the values the platform will be built from
//! (TASK-0245) — `KSELFTEST: platform from fdt ok (uart=… plic=… tb=…Hz harts=…)`,
//! or the `FAIL` marker with the parser's reason. Nothing in the kernel consumes the
//! tree yet beyond this proof; the literals it replaces die with TASK-0245.
//!
//! WHY RECORD BEFORE PAGING: `_start` runs with SATP=0, so the tree's header can be
//! read at its physical address to learn `totalsize`; after the kernel address space
//! is active only mapped pages are reachable, and the tree lives wherever the previous
//! stage put it (QEMU: the top of RAM; nxboot: its own heap; the board: the FIT's copy).
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder (every profile requires the `ok` marker;
//!   the FAIL gate catches the other); the parser itself is host-tested in nexus-fdt
//! RFC: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md

use core::sync::atomic::{AtomicUsize, Ordering};

/// Physical address of the tree (0 = none recorded).
static DTB_PHYS: AtomicUsize = AtomicUsize::new(0);
/// `totalsize` read from its header (0 = none / invalid header).
static DTB_LEN: AtomicUsize = AtomicUsize::new(0);
/// The boot hart as the firmware reported it.
static BOOT_HART: AtomicUsize = AtomicUsize::new(0);

const FDT_MAGIC: u32 = 0xd00d_feed;
const PAGE: usize = 4096;
/// A tree larger than this is not one produced by any stage we boot from (QEMU
/// virt: ~12 KiB, the board: ~9 KiB plus headroom); refuse to map megabytes on
/// a corrupt header.
const MAX_DTB_LEN: usize = 1024 * 1024;

/// Record the firmware registers. Must run with paging OFF (physical reads) and
/// AFTER BSS is zeroed (these statics live in BSS).
pub fn record(hartid: usize, dtb: usize) {
    BOOT_HART.store(hartid, Ordering::Relaxed);
    DTB_PHYS.store(dtb, Ordering::Relaxed);
    if dtb == 0 || dtb % 4 != 0 {
        return;
    }
    // SAFETY: SATP is 0 here (early boot), `dtb` is the firmware-provided physical
    // address of a tree at least one header long; two aligned volatile reads, no
    // pointer is retained.
    let (magic, total) = unsafe {
        let p = dtb as *const u32;
        (
            u32::from_be(core::ptr::read_volatile(p)),
            u32::from_be(core::ptr::read_volatile(p.add(1))) as usize,
        )
    };
    if magic == FDT_MAGIC && (40..=MAX_DTB_LEN).contains(&total) {
        DTB_LEN.store(total, Ordering::Relaxed);
    }
}

/// The page-aligned physical range the tree occupies, for the identity map.
pub fn range() -> Option<(usize, usize)> {
    let base = DTB_PHYS.load(Ordering::Relaxed);
    let len = DTB_LEN.load(Ordering::Relaxed);
    if base == 0 || len == 0 {
        return None;
    }
    let start = base & !(PAGE - 1);
    let end = (base + len + PAGE - 1) & !(PAGE - 1);
    Some((start, end))
}

pub fn boot_hart() -> usize {
    BOOT_HART.load(Ordering::Relaxed)
}

/// The tree as bytes: at its physical address while paging is off, through the
/// identity map [`range`] once the kernel address space is active.
pub fn bytes() -> Option<&'static [u8]> {
    let base = DTB_PHYS.load(Ordering::Relaxed);
    let len = DTB_LEN.load(Ordering::Relaxed);
    if base == 0 || len == 0 {
        return None;
    }
    // SAFETY: `range()` was identity-mapped read-only by `map_kernel_segments`
    // before this space became active; the tree is never written by the kernel,
    // and `len` is the header's own `totalsize`, bounded by MAX_DTB_LEN.
    Some(unsafe { core::slice::from_raw_parts(base as *const u8, len) })
}

/// Parse the tree and print the platform values it carries, or why it could not
/// be parsed. Called once from `kmain` after the kernel address space is active.
pub fn report() {
    let Some(bytes) = bytes() else {
        log_info!(target: "selftest", "KSELFTEST: platform from fdt FAIL (no tree: a1=0x{:x})",
            DTB_PHYS.load(Ordering::Relaxed));
        return;
    };
    let fdt = match nexus_fdt::Fdt::new(bytes) {
        Ok(f) => f,
        Err(e) => {
            log_info!(target: "selftest", "KSELFTEST: platform from fdt FAIL ({})", e);
            return;
        }
    };
    let uart = fdt.stdout().and_then(|n| n.reg(0).ok().flatten()).map(|r| r.addr);
    let plic = fdt.plic().and_then(|n| n.reg(0).ok().flatten()).map(|r| r.addr);
    let cpus = fdt.cpus().ok();
    match (uart, plic, cpus) {
        (Some(uart), Some(plic), Some(cpus)) => {
            let banks = fdt.memory_banks().count();
            // `/chosen/nexus,*` is written by nxboot (the slot always; on QEMU the
            // lane's fw_cfg knobs): printing them proves the kernel reads the
            // loader's COPY of the tree, not the firmware's original — the RFC-0098
            // C2 round trip end to end. `-` = the property is absent.
            let chosen = fdt.chosen().ok();
            let nexus = |k: &str| chosen.and_then(|c| c.nexus_str(k)).unwrap_or("-");
            log_info!(target: "selftest",
                "KSELFTEST: platform from fdt ok (uart=0x{:x} plic=0x{:x} ndev={} tb={}Hz harts={} banks={} boot_hart={} timer={} ticks_per_us={} chosen.slot={} chosen.profile={} chosen.display={})",
                uart, plic, crate::hal::platform::plic_ndev(), cpus.timebase_hz,
                crate::hal::platform::hart_count(), banks, boot_hart(),
                if crate::hal::platform::timer_uses_sstc() { "sstc" } else { "sbi" },
                crate::hal::platform::ticks_per_us(),
                nexus("boot-slot"), nexus("boot-profile"), nexus("display-mode"));
        }
        _ => {
            log_info!(target: "selftest",
                "KSELFTEST: platform from fdt FAIL (missing: uart={} plic={} cpus={})",
                uart.is_some(), plic.is_some(), cpus.is_some());
        }
    }
}
