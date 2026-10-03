// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a device's register window, mapped (TASK-0251 P2, the MMIO seam). A driver
//! receives its device's window as a capability (RFC-0017: USER|RW, never exec, in whole
//! pages); `MmioWindow::map` maps it at a kernel-chosen address (`mmio_map_auto`, RFC-0085)
//! and every register access afterwards is a bounds-checked, 4-byte aligned volatile word —
//! the one place userspace touches device registers, so no driver carries its own `unsafe`
//! copy of it. A block whose registers start inside its page (the board's HDMI encoder at
//! `0xc040_0500`, the APMU at `0xd428_2800`) is a `window` of the page's window, at the
//! offset the tree's `reg` names. `nexus_driverkit::Mmio` puts the `nexus_hal::Bus` trait on
//! top. On the host there is no hardware: reads answer `None`, writes are refused.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! PUBLIC API: MmioWindow
//! TEST_COVERAGE: `tests` below (bounds, alignment, sub-windows); every QEMU and board lane
//!   through the drivers that map their windows here

/// One mapped register window: `len` bytes at virtual address `va`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmioWindow {
    va: usize,
    len: usize,
}

impl MmioWindow {
    /// Map `len` bytes of the window behind the device capability `cap`, from `offset`.
    #[cfg(nexus_env = "os")]
    pub fn map(cap: u32, offset: usize, len: usize) -> crate::SysResult<Self> {
        if len == 0 {
            return Err(crate::AbiError::InvalidArgument);
        }
        let va = crate::mmio_map_auto(cap, offset, len)?;
        Ok(MmioWindow { va, len })
    }

    /// The window's virtual address (where its first register lives in this address space).
    pub const fn base(&self) -> usize {
        self.va
    }

    /// Its length in bytes.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// True for a window of no bytes (never made by `map`).
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The block inside this window at `offset`, `len` bytes long — `None` when it does not
    /// lie wholly inside (a register block's in-page offset, read from the tree's `reg`).
    pub const fn window(&self, offset: usize, len: usize) -> Option<MmioWindow> {
        match offset.checked_add(len) {
            Some(end) if end <= self.len && len > 0 => {
                Some(MmioWindow { va: self.va + offset, len })
            }
            _ => None,
        }
    }

    /// Whether a 4-byte register at `offset` lies inside the window and is aligned.
    pub const fn holds(&self, offset: usize) -> bool {
        offset % 4 == 0 && offset < self.len && self.len - offset >= 4
    }

    /// The register word at `offset`; `None` outside the window or unaligned.
    pub fn read32(&self, offset: usize) -> Option<u32> {
        if !self.holds(offset) {
            return None;
        }
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        {
            // SAFETY: `va..va + len` was mapped for the device capability by `map` (or is a
            // `window` of such a mapping) and stays mapped for the program's life; the word
            // lies inside it and is 4-byte aligned (checked above). A volatile read: the
            // device, not the compiler, decides the value.
            Some(unsafe { core::ptr::read_volatile((self.va + offset) as *const u32) })
        }
        #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
        {
            None
        }
    }

    /// Write `value` to the register at `offset`; false outside the window or unaligned.
    pub fn write32(&self, offset: usize, value: u32) -> bool {
        if !self.holds(offset) {
            return false;
        }
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        {
            // SAFETY: as in `read32`, a volatile write.
            unsafe { core::ptr::write_volatile((self.va + offset) as *mut u32, value) };
            true
        }
        #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
        {
            let _ = value;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MmioWindow;

    fn page() -> MmioWindow {
        MmioWindow { va: 0x4000_0000, len: 0x1000 }
    }

    #[test]
    fn a_block_inside_its_page_is_a_window_at_the_trees_offset() {
        let encoder = page().window(0x500, 0x200).unwrap();
        assert_eq!((encoder.base(), encoder.len()), (0x4000_0500, 0x200));
        assert!(encoder.holds(0x1fc) && encoder.holds(0x0));
    }

    #[test]
    fn test_reject_a_register_outside_or_unaligned() {
        let w = page().window(0x500, 0x200).unwrap();
        assert!(!w.holds(0x200), "one past the block");
        assert!(!w.holds(0x1fe), "unaligned");
        assert!(!w.holds(usize::MAX - 1), "no wrap");
        assert_eq!(w.read32(0x200), None);
        assert!(!w.write32(0x200, 1));
    }

    #[test]
    fn test_reject_a_window_that_leaves_its_parent() {
        assert_eq!(page().window(0xf00, 0x200), None);
        assert_eq!(page().window(0, 0), None, "a window of no bytes");
        assert_eq!(page().window(usize::MAX, 4), None, "no wrap");
    }
}
