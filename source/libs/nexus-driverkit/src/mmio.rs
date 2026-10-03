// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The drivers' register bus (TASK-0251 P2, the MMIO seam): `nexus_hal::Bus` over the ABI's
//! mapped [`MmioWindow`] — the one implementation every driver shares instead of carrying its
//! own volatile copy. [`Mmio`] is one window addressed by register offset (a controller's
//! block); [`MmioSet`] is several windows addressed by absolute virtual address (the SoC glue's
//! provider windows, whose plans carry `base + offset`). A word outside every window reads as
//! all ones — the value of a bus where nothing answers — and a write there is dropped; the
//! callers' read-backs turn either into a named fault.

use nexus_abi::MmioWindow;
use nexus_hal::Bus;

/// What a read outside every window answers: nothing drives the bus.
pub const FLOATING: u32 = u32::MAX;

/// One register window, addressed by offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mmio(MmioWindow);

impl Mmio {
    pub const fn new(window: MmioWindow) -> Self {
        Mmio(window)
    }

    /// The window under the bus.
    pub const fn window(&self) -> MmioWindow {
        self.0
    }
}

impl Bus for Mmio {
    fn read(&self, addr: usize) -> u32 {
        self.0.read32(addr).unwrap_or(FLOATING)
    }

    fn write(&self, addr: usize, value: u32) {
        let _ = self.0.write32(addr, value);
    }
}

/// Up to `N` windows, addressed by absolute virtual address: a word belongs to the window
/// whose `base..base + len` holds it.
#[derive(Clone, Copy, Debug)]
pub struct MmioSet<const N: usize> {
    windows: [Option<MmioWindow>; N],
}

impl<const N: usize> Default for MmioSet<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MmioSet<N> {
    pub const fn new() -> Self {
        MmioSet { windows: [None; N] }
    }

    /// Add `window` in slot `index` (the caller's own numbering, e.g. a provider kind).
    pub fn set(&mut self, index: usize, window: MmioWindow) {
        if let Some(slot) = self.windows.get_mut(index) {
            *slot = Some(window);
        }
    }

    /// The window that holds the word at absolute `addr`, and the offset inside it.
    fn locate(&self, addr: usize) -> Option<(MmioWindow, usize)> {
        self.windows.iter().flatten().find_map(|w| {
            let offset = addr.checked_sub(w.base())?;
            w.holds(offset).then_some((*w, offset))
        })
    }
}

impl<const N: usize> Bus for MmioSet<N> {
    fn read(&self, addr: usize) -> u32 {
        self.locate(addr).and_then(|(w, off)| w.read32(off)).unwrap_or(FLOATING)
    }

    fn write(&self, addr: usize, value: u32) {
        if let Some((w, off)) = self.locate(addr) {
            let _ = w.write32(off, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On the host no window answers: every read floats, every write is dropped — and an
    /// address outside every window floats the same way on the OS.
    #[test]
    fn test_reject_a_word_outside_every_window() {
        let set: MmioSet<2> = MmioSet::new();
        assert_eq!(set.read(0x1000), FLOATING);
        set.write(0x1000, 1);
        assert_eq!(set.locate(0x1000), None);
    }
}
