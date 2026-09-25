// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the planner. It walks the host's root bus (device 0..32, the functions of a
//! multi-function device), records each function (ids, class, header, INTx), sizes every
//! memory BAR of an endpoint with its decoding off, and places the BARs in the host's
//! windows: largest first, each at the lowest free address aligned to its span, a 64-bit
//! BAR in the 64-bit window when there is one, a 32-bit BAR below 4 GiB. A span is the BAR
//! rounded up to whole pages, so no two functions ever share a page. The BARs are written,
//! memory decoding is turned on for every function that got one, INTx pins are routed.
//! Host bridges and PCI-to-PCI bridges are recorded and never programmed (a host bridge
//! keeps its decoding; a bridge is not crossed); I/O BARs are refused (no port space is driven);
//! a BAR the specification does not allow, or that finds no room, is refused by name and
//! the rest goes on. Bus mastering stays off — [`enable_bus_master`] is the grant's.
//! OWNERS: @runtime @drivers

use crate::config::*;
use crate::host::{PciHost, WindowKind};
use crate::Bdf;

/// Functions a plan holds.
pub const MAX_FUNCTIONS: usize = 32;
/// The page a span is made of.
pub const PAGE: u64 = 4096;
/// Base address registers of an endpoint.
pub const BARS: usize = 6;
/// Class code of a host bridge.
pub const HOST_BRIDGE: u16 = 0x0600;

/// A placed memory BAR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    /// Bytes the function decodes (a power of two, at least 16).
    pub size: u64,
    /// The address the function decodes, in PCI space.
    pub pci: u64,
    /// The same address as the CPU sees it.
    pub cpu: u64,
    /// The bytes a capability for it covers: whole pages that hold nothing else.
    pub span: u64,
    /// A 64-bit BAR (it uses the next index too).
    pub is64: bool,
    /// Prefetchable memory.
    pub prefetchable: bool,
}

/// Why a BAR was not placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// An I/O BAR: no function is given port space.
    Io,
    /// A size the specification does not allow (not a power of two, below 16 bytes), or a
    /// 64-bit BAR in the last register.
    BadSize,
    /// No window of its kind has room.
    NoRoom,
}

/// One function of the root bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Function {
    /// Where it answers.
    pub bdf: Bdf,
    /// Vendor id.
    pub vendor: u16,
    /// Device id.
    pub device: u16,
    /// Base class [23:16], subclass [15:8], programming interface [7:0].
    pub class: u32,
    /// A PCI-to-PCI bridge (recorded, not crossed, not programmed).
    pub bridge: bool,
    /// The placed BARs by register index.
    pub bars: [Option<Bar>; BARS],
    /// The refused BARs by register index.
    pub refused: [Option<Refusal>; BARS],
    /// The INTx pin (0: none, 1..=4: INTA..INTD).
    pub pin: u8,
    /// The interrupt line the pin routes to (0: no pin, or no route in the map).
    pub irq: u32,
}

impl Function {
    /// Base class and subclass (`0x0805`: an SD host controller).
    pub fn class_code(&self) -> u16 {
        (self.class >> 8) as u16
    }
}

/// What the planner found and did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    functions: [Option<Function>; MAX_FUNCTIONS],
    count: usize,
}

impl Plan {
    /// The functions in bus order.
    pub fn functions(&self) -> impl Iterator<Item = &Function> {
        self.functions[..self.count].iter().flatten()
    }

    /// The functions of one class code (base class and subclass).
    pub fn of_class(&self, code: u16) -> impl Iterator<Item = &Function> {
        self.functions().filter(move |f| f.class_code() == code)
    }

    /// How many functions answered.
    pub fn len(&self) -> usize {
        self.count
    }

    /// True when nothing answered.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Why no plan was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// More functions answered than a plan holds.
    TooManyFunctions,
}

/// A BAR waiting for its place.
#[derive(Clone, Copy)]
struct Want {
    function: usize,
    index: usize,
    size: u64,
    is64: bool,
    prefetchable: bool,
}

fn span_of(size: u64) -> u64 {
    size.max(PAGE)
}

/// Enumerate the host's root bus, place every memory BAR, turn memory decoding on, route
/// INTx. Deterministic: the same host and the same functions give the same plan.
pub fn plan<C: ConfigSpace>(host: &PciHost, cfg: &C) -> Result<Plan, PlanError> {
    let mut plan = Plan { functions: [None; MAX_FUNCTIONS], count: 0 };
    let mut wants = [None::<Want>; MAX_FUNCTIONS * BARS];
    let mut nwants = 0;
    for dev in 0..32u8 {
        let first = Bdf { bus: host.first_bus, dev, func: 0 };
        if cfg.read(first, ID) & 0xFFFF == 0xFFFF {
            continue;
        }
        let multi = (cfg.read(first, HEADER) >> 16) & 0x80 != 0;
        for func in 0..if multi { 8 } else { 1 } {
            let bdf = Bdf { bus: host.first_bus, dev, func };
            let id = cfg.read(bdf, ID);
            if id & 0xFFFF == 0xFFFF {
                continue;
            }
            let slot = plan.functions.get_mut(plan.count).ok_or(PlanError::TooManyFunctions)?;
            let header = (cfg.read(bdf, HEADER) >> 16) & 0x7F;
            let pin = ((cfg.read(bdf, INTERRUPT) >> 8) & 0xFF) as u8;
            let mut function = Function {
                bdf,
                vendor: id as u16,
                device: (id >> 16) as u16,
                class: cfg.read(bdf, CLASS) >> 8,
                bridge: header == 1,
                bars: [None; BARS],
                refused: [None; BARS],
                pin,
                irq: host.route(bdf, pin).unwrap_or(0),
            };
            // Host bridges keep their decoding (some stop forwarding without it) and
            // bridges are not crossed: neither is sized or programmed.
            let host_bridge = function.class_code() == HOST_BRIDGE;
            if header == 0 && !host_bridge {
                for want in size_bars(cfg, &mut function) {
                    wants[nwants] = Some(Want { function: plan.count, ..want });
                    nwants += 1;
                }
            }
            *slot = Some(function);
            plan.count += 1;
        }
    }
    place(host, &mut plan, &mut wants[..nwants]);
    for function in plan.functions[..plan.count].iter().flatten() {
        if function.bars.iter().any(Option::is_some) {
            program(cfg, function);
        }
    }
    Ok(plan)
}

/// Size the six BARs of an endpoint with its decoding off; the memory ones come back as
/// wants, the others are refused on the function.
fn size_bars<C: ConfigSpace>(cfg: &C, f: &mut Function) -> impl Iterator<Item = Want> {
    let mut wants = [None::<Want>; BARS];
    let command = cfg.read(f.bdf, COMMAND) & 0xFFFF;
    cfg.write(f.bdf, COMMAND, command & !(CMD_IO | CMD_MEMORY | CMD_MASTER));
    let probe = |reg: u16| {
        let original = cfg.read(f.bdf, reg);
        cfg.write(f.bdf, reg, u32::MAX);
        let mask = cfg.read(f.bdf, reg);
        cfg.write(f.bdf, reg, original);
        mask
    };
    let mut i = 0;
    while i < BARS {
        let reg = BAR0 + 4 * i as u16;
        let mask = probe(reg);
        if mask == 0 {
            i += 1;
            continue;
        }
        if mask & 1 != 0 {
            f.refused[i] = Some(Refusal::Io);
            i += 1;
            continue;
        }
        let is64 = (mask >> 1) & 3 == 2;
        let prefetchable = mask & 8 != 0;
        let size = if is64 {
            if i + 1 == BARS {
                f.refused[i] = Some(Refusal::BadSize);
                break;
            }
            let high = u64::from(probe(reg + 4));
            (!((high << 32) | u64::from(mask & !0xF))).wrapping_add(1)
        } else {
            u64::from((!(mask & !0xF)).wrapping_add(1))
        };
        if size.is_power_of_two() && size >= 16 {
            wants[i] = Some(Want { function: 0, index: i, size, is64, prefetchable });
        } else {
            f.refused[i] = Some(Refusal::BadSize);
        }
        i += if is64 { 2 } else { 1 };
    }
    wants.into_iter().flatten()
}

/// Largest span first (bus order breaks ties — the order is total, so the unstable sort
/// is deterministic), lowest aligned address in its window.
fn place(host: &PciHost, plan: &mut Plan, wants: &mut [Option<Want>]) {
    wants.sort_unstable_by(|a, b| match (a, b) {
        (Some(a), Some(b)) => span_of(b.size)
            .cmp(&span_of(a.size))
            .then(a.function.cmp(&b.function))
            .then(a.index.cmp(&b.index)),
        _ => core::cmp::Ordering::Equal,
    });
    let mut cursor32 = host.window(WindowKind::Mem32).map(|w| w.pci);
    let mut cursor64 = host.window(WindowKind::Mem64).map(|w| w.pci);
    for want in wants.iter().flatten() {
        let span = span_of(want.size);
        let mut placed = None;
        // A 64-bit BAR prefers the 64-bit window and may live below 4 GiB; a 32-bit one
        // lives below 4 GiB only.
        let tries: &[(WindowKind, bool)] = if want.is64 {
            &[(WindowKind::Mem64, true), (WindowKind::Mem32, false)]
        } else {
            &[(WindowKind::Mem32, false)]
        };
        for &(kind, wide) in tries {
            let (Some(window), cursor) =
                (host.window(kind), if wide { &mut cursor64 } else { &mut cursor32 })
            else {
                continue;
            };
            let Some(next) = cursor.as_mut() else { continue };
            let at = next.div_ceil(span) * span;
            let end = at.checked_add(span);
            let fits =
                end.is_some_and(|e| e <= window.pci + window.size && (want.is64 || e <= 1 << 32));
            if fits {
                *next = at + span;
                placed = Some(Bar {
                    size: want.size,
                    pci: at,
                    cpu: window.cpu + (at - window.pci),
                    span,
                    is64: want.is64,
                    prefetchable: want.prefetchable,
                });
                break;
            }
        }
        let Some(Some(function)) = plan.functions.get_mut(want.function) else { continue };
        match placed {
            Some(bar) => function.bars[want.index] = Some(bar),
            None => function.refused[want.index] = Some(Refusal::NoRoom),
        }
    }
}

/// Write the placed BARs and turn memory decoding on (and nothing else).
fn program<C: ConfigSpace>(cfg: &C, f: &Function) {
    for (i, bar) in f.bars.iter().enumerate() {
        let Some(bar) = bar else { continue };
        let reg = BAR0 + 4 * i as u16;
        cfg.write(f.bdf, reg, bar.pci as u32);
        if bar.is64 {
            cfg.write(f.bdf, reg + 4, (bar.pci >> 32) as u32);
        }
    }
    cfg.write(f.bdf, COMMAND, CMD_MEMORY);
}

/// Let `bdf` master the bus: its memory decoding stays on, bus mastering joins it — the
/// one configuration write made for a function a DMA driver is granted.
pub fn enable_bus_master<C: ConfigSpace>(cfg: &C, bdf: Bdf) {
    cfg.write(bdf, COMMAND, CMD_MEMORY | CMD_MASTER);
}
