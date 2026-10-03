// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's view of the device tree (RFC-0098 C3, TASK-0245 P4). The kernel
//! injects a read-only alias of the tree it was handed in `a1` into init's slot
//! `INIT_DEVICE_TREE_SLOT`; this module maps it once, parses it with `nexus-fdt`
//! and answers the two questions init has: which devices exist (every
//! `virtio,mmio` transport with its window and PLIC line, the RTC by compatible)
//! and what the loader wrote into `/chosen/nexus,*` (the lane's profile). Every
//! device capability init mints carries the node's `reg` and `interrupts`, its
//! coherence and the DMA reach of the buses above it (`dma-ranges`, TASK-0246 P1) —
//! one descriptor (`nexus_abi::DeviceDesc`); no address, interrupt number or reach
//! is derived from anything else. The same alias is handed to the harness
//! (`NamedSlot::DeviceTree`).
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder — `init: devices from fdt ok (...)` is required in
//!   every profile; the parser is host-tested in nexus-fdt
//! RFC: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md

use core::sync::atomic::{AtomicUsize, Ordering};

use nexus_fdt::Fdt;

use crate::bootstrap::helpers::{debug_write_bytes, debug_write_hex};
use crate::os_payload::{InitError, Result};

/// One device as the tree describes it: the MMIO window, the PLIC line, the
/// coherence and the DMA reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeviceWindow {
    pub base: usize,
    pub len: usize,
    /// First cell of `interrupts` (0 = the node lists none).
    pub irq: u32,
    /// The device does not snoop the CPU caches: `dma-noncoherent` on the node or
    /// its bus (RFC-0098 C4) — minted into the capability like the line.
    pub dma_noncoherent: bool,
    /// What its DMA can address: the `dma-ranges` of the buses above it, composed
    /// (RFC-0098 C4, TASK-0246 P1) — the kernel allocates the device's memory inside
    /// it and answers `vmo_runs` in its bus addresses.
    pub reach: nexus_fdt::DmaReach,
}

// Every reach the tree can yield fits the descriptor.
const _: () = assert!(nexus_fdt::MAX_DMA_WINDOWS <= nexus_abi::MAX_DMA_WINDOWS);

impl DeviceWindow {
    /// The descriptor the kernel mints the device capability from.
    pub(crate) fn desc(&self) -> Result<nexus_abi::DeviceDesc> {
        let mut windows = [nexus_abi::DmaWindow::default(); nexus_abi::MAX_DMA_WINDOWS];
        let reach = self.reach.windows();
        for (out, w) in windows.iter_mut().zip(reach) {
            *out = nexus_abi::DmaWindow { bus: w.bus, cpu: w.cpu, size: w.size };
        }
        let desc = nexus_abi::DeviceDesc::new(
            self.base as u64,
            self.len as u64,
            self.irq,
            self.dma_noncoherent,
            &windows[..reach.len()],
        );
        desc.ok_or(InitError::Map("dma reach wider than the device descriptor"))
    }
}

/// The virtio transports init hands to drivers, classified by device id.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VirtioDevices {
    pub net: Option<DeviceWindow>,
    pub rng: Option<DeviceWindow>,
    /// ADR-0044: [0] = the ONE disk; [1] = a second device if the launcher attaches one.
    pub blk: [Option<DeviceWindow>; 2],
    pub gpu: Option<DeviceWindow>,
    pub input: [Option<DeviceWindow>; 3],
    /// Transports seen (for the marker).
    pub transports: usize,
}

/// Where the tree is mapped (0 = not yet / unavailable).
static TREE_VA: AtomicUsize = AtomicUsize::new(0);
static TREE_LEN: AtomicUsize = AtomicUsize::new(0);

const FDT_MAGIC: u32 = 0xd00d_feed;
/// No stage we boot from produces a tree this large; a bigger header is corrupt.
const MAX_DTB_LEN: usize = 1024 * 1024;
const PAGE: usize = 4096;

/// The tree as bytes, mapped on first use. `None` = the kernel injected no alias
/// (a foreign loader with an unaligned tree) or the header is not a tree.
pub(crate) fn bytes() -> Option<&'static [u8]> {
    let va = TREE_VA.load(Ordering::Acquire);
    if va != 0 {
        let len = TREE_LEN.load(Ordering::Acquire);
        // SAFETY: mapped read-only below for exactly this length; never unmapped.
        return Some(unsafe { core::slice::from_raw_parts(va as *const u8, len) });
    }
    let slot = nexus_abi::INIT_DEVICE_TREE_SLOT;
    let mut info = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(slot, &mut info).ok()?;
    let pages = usize::try_from(info.len).ok()?;
    if pages == 0 || pages % PAGE != 0 || pages > MAX_DTB_LEN + PAGE {
        return None;
    }
    let flags =
        nexus_abi::page_flags::VALID | nexus_abi::page_flags::READ | nexus_abi::page_flags::USER;
    let va = nexus_abi::vm_map(slot, 0, pages, flags).ok()?;
    // The header bounds the tree inside the pages the kernel exposed.
    // SAFETY: `pages` bytes are mapped at `va`, read-only.
    let head = unsafe { core::slice::from_raw_parts(va as *const u8, 8) };
    let magic = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let total = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if magic != FDT_MAGIC || total < 40 || total > pages {
        return None;
    }
    TREE_LEN.store(total, Ordering::Release);
    TREE_VA.store(va, Ordering::Release);
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, total) })
}

/// A parsed view (cheap: the parser is a bounds-checked cursor over `bytes()`).
pub(crate) fn tree() -> Option<Fdt<'static>> {
    Fdt::new(bytes()?).ok()
}

/// `/chosen/nexus,<name>` as written by the loader (RFC-0098 C2).
pub(crate) fn chosen_str(name: &str) -> Option<&'static str> {
    tree()?.chosen().ok()?.nexus_str(name)
}

/// A node's device window: its first `reg` (page-aligned, whole pages), its line, coherence and
/// DMA reach.
pub(crate) fn window_of(node: nexus_fdt::Node<'static>) -> Option<DeviceWindow> {
    let reg = node.reg(0).ok().flatten()?;
    let addr = usize::try_from(reg.addr).ok()?;
    let size = usize::try_from(reg.size).ok()?;
    if size == 0 {
        return None;
    }
    // A device window is the pages its registers occupy (RFC-0017: MMIO is granted by
    // the page): a block whose registers start inside a page — the board's APMU syscon
    // at 0xd4282800, a 0x400-byte block — gets the page it lies in, and its consumer
    // applies the in-page offset it reads from the same tree node (TASK-0260B P3: the
    // old `base % PAGE != 0` refusal left socd without the APMU).
    let base = addr & !(PAGE - 1);
    let len = (addr + size).div_ceil(PAGE) * PAGE - base;
    // A window is granted in whole pages (the create syscall requires it).
    let irq = node.interrupts().next().unwrap_or(0);
    // A malformed `dma-ranges` above the node is a wrong tree: no window, no grant.
    let reach = node.dma_reach().ok()?;
    Some(DeviceWindow { base, len, irq, dma_noncoherent: !node.dma_coherent(), reach })
}

/// Every `virtio,mmio` transport the tree lists, classified by the device id read
/// through a short-lived window of its own. Windows are minted from the node's
/// `reg` + `interrupts` — the same numbers the grants below carry.
pub(crate) fn discover_virtio() -> Result<VirtioDevices> {
    const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976; // "virt"
    const VIRTIO_DEVICE_ID_NET: u32 = 1;
    const VIRTIO_DEVICE_ID_BLK: u32 = 2;
    const VIRTIO_DEVICE_ID_RNG: u32 = 4;
    const VIRTIO_DEVICE_ID_GPU: u32 = 16;
    const VIRTIO_DEVICE_ID_INPUT: u32 = 18;
    const REG_MAGIC: usize = 0x000;
    const REG_DEVICE_ID: usize = 0x008;

    let fdt = tree().ok_or(InitError::Map("no device tree"))?;
    let mut found = VirtioDevices::default();
    // Lowest address first: the launcher attaches devices in that order, and the
    // slot order downstream (blk[0] = the ONE disk, input[0..3]) relies on it.
    let mut windows: [Option<DeviceWindow>; 16] = [None; 16];
    let mut n = 0;
    for node in fdt.find_compatible(&["virtio,mmio"]) {
        if let Some(w) = window_of(node) {
            if n < windows.len() {
                windows[n] = Some(w);
                n += 1;
            }
        }
    }
    windows[..n].sort_unstable_by_key(|w| w.map_or(usize::MAX, |w| w.base));
    for w in windows[..n].iter().flatten() {
        found.transports += 1;
        let cap =
            nexus_abi::device_mmio_cap_create(&w.desc()?, usize::MAX).map_err(InitError::Abi)?;
        let va = nexus_abi::mmio_map_auto(cap, 0, w.len).map_err(InitError::Abi)?;
        // SAFETY: the window was just mapped for `w.len` bytes; two register reads.
        let (magic, device_id) = unsafe {
            (
                core::ptr::read_volatile((va + REG_MAGIC) as *const u32),
                core::ptr::read_volatile((va + REG_DEVICE_ID) as *const u32),
            )
        };
        let _ = nexus_abi::vm_unmap(va, w.len);
        let _ = nexus_abi::cap_close(cap);
        if magic != VIRTIO_MMIO_MAGIC {
            continue;
        }
        let w = Some(*w);
        match device_id {
            VIRTIO_DEVICE_ID_NET => found.net = found.net.or(w),
            VIRTIO_DEVICE_ID_RNG => found.rng = found.rng.or(w),
            VIRTIO_DEVICE_ID_GPU => found.gpu = found.gpu.or(w),
            VIRTIO_DEVICE_ID_BLK => {
                if let Some(free) = found.blk.iter_mut().find(|s| s.is_none()) {
                    *free = w;
                }
            }
            VIRTIO_DEVICE_ID_INPUT => {
                if let Some(free) = found.input.iter_mut().find(|s| s.is_none()) {
                    *free = w;
                }
            }
            _ => {}
        }
    }
    Ok(found)
}

/// Every SoC-glue provider the tree lists (RFC-0106), with the window init grants
/// to `socd`: `(kind, window)` — the kind is nexus-soc's, so the slot index the
/// grant lands in is the one socd maps. At most one window per kind; a provider
/// without a page-aligned `reg` is skipped (the tree, not a guess, is wrong then).
pub(crate) fn providers() -> impl Iterator<Item = (nexus_soc::ProviderKind, DeviceWindow)> {
    let mut found: [Option<DeviceWindow>; 6] = [None; 6];
    if let Some(fdt) = tree() {
        for node in fdt.all_nodes() {
            let Some(kind) = nexus_soc::ProviderKind::of(node) else { continue };
            if found[kind as usize].is_none() {
                found[kind as usize] = window_of(node);
            }
        }
    }
    const KINDS: [nexus_soc::ProviderKind; 6] = [
        nexus_soc::ProviderKind::Apbc,
        nexus_soc::ProviderKind::Apmu,
        nexus_soc::ProviderKind::Mpmu,
        nexus_soc::ProviderKind::Apbc2,
        nexus_soc::ProviderKind::Pll,
        nexus_soc::ProviderKind::Pinctrl,
    ];
    KINDS.into_iter().zip(found).filter_map(|(k, w)| w.map(|w| (k, w)))
}

/// The display plane the tree names (TASK-0251 P2): a display controller and the HDMI encoder
/// it drives, by compatible — the board's. QEMU virt names neither; its display is the virtio
/// GPU (`VirtioDevices::gpu`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DisplayPlane {
    pub controller: Option<DeviceWindow>,
    pub encoder: Option<DeviceWindow>,
}

/// The display plane, from the tree (the first node of each compatible).
pub(crate) fn display_plane() -> DisplayPlane {
    let Some(fdt) = tree() else { return DisplayPlane::default() };
    DisplayPlane {
        controller: fdt.find_compatible(&["spacemit,dpu-online2"]).find_map(window_of),
        encoder: fdt.find_compatible(&["spacemit,hdmi"]).find_map(window_of),
    }
}

/// The real-time clock by compatible (QEMU virt: `google,goldfish-rtc`; the
/// board's RTC arrives with TASK-0245B).
pub(crate) fn rtc() -> Option<DeviceWindow> {
    let fdt = tree()?;
    fdt.find_compatible(&["google,goldfish-rtc"]).find_map(window_of)
}

/// The discovery marker: what init found and where the disk's line is — the
/// proof that every window and interrupt came from the tree (required by the
/// harness in every profile).
pub(crate) fn report(v: &VirtioDevices) {
    debug_write_bytes(b"init: devices from fdt ok (virtio=");
    debug_write_hex(v.transports);
    debug_write_bytes(b" blk_irq=");
    debug_write_hex(v.blk[0].map_or(0, |w| w.irq as usize));
    debug_write_bytes(b" gpu_irq=");
    debug_write_hex(v.gpu.map_or(0, |w| w.irq as usize));
    debug_write_bytes(b" input_irqs=");
    for w in v.input.iter() {
        debug_write_hex(w.map_or(0, |w| w.irq as usize));
        debug_write_bytes(b",");
    }
    debug_write_bytes(b" rtc=");
    debug_write_hex(rtc().map_or(0, |w| w.base));
    debug_write_bytes(b")\n");
}
