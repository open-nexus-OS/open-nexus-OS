// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the read-only device tree a service receives from init (RFC-0098 C3):
//! a `VmoRo` alias of the tree the kernel was handed, pinned into the service's
//! declared `DeviceTree` slot. This maps it once, bounds the tree by its own
//! header inside the pages the kernel exposed, and hands out the bytes — the one
//! path `socd`, the proof harness and every later consumer share. Nothing here
//! parses: `nexus-fdt` does, on the returned slice.
//! OWNERS: @runtime
//! STATUS: Functional
//! PUBLIC API: map_read_only()
//! TEST_COVERAGE: every QEMU lane resolves its profile through this path

/// The largest tree any stage we boot from produces; a bigger claim is corrupt.
pub const MAX_DTB_LEN: usize = 1024 * 1024;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
const FDT_MAGIC: u32 = 0xd00d_feed;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
const PAGE: usize = 4096;

/// Map the tree behind `slot` read-only and return it, bounded by its header.
/// `None` = no alias in the slot (a foreign loader), a claim that is not a tree,
/// or a tree larger than the pages the kernel exposed. Each call maps anew;
/// callers cache the slice.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
pub fn map_read_only(slot: u32) -> Option<&'static [u8]> {
    let mut info = crate::CapQuery::default();
    crate::cap_query(slot, &mut info).ok()?;
    let pages = usize::try_from(info.len).ok()?;
    if pages == 0 || pages % PAGE != 0 || pages > MAX_DTB_LEN + PAGE {
        return None;
    }
    let flags = crate::page_flags::VALID | crate::page_flags::READ | crate::page_flags::USER;
    let va = crate::vm_map(slot, 0, pages, flags).ok()?;
    // SAFETY: `pages` bytes are mapped read-only at `va` for the rest of the
    // program (never unmapped); the header bounds the tree inside them.
    let head = unsafe { core::slice::from_raw_parts(va as *const u8, 8) };
    let magic = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let total = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if magic != FDT_MAGIC || total < 40 || total > pages {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, total) })
}

/// Host builds carry no tree.
#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
pub fn map_read_only(_slot: u32) -> Option<&'static [u8]> {
    None
}
