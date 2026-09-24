// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a bus master's DMA reach, read from the tree (RFC-0098 C4, TASK-0246 P1).
//! A device on a bus with `dma-ranges` can address only the windows that property
//! names, and the address it is programmed with (`bus`) may differ from the physical
//! address (`cpu`) — the K1's storage bus reaches only the first 2 GiB, its multimedia
//! bus reaches the upper bank through a translated window. The windows are composed
//! level by level up the tree: an empty `dma-ranges` is the identity at that level, an
//! ABSENT one ends the walk (nothing above constrains the device), and a device with no
//! `dma-ranges` above it at all reaches every physical address, identity
//! ([`DmaReach::All`]). Bounded: at most [`MAX_DMA_WINDOWS`] windows at any level and
//! after composition — more is a typed error, never a silent truncation.

use crate::header::Error;
use crate::node::{cells, Node};

/// Windows a device's reach may carry (the K1's widest bus has three).
pub const MAX_DMA_WINDOWS: usize = 4;

/// Device-visible `bus .. bus + size` is physical `cpu .. cpu + size`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaWindow {
    pub bus: u64,
    pub cpu: u64,
    pub size: u64,
}

/// What a bus master can address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaReach {
    /// No `dma-ranges` above the device: every physical address, identity.
    All,
    /// Only these windows (in the order the tree lists them).
    Windows { windows: [DmaWindow; MAX_DMA_WINDOWS], count: usize },
}

impl DmaReach {
    /// The windows (none for [`DmaReach::All`]).
    pub fn windows(&self) -> &[DmaWindow] {
        match self {
            DmaReach::All => &[],
            DmaReach::Windows { windows, count } => &windows[..*count],
        }
    }
}

type Level = ([DmaWindow; MAX_DMA_WINDOWS], usize);

impl Node<'_> {
    /// This device's DMA reach: the `dma-ranges` of the buses above it, composed.
    pub fn dma_reach(&self) -> Result<DmaReach, Error> {
        let mut level = self.parent();
        let mut composed: Option<Level> = None;
        while let Some(bus) = level {
            match bus.prop("dma-ranges") {
                None => break,
                Some([]) => {}
                Some(v) => {
                    let parsed = parse_level(bus, v)?;
                    composed = Some(match composed {
                        None => parsed,
                        Some((prev, n)) => compose(&prev[..n], &parsed.0[..parsed.1])?,
                    });
                }
            }
            level = bus.parent();
        }
        Ok(match composed {
            None => DmaReach::All,
            Some((windows, count)) => DmaReach::Windows { windows, count },
        })
    }
}

/// One bus's `dma-ranges`: (child address in the bus's cells, parent address in its
/// parent's cells, size in the bus's size cells) triplets.
fn parse_level(bus: Node<'_>, v: &[u8]) -> Result<Level, Error> {
    let cac = bus.address_cells();
    let sc = bus.size_cells();
    let pac = bus.parent().map_or(2, |p| p.address_cells());
    let entry = (cac + pac + sc) * 4;
    if entry == 0 || v.len() % entry != 0 {
        return Err(Error::ShortProp);
    }
    let mut out = ([DmaWindow::default(); MAX_DMA_WINDOWS], 0usize);
    for chunk in v.chunks(entry) {
        let window = DmaWindow {
            bus: cells(&chunk[..cac * 4]),
            cpu: cells(&chunk[cac * 4..(cac + pac) * 4]),
            size: cells(&chunk[(cac + pac) * 4..]),
        };
        if window.size == 0
            || window.bus.checked_add(window.size).is_none()
            || window.cpu.checked_add(window.size).is_none()
        {
            return Err(Error::DmaRanges);
        }
        let slot = out.0.get_mut(out.1).ok_or(Error::DmaRanges)?;
        *slot = window;
        out.1 += 1;
    }
    Ok(out)
}

/// Carry each window's CPU side (an address in THIS level's child space) through
/// the level's entries; a part no entry covers is out of reach and is dropped.
fn compose(prev: &[DmaWindow], level: &[DmaWindow]) -> Result<Level, Error> {
    let mut out = ([DmaWindow::default(); MAX_DMA_WINDOWS], 0usize);
    for w in prev {
        for l in level {
            let lo = w.cpu.max(l.bus);
            let hi = (w.cpu + w.size).min(l.bus + l.size);
            if lo < hi {
                let slot = out.0.get_mut(out.1).ok_or(Error::DmaRanges)?;
                *slot = DmaWindow {
                    bus: w.bus + (lo - w.cpu),
                    cpu: l.cpu + (lo - l.bus),
                    size: hi - lo,
                };
                out.1 += 1;
            }
        }
    }
    Ok(out)
}
