// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a device's DMA reach inside the kernel (RFC-0098 C4, TASK-0246 P1). A bus
//! master can address only the windows its bus's `dma-ranges` name, and the address it
//! is programmed with may differ from the physical one (the K1's storage bus reaches
//! only the first 2 GiB; its multimedia bus reaches the upper bank through a translated
//! window). init reads the reach from the tree and hands it to the kernel with the rest
//! of the device in ONE versioned descriptor (`device_cap_create`); the kernel keeps
//! one immutable record per register window, allocates a device's DMA memory within its
//! reach, and answers `vmo_runs` in that device's bus addresses. Pure — the descriptor
//! decode, the translation and the table run on host; `mm::devices` holds the live
//! table and `syscall/api` does the capability work.
//! NOT target-gated (pure logic); `mod mm` is riscv/none-only, hence the `#[path]`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable (descriptor version `DEVICE_DESC_VERSION`)
//! TEST_COVERAGE: `tests` below (decode reject matrix, translation, table rules);
//!   `KSELFTEST: vmo runs ok (…)` + `KSELFTEST: vmo reach ok (…)` on QEMU
//! INVARIANTS: a record is immutable once registered and a register window has at
//!   most one; windows never overlap (in CPU or bus space), so a translation is
//!   unique; a physical byte outside every window has no bus address — it is
//!   refused, never guessed.

/// Windows a reach may carry (the K1's widest bus has three).
pub const MAX_DMA_WINDOWS: usize = 4;
/// The descriptor layout this kernel reads.
pub const DEVICE_DESC_VERSION: u32 = 1;
/// Bytes of a descriptor: version, flags, base, len, irq, window count, the windows.
pub const DEVICE_DESC_BYTES: usize = 32 + 24 * MAX_DMA_WINDOWS;
/// Descriptor flag bit 0: the device does not snoop the CPU caches.
pub const DEVICE_DMA_NONCOHERENT: u32 = 1;
/// Device records the kernel keeps (one per register window).
pub const MAX_DEVICES: usize = 64;
/// Register windows are granted in whole pages.
const PAGE: u64 = 4096;

/// Device-visible `bus .. bus + size` is physical `cpu .. cpu + size`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaWindow {
    pub bus: u64,
    pub cpu: u64,
    pub size: u64,
}

/// What a device can address: no window means every physical address, identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaReach {
    windows: [DmaWindow; MAX_DMA_WINDOWS],
    count: usize,
}

/// Why a descriptor was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescError {
    /// Not exactly `DEVICE_DESC_BYTES`.
    Length,
    /// A layout this kernel does not speak.
    Version,
    /// A flag bit nobody defined.
    Flags,
    /// The register window is empty, unaligned or overflows.
    Mmio,
    /// A PLIC line the controller cannot have.
    Irq,
    /// More windows than `MAX_DMA_WINDOWS`, or a non-zero unused slot.
    WindowCount,
    /// An empty, overflowing or overlapping window.
    Window,
}

/// Why a run has no bus address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReachError {
    /// Some byte lies in no window.
    OutOfReach,
    /// More pieces than the answer holds.
    TooMany,
}

impl DmaReach {
    /// Every physical address, identity (no `dma-ranges` above the device).
    pub const ALL: Self =
        Self { windows: [DmaWindow { bus: 0, cpu: 0, size: 0 }; MAX_DMA_WINDOWS], count: 0 };

    /// A reach of these windows, validated and ordered by CPU address (the order
    /// the allocator tries them in — deterministic whatever the tree's order).
    pub fn new(windows: &[DmaWindow]) -> Result<Self, DescError> {
        if windows.len() > MAX_DMA_WINDOWS {
            return Err(DescError::WindowCount);
        }
        let mut reach = Self::ALL;
        for w in windows {
            let bus_end = w.bus.checked_add(w.size).ok_or(DescError::Window)?;
            let cpu_end = w.cpu.checked_add(w.size).ok_or(DescError::Window)?;
            if w.size == 0 {
                return Err(DescError::Window);
            }
            for o in reach.windows() {
                let overlaps = |a: u64, a_end: u64, b: u64, b_end: u64| a < b_end && b < a_end;
                if overlaps(w.cpu, cpu_end, o.cpu, o.cpu + o.size)
                    || overlaps(w.bus, bus_end, o.bus, o.bus + o.size)
                {
                    return Err(DescError::Window);
                }
            }
            reach.windows[reach.count] = *w;
            reach.count += 1;
        }
        reach.windows[..reach.count].sort_unstable_by_key(|w| w.cpu);
        Ok(reach)
    }

    /// True when the device reaches every physical address, identity.
    pub fn is_all(&self) -> bool {
        self.count == 0
    }

    /// The windows, by CPU address (none for [`DmaReach::ALL`]).
    pub fn windows(&self) -> &[DmaWindow] {
        &self.windows[..self.count]
    }

    /// Append the device-visible pieces of physical `pa .. pa + len` to
    /// `out[..*written]`, merging a piece into the previous one when the two are
    /// contiguous in bus space. Refused when a byte lies in no window or the
    /// answer is full.
    pub fn to_bus(
        &self,
        pa: u64,
        len: u64,
        out: &mut [(u64, u64)],
        written: &mut usize,
    ) -> Result<(), ReachError> {
        let end = pa.checked_add(len).ok_or(ReachError::OutOfReach)?;
        let mut at = pa;
        while at < end {
            let (bus, piece) = if self.is_all() {
                (at, end - at)
            } else {
                let w = self
                    .windows()
                    .iter()
                    .find(|w| w.cpu <= at && at < w.cpu + w.size)
                    .ok_or(ReachError::OutOfReach)?;
                (w.bus + (at - w.cpu), end.min(w.cpu + w.size) - at)
            };
            match written.checked_sub(1).map(|i| &mut out[i]) {
                Some(last) if last.0 + last.1 == bus => last.1 += piece,
                _ => {
                    let slot = out.get_mut(*written).ok_or(ReachError::TooMany)?;
                    *slot = (bus, piece);
                    *written += 1;
                }
            }
            at += piece;
        }
        Ok(())
    }

    /// The CPU ranges `(start, end)` the device reaches, ascending — where its DMA
    /// memory may come from. Empty for [`DmaReach::ALL`] (any frame will do).
    pub fn cpu_ranges(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.windows().iter().map(|w| (w.cpu, w.cpu + w.size))
    }
}

/// A device as init describes it and the kernel keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDesc {
    /// The register window (physical, page-aligned).
    pub base: u64,
    pub len: u64,
    /// The PLIC line (0 = none).
    pub irq: u32,
    /// The device does not snoop the CPU caches.
    pub noncoherent: bool,
    /// What its DMA can address.
    pub reach: DmaReach,
}

fn le_u32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn le_u64(b: &[u8], at: usize) -> u64 {
    u64::from(le_u32(b, at)) | (u64::from(le_u32(b, at + 4)) << 32)
}

/// Decode and validate a descriptor (`nexus_abi::DeviceDesc`, little endian):
/// `version: u32`, `flags: u32`, `base: u64`, `len: u64`, `irq: u32`,
/// `dma_count: u32`, then `MAX_DMA_WINDOWS` × (`bus`, `cpu`, `size`: u64) with the
/// slots past `dma_count` zero. Deny-by-default: every field is checked.
pub fn decode_desc(bytes: &[u8], max_irq: u32) -> Result<DeviceDesc, DescError> {
    if bytes.len() != DEVICE_DESC_BYTES {
        return Err(DescError::Length);
    }
    if le_u32(bytes, 0) != DEVICE_DESC_VERSION {
        return Err(DescError::Version);
    }
    let flags = le_u32(bytes, 4);
    if flags & !DEVICE_DMA_NONCOHERENT != 0 {
        return Err(DescError::Flags);
    }
    let (base, len) = (le_u64(bytes, 8), le_u64(bytes, 16));
    if len == 0 || base % PAGE != 0 || len % PAGE != 0 || base.checked_add(len).is_none() {
        return Err(DescError::Mmio);
    }
    let irq = le_u32(bytes, 24);
    if irq > max_irq {
        return Err(DescError::Irq);
    }
    let count = le_u32(bytes, 28) as usize;
    if count > MAX_DMA_WINDOWS {
        return Err(DescError::WindowCount);
    }
    let mut windows = [DmaWindow::default(); MAX_DMA_WINDOWS];
    for (i, w) in windows.iter_mut().enumerate() {
        let at = 32 + i * 24;
        *w = DmaWindow {
            bus: le_u64(bytes, at),
            cpu: le_u64(bytes, at + 8),
            size: le_u64(bytes, at + 16),
        };
        if i >= count && *w != DmaWindow::default() {
            return Err(DescError::WindowCount);
        }
    }
    let reach = DmaReach::new(&windows[..count])?;
    Ok(DeviceDesc { base, len, irq, noncoherent: flags & DEVICE_DMA_NONCOHERENT != 0, reach })
}

/// A record's handle: the table index plus one (never 0).
pub type DeviceId = u16;

/// Why the table refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    /// A different description of a window that already has one.
    Conflict,
    /// `MAX_DEVICES` records.
    Full,
    /// No record has this id.
    NoSuchDevice,
}

/// The kernel's devices: one immutable record per register window.
pub struct DeviceTable {
    records: [Option<DeviceDesc>; MAX_DEVICES],
}

impl DeviceTable {
    /// An empty table.
    pub const fn new() -> Self {
        Self { records: [None; MAX_DEVICES] }
    }

    /// The record for `desc`: the existing one when the window is already
    /// described identically (init probes a window, then grants it), a refusal when
    /// it is described differently, a new one otherwise.
    pub fn register(&mut self, desc: DeviceDesc) -> Result<DeviceId, TableError> {
        if let Some(i) = self.records.iter().position(|r| r.is_some_and(|r| r.base == desc.base)) {
            return if self.records[i] == Some(desc) {
                Ok(i as DeviceId + 1)
            } else {
                Err(TableError::Conflict)
            };
        }
        let free = self.records.iter().position(Option::is_none).ok_or(TableError::Full)?;
        self.records[free] = Some(desc);
        Ok(free as DeviceId + 1)
    }

    /// The record behind `id`.
    pub fn get(&self, id: DeviceId) -> Option<&DeviceDesc> {
        self.records.get(usize::from(id).checked_sub(1)?)?.as_ref()
    }

    /// Drop a record the caller has proven no capability names (the kernel
    /// selftest's synthetic devices); a real device is never retired.
    pub fn retire(&mut self, id: DeviceId) -> Result<(), TableError> {
        let slot = usize::from(id).checked_sub(1).and_then(|i| self.records.get_mut(i));
        match slot {
            Some(record @ Some(_)) => {
                *record = None;
                Ok(())
            }
            _ => Err(TableError::NoSuchDevice),
        }
    }
}

impl Default for DeviceTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    fn window(bus: u64, cpu: u64, size: u64) -> DmaWindow {
        DmaWindow { bus, cpu, size }
    }

    fn desc_bytes(
        version: u32,
        flags: u32,
        base: u64,
        len: u64,
        irq: u32,
        windows: &[DmaWindow],
    ) -> [u8; DEVICE_DESC_BYTES] {
        let mut b = [0u8; DEVICE_DESC_BYTES];
        b[0..4].copy_from_slice(&version.to_le_bytes());
        b[4..8].copy_from_slice(&flags.to_le_bytes());
        b[8..16].copy_from_slice(&base.to_le_bytes());
        b[16..24].copy_from_slice(&len.to_le_bytes());
        b[24..28].copy_from_slice(&irq.to_le_bytes());
        b[28..32].copy_from_slice(&(windows.len() as u32).to_le_bytes());
        for (i, w) in windows.iter().enumerate() {
            let at = 32 + i * 24;
            b[at..at + 8].copy_from_slice(&w.bus.to_le_bytes());
            b[at + 8..at + 16].copy_from_slice(&w.cpu.to_le_bytes());
            b[at + 16..at + 24].copy_from_slice(&w.size.to_le_bytes());
        }
        b
    }

    /// The board's storage bus and multimedia bus, as init describes them.
    fn storage() -> DmaReach {
        DmaReach::new(&[window(0, 0, 2 * GIB)]).unwrap()
    }
    fn multimedia() -> DmaReach {
        DmaReach::new(&[window(2 * GIB, 4 * GIB, 14 * GIB), window(0, 0, 2 * GIB)]).unwrap()
    }

    #[test]
    fn the_emmc_host_decodes_with_its_line_coherence_and_reach() {
        let b = desc_bytes(
            1,
            DEVICE_DMA_NONCOHERENT,
            0xd428_1000,
            0x1000,
            101,
            &[window(0, 0, 2 * GIB)],
        );
        let d = decode_desc(&b, 159).unwrap();
        assert_eq!((d.base, d.len, d.irq, d.noncoherent), (0xd428_1000, 0x1000, 101, true));
        assert_eq!(d.reach, storage());
        let all = decode_desc(&desc_bytes(1, 0, 0x4000_1000, 0x1000, 1, &[]), 53).unwrap();
        assert!(all.reach.is_all() && !all.noncoherent);
    }

    #[test]
    fn test_reject_device_desc_malformed() {
        let ok = desc_bytes(1, 0, 0x1000, 0x1000, 1, &[]);
        assert_eq!(decode_desc(&ok[..DEVICE_DESC_BYTES - 1], 53), Err(DescError::Length));
        assert_eq!(
            decode_desc(&desc_bytes(2, 0, 0x1000, 0x1000, 1, &[]), 53),
            Err(DescError::Version)
        );
        assert_eq!(
            decode_desc(&desc_bytes(1, 2, 0x1000, 0x1000, 1, &[]), 53),
            Err(DescError::Flags)
        );
        assert_eq!(
            decode_desc(&desc_bytes(1, 0, 0x1001, 0x1000, 1, &[]), 53),
            Err(DescError::Mmio)
        );
        assert_eq!(decode_desc(&desc_bytes(1, 0, 0x1000, 0, 1, &[]), 53), Err(DescError::Mmio));
        assert_eq!(
            decode_desc(&desc_bytes(1, 0, u64::MAX - 0xfff, 0x2000, 1, &[]), 53),
            Err(DescError::Mmio)
        );
        assert_eq!(
            decode_desc(&desc_bytes(1, 0, 0x1000, 0x1000, 54, &[]), 53),
            Err(DescError::Irq)
        );
    }

    #[test]
    fn test_reject_device_desc_bad_windows() {
        // Too many, a stray non-zero slot past the count, empty, overflowing, overlapping.
        let mut five = desc_bytes(1, 0, 0x1000, 0x1000, 1, &[window(0, 0, 1)]);
        five[28..32].copy_from_slice(&5u32.to_le_bytes());
        assert_eq!(decode_desc(&five, 53), Err(DescError::WindowCount));
        let mut stray = desc_bytes(1, 0, 0x1000, 0x1000, 1, &[]);
        stray[32 + 8] = 1;
        assert_eq!(decode_desc(&stray, 53), Err(DescError::WindowCount));
        assert_eq!(DmaReach::new(&[window(0, 0, 0)]), Err(DescError::Window));
        assert_eq!(DmaReach::new(&[window(u64::MAX, 0, 2)]), Err(DescError::Window));
        let cpu_overlap = [window(0, 0, GIB), window(4 * GIB, GIB / 2, GIB)];
        assert_eq!(DmaReach::new(&cpu_overlap), Err(DescError::Window));
        let bus_overlap = [window(0, 0, GIB), window(GIB / 2, 4 * GIB, GIB)];
        assert_eq!(DmaReach::new(&bus_overlap), Err(DescError::Window));
    }

    #[test]
    fn a_run_in_reach_translates_and_merges_in_bus_space() {
        let mut out = [(0u64, 0u64); 4];
        let mut n = 0;
        // Identity: the storage bus hands back the physical address (below 2 GiB).
        storage().to_bus(0x4040_0000, 0x3000, &mut out, &mut n).unwrap();
        storage().to_bus(0x4040_3000, 0x1000, &mut out, &mut n).unwrap();
        assert_eq!(out[..n], [(0x4040_0000, 0x4000)]);
        // QEMU's first frames (2 GiB + 4 MiB) are beyond a storage master's reach.
        let mut n = 0;
        assert_eq!(
            storage().to_bus(0x8040_0000, 0x1000, &mut out, &mut n),
            Err(ReachError::OutOfReach)
        );
        // Translated: 4 GiB + 1 MiB in the upper bank is bus 2 GiB + 1 MiB.
        let mut n = 0;
        multimedia().to_bus(4 * GIB + 0x10_0000, 0x2000, &mut out, &mut n).unwrap();
        assert_eq!(out[..n], [(2 * GIB + 0x10_0000, 0x2000)]);
        // Everything, identity.
        let mut n = 0;
        DmaReach::ALL.to_bus(5 * GIB, 0x1000, &mut out, &mut n).unwrap();
        assert_eq!(out[..n], [(5 * GIB, 0x1000)]);
    }

    #[test]
    fn test_reject_a_run_out_of_reach() {
        let mut out = [(0u64, 0u64); 4];
        let mut n = 0;
        // The upper bank is unreachable for a storage master.
        assert_eq!(
            storage().to_bus(4 * GIB, 0x1000, &mut out, &mut n),
            Err(ReachError::OutOfReach)
        );
        // A run that starts in reach and leaves it is refused as a whole.
        let mut n = 0;
        assert_eq!(
            storage().to_bus(2 * GIB - 0x1000, 0x2000, &mut out, &mut n),
            Err(ReachError::OutOfReach)
        );
        // Two windows that are adjacent in CPU space but not in bus space: two pieces.
        let split = DmaReach::new(&[window(4 * GIB, 0, GIB), window(0, GIB, GIB)]).unwrap();
        let mut n = 0;
        split.to_bus(GIB - 0x1000, 0x2000, &mut out, &mut n).unwrap();
        assert_eq!(out[..n], [(5 * GIB - 0x1000, 0x1000), (0, 0x1000)]);
        let mut one = [(0u64, 0u64); 1];
        let mut n = 0;
        assert_eq!(split.to_bus(GIB - 0x1000, 0x2000, &mut one, &mut n), Err(ReachError::TooMany));
    }

    #[test]
    fn windows_are_ordered_by_cpu_address_for_the_allocator() {
        assert_eq!(
            multimedia().cpu_ranges().collect::<Vec<_>>(),
            [(0, 2 * GIB), (4 * GIB, 18 * GIB)]
        );
        assert_eq!(DmaReach::ALL.cpu_ranges().count(), 0);
    }

    #[test]
    fn a_window_is_registered_once_and_probing_it_again_finds_the_same_record() {
        let mut table = DeviceTable::new();
        let emmc = DeviceDesc {
            base: 0xd428_1000,
            len: 0x1000,
            irq: 101,
            noncoherent: true,
            reach: storage(),
        };
        let id = table.register(emmc).unwrap();
        assert_eq!(table.register(emmc), Ok(id));
        assert_eq!(table.get(id), Some(&emmc));
        let other = DeviceDesc { base: 0xd428_0000, ..emmc };
        assert_ne!(table.register(other).unwrap(), id);
    }

    #[test]
    fn test_reject_a_second_description_of_one_window() {
        let mut table = DeviceTable::new();
        let emmc = DeviceDesc {
            base: 0xd428_1000,
            len: 0x1000,
            irq: 101,
            noncoherent: true,
            reach: storage(),
        };
        table.register(emmc).unwrap();
        let wider = DeviceDesc { reach: DmaReach::ALL, ..emmc };
        assert_eq!(table.register(wider), Err(TableError::Conflict));
    }

    #[test]
    fn test_reject_a_full_table_and_unknown_ids() {
        let mut table = DeviceTable::new();
        for i in 0..MAX_DEVICES as u64 {
            let d = DeviceDesc {
                base: (i + 1) * 0x1000,
                len: 0x1000,
                irq: 0,
                noncoherent: false,
                reach: DmaReach::ALL,
            };
            table.register(d).unwrap();
        }
        let one_more = DeviceDesc {
            base: 0x100_0000,
            len: 0x1000,
            irq: 0,
            noncoherent: false,
            reach: DmaReach::ALL,
        };
        assert_eq!(table.register(one_more), Err(TableError::Full));
        assert_eq!(table.get(0), None);
        assert_eq!(table.get(MAX_DEVICES as DeviceId + 1), None);
        assert_eq!(table.retire(0), Err(TableError::NoSuchDevice));
        table.retire(1).unwrap();
        assert_eq!(table.retire(1), Err(TableError::NoSuchDevice));
        assert!(table.register(one_more).is_ok(), "a retired slot is reused");
    }
}
