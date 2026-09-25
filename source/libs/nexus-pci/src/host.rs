// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a generic ECAM host as the tree describes it (the PCI binding): the ECAM window
//! (`reg`, translated to the CPU), the bus range, the windows the host forwards (`ranges`:
//! I/O, 32-bit and 64-bit memory, each a PCI and a CPU address), the INTx routes
//! (`interrupt-map` under `interrupt-map-mask`, each resolved to its parent's first
//! interrupt cell — the PLIC line), and what the functions behind it inherit: coherence and
//! DMA reach. Every property is checked; a malformed host is refused by name.
//! OWNERS: @runtime @drivers

use nexus_fdt::{DmaReach, Fdt, Node};

use crate::config::BUS_BYTES;
use crate::Bdf;

/// Windows a host holds.
pub const MAX_WINDOWS: usize = 4;
/// INTx routes a host holds (QEMU virt: 16).
pub const MAX_ROUTES: usize = 64;

/// The address space a window forwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// I/O ports (not driven: no function is given port space).
    Io,
    /// Memory below 4 GiB.
    Mem32,
    /// Memory at 64-bit addresses.
    Mem64,
}

/// PCI addresses `pci .. pci + size` are CPU addresses `cpu .. cpu + size`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The space.
    pub kind: WindowKind,
    /// Prefetchable memory.
    pub prefetchable: bool,
    /// The first PCI address.
    pub pci: u64,
    /// The first CPU address.
    pub cpu: u64,
    /// Bytes.
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Route {
    unit: [u32; 3],
    pin: u32,
    line: u32,
}

/// Why a host node is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    /// Not the PCI binding (`#address-cells` 3, `#size-cells` 2, `#interrupt-cells` 1).
    Binding,
    /// No ECAM window (`reg`), or one the tree cannot translate.
    Ecam,
    /// A bus range out of order, or an ECAM window too small for it.
    BusRange,
    /// A `ranges` entry of the wrong length, of no forwarding space, empty or overflowing.
    Ranges,
    /// More windows than a host holds.
    TooManyWindows,
    /// An `interrupt-map` or `interrupt-map-mask` of the wrong length.
    InterruptMap,
    /// More routes than a host holds.
    TooManyRoutes,
    /// An interrupt parent the tree lacks, or one without `#interrupt-cells`.
    InterruptParent,
    /// The `dma-ranges` above the host are malformed.
    DmaRanges,
}

/// A generic ECAM host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciHost {
    /// The CPU address of the first bus's configuration space.
    pub ecam: u64,
    /// The first bus (the root bus).
    pub first_bus: u8,
    /// The last bus.
    pub last_bus: u8,
    /// The functions behind the host snoop the CPU caches.
    pub coherent: bool,
    /// What the functions behind the host can address by DMA.
    pub reach: DmaReach,
    windows: [Window; MAX_WINDOWS],
    nwindows: usize,
    mask: [u32; 4],
    routes: [Route; MAX_ROUTES],
    nroutes: usize,
}

fn be32(v: &[u8], cell: usize) -> Option<u32> {
    let b = v.get(cell * 4..cell * 4 + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn cells(v: &[u8], at: usize, n: usize) -> Option<u64> {
    (0..n).try_fold(0u64, |acc, i| Some((acc << 32) | u64::from(be32(v, at + i)?)))
}

impl PciHost {
    /// Read the host from its node (`compatible = "pci-host-ecam-generic"`).
    pub fn from_node(fdt: &Fdt<'_>, node: Node<'_>) -> Result<Self, HostError> {
        let binding = node.prop_u32("#address-cells") == Some(3)
            && node.prop_u32("#size-cells") == Some(2)
            && node.prop_u32("#interrupt-cells").unwrap_or(1) == 1;
        if !binding {
            return Err(HostError::Binding);
        }
        let ecam = node.reg(0).map_err(|_| HostError::Ecam)?.ok_or(HostError::Ecam)?;
        let (first, last) = match node.prop("bus-range") {
            None => (0, 255),
            Some(v) if v.len() == 8 => (be32(v, 0).unwrap_or(0), be32(v, 1).unwrap_or(0)),
            Some(_) => return Err(HostError::BusRange),
        };
        if first > last || last > 255 || ecam.size < (u64::from(last - first) + 1) * BUS_BYTES {
            return Err(HostError::BusRange);
        }
        let mut host = Self {
            ecam: ecam.addr,
            first_bus: first as u8,
            last_bus: last as u8,
            coherent: node.dma_coherent(),
            reach: node.child_dma_reach().map_err(|_| HostError::DmaRanges)?,
            windows: [Window { kind: WindowKind::Io, prefetchable: false, pci: 0, cpu: 0, size: 0 };
                MAX_WINDOWS],
            nwindows: 0,
            mask: [u32::MAX; 4],
            routes: [Route { unit: [0; 3], pin: 0, line: 0 }; MAX_ROUTES],
            nroutes: 0,
        };
        host.read_ranges(node)?;
        host.read_routes(fdt, node)?;
        Ok(host)
    }

    fn read_ranges(&mut self, node: Node<'_>) -> Result<(), HostError> {
        let v = node.prop("ranges").unwrap_or(&[]);
        let pac = node.parent().and_then(|p| p.prop_u32("#address-cells")).unwrap_or(2) as usize;
        let entry = 3 + pac + 2;
        if v.len() % (entry * 4) != 0 {
            return Err(HostError::Ranges);
        }
        for i in 0..v.len() / (entry * 4) {
            let at = i * entry;
            let hi = be32(v, at).ok_or(HostError::Ranges)?;
            let pci = cells(v, at + 1, 2).ok_or(HostError::Ranges)?;
            let parent = cells(v, at + 3, pac).ok_or(HostError::Ranges)?;
            let size = cells(v, at + 3 + pac, 2).ok_or(HostError::Ranges)?;
            let kind = match (hi >> 24) & 3 {
                1 => WindowKind::Io,
                2 => WindowKind::Mem32,
                3 => WindowKind::Mem64,
                _ => return Err(HostError::Ranges),
            };
            let cpu = node.cpu_address(parent).map_err(|_| HostError::Ranges)?;
            let fits = size > 0
                && pci.checked_add(size).is_some()
                && cpu.checked_add(size).is_some()
                && (kind != WindowKind::Mem32 || pci + size <= 1 << 32);
            if !fits {
                return Err(HostError::Ranges);
            }
            let slot = self.windows.get_mut(self.nwindows).ok_or(HostError::TooManyWindows)?;
            *slot = Window { kind, prefetchable: hi & (1 << 30) != 0, pci, cpu, size };
            self.nwindows += 1;
        }
        Ok(())
    }

    fn read_routes(&mut self, fdt: &Fdt<'_>, node: Node<'_>) -> Result<(), HostError> {
        let Some(map) = node.prop("interrupt-map") else { return Ok(()) };
        if let Some(m) = node.prop("interrupt-map-mask") {
            if m.len() != 16 {
                return Err(HostError::InterruptMap);
            }
            for (i, cell) in self.mask.iter_mut().enumerate() {
                *cell = be32(m, i).ok_or(HostError::InterruptMap)?;
            }
        }
        let mut at = 0usize;
        while at * 4 < map.len() {
            let child = [be32(map, at), be32(map, at + 1), be32(map, at + 2), be32(map, at + 3)];
            let [Some(u0), Some(u1), Some(u2), Some(pin)] = child else {
                return Err(HostError::InterruptMap);
            };
            let phandle = be32(map, at + 4).ok_or(HostError::InterruptMap)?;
            let parent = fdt.node_by_phandle(phandle).ok_or(HostError::InterruptParent)?;
            let pic =
                parent.prop_u32("#interrupt-cells").ok_or(HostError::InterruptParent)? as usize;
            let pac = parent.prop_u32("#address-cells").unwrap_or(0) as usize;
            if pic == 0 {
                return Err(HostError::InterruptParent);
            }
            let line = be32(map, at + 5 + pac).ok_or(HostError::InterruptMap)?;
            if (at + 5 + pac + pic) * 4 > map.len() {
                return Err(HostError::InterruptMap);
            }
            let slot = self.routes.get_mut(self.nroutes).ok_or(HostError::TooManyRoutes)?;
            *slot = Route { unit: [u0, u1, u2], pin, line };
            self.nroutes += 1;
            at += 5 + pac + pic;
        }
        Ok(())
    }

    /// The windows, in tree order.
    pub fn windows(&self) -> &[Window] {
        &self.windows[..self.nwindows]
    }

    /// The first window of `kind`.
    pub fn window(&self, kind: WindowKind) -> Option<Window> {
        self.windows().iter().find(|w| w.kind == kind).copied()
    }

    /// The interrupt line INTx `pin` (1..=4) of `bdf` routes to; `None` when the map has no
    /// entry for it.
    pub fn route(&self, bdf: Bdf, pin: u8) -> Option<u32> {
        if !(1..=4).contains(&pin) {
            return None;
        }
        let hi =
            (u32::from(bdf.bus) << 16) | (u32::from(bdf.dev) << 11) | (u32::from(bdf.func) << 8);
        let unit = [hi & self.mask[0], 0, 0];
        let pin = u32::from(pin) & self.mask[3];
        self.routes[..self.nroutes].iter().find(|r| r.unit == unit && r.pin == pin).map(|r| r.line)
    }

    /// The root bus's configuration space: its CPU address and bytes (what a caller maps).
    pub fn root_config(&self) -> (u64, u64) {
        (self.ecam, BUS_BYTES)
    }
}
