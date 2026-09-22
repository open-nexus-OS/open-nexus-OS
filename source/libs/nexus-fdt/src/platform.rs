// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the platform-level queries every consumer asks (RFC-0098 C3): memory
//! banks and reserved ranges, the harts with their timebase and ISA, the PLIC and
//! its S-mode contexts, the console, `/chosen`. Pure functions over the node API;
//! nothing here knows a machine by name.

use crate::header::{Error, Fdt};
use crate::node::{Node, Reg, StrList};

/// A `/memory@*` bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryBank {
    pub base: u64,
    pub size: u64,
}

/// A range no consumer may allocate: from the reservation block or a
/// `/reserved-memory` child. `no_map` = the producer asked for it to be left
/// unmapped as well (firmware, secure heaps); a `false` is a shared pool the
/// kernel may map but must not hand out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservedRange {
    pub base: u64,
    pub size: u64,
    pub no_map: bool,
}

/// `/cpus` as a whole.
#[derive(Clone, Copy)]
pub struct Cpus<'a> {
    node: Node<'a>,
    pub timebase_hz: u32,
}

/// One hart.
#[derive(Clone, Copy)]
pub struct Cpu<'a> {
    node: Node<'a>,
    pub hart: u32,
}

/// `/cpus/cpu-map`: which cluster a hart belongs to.
#[derive(Clone, Copy)]
pub struct CpuMap<'a> {
    node: Node<'a>,
}

/// `/chosen`.
#[derive(Clone, Copy)]
pub struct Chosen<'a> {
    node: Node<'a>,
}

impl<'a> Fdt<'a> {
    /// Every bank of every `device_type = "memory"` node, in tree order.
    pub fn memory_banks(&self) -> impl Iterator<Item = MemoryBank> + 'a {
        let fdt = *self;
        fdt.all_nodes()
            .filter(|n| n.prop_str("device_type") == Some("memory"))
            .flat_map(|n| RegIter { node: n, i: 0 })
            .map(|r| MemoryBank { base: r.addr, size: r.size })
    }

    /// Reservation-block entries (always `no_map`) followed by `/reserved-memory`
    /// children that carry a `reg`.
    pub fn reserved_ranges(&self) -> impl Iterator<Item = ReservedRange> + 'a {
        let fdt = *self;
        let block = fdt.reserved_entries().map(|e| ReservedRange {
            base: e.address,
            size: e.size,
            no_map: true,
        });
        let node = fdt.node_at_path("/reserved-memory");
        let children = node.into_iter().flat_map(|rm| rm.children()).flat_map(|c| {
            let no_map = c.prop("no-map").is_some();
            RegIter { node: c, i: 0 }.map(move |r| ReservedRange {
                base: r.addr,
                size: r.size,
                no_map,
            })
        });
        block.chain(children)
    }

    /// `/cpus`, refused when absent or without a timebase.
    pub fn cpus(&self) -> Result<Cpus<'a>, Error> {
        let node = self.node_at_path("/cpus").ok_or(Error::NotFound)?;
        let timebase_hz = node.prop_u32("timebase-frequency").ok_or(Error::NotFound)?;
        Ok(Cpus { node, timebase_hz })
    }

    /// `/chosen`, refused when absent.
    pub fn chosen(&self) -> Result<Chosen<'a>, Error> {
        self.node_at_path("/chosen").map(|node| Chosen { node }).ok_or(Error::NotFound)
    }

    /// The first enabled interrupt controller compatible with a PLIC.
    pub fn plic(&self) -> Option<Node<'a>> {
        self.find_compatible(&["riscv,plic0", "sifive,plic-1.0.0", "spacemit,k1-plic"])
            .find(|n| n.is_enabled())
    }

    /// The node `/chosen/stdout-path` names, `:baud` suffix and aliases resolved.
    pub fn stdout(&self) -> Option<Node<'a>> {
        let chosen = self.chosen().ok()?;
        self.node_at_path(chosen.stdout_path()?)
    }
}

/// Iterates a node's `reg` entries.
struct RegIter<'a> {
    node: Node<'a>,
    i: usize,
}

impl Iterator for RegIter<'_> {
    type Item = Reg;
    fn next(&mut self) -> Option<Reg> {
        let r = self.node.reg(self.i).ok()??;
        self.i += 1;
        Some(r)
    }
}

impl<'a> Cpus<'a> {
    /// Every `device_type = "cpu"` child with a `reg` (its hart id).
    pub fn harts(&self) -> impl Iterator<Item = Cpu<'a>> {
        self.node
            .children()
            .filter(|c| c.prop_str("device_type") == Some("cpu"))
            .filter_map(|c| c.prop_cell("reg", 0).map(|hart| Cpu { node: c, hart }))
    }

    pub fn count(&self) -> usize {
        self.harts().count()
    }

    /// `/cpus/cpu-map`, when the tree describes clusters.
    pub fn cpu_map(&self) -> Option<CpuMap<'a>> {
        self.node.children().find(|c| c.name() == "cpu-map").map(|node| CpuMap { node })
    }
}

impl<'a> Cpu<'a> {
    pub fn node(&self) -> Node<'a> {
        self.node
    }

    pub fn is_enabled(&self) -> bool {
        self.node.is_enabled()
    }

    /// `riscv,isa-extensions` (the modern list) — falls back to nothing on trees
    /// that only carry the legacy `riscv,isa` string.
    pub fn isa_extensions(&self) -> StrList<'a> {
        self.node.prop_strs("riscv,isa-extensions")
    }

    /// True when `name` is in `riscv,isa-extensions` or appears as a `_name`
    /// suffix of the legacy `riscv,isa` string.
    pub fn has_extension(&self, name: &str) -> bool {
        if self.isa_extensions().any(|e| e == name) {
            return true;
        }
        match self.node.prop_str("riscv,isa") {
            Some(isa) => isa.split('_').any(|part| part == name),
            None => false,
        }
    }

    /// `mmu-type` (`riscv,sv39` …).
    pub fn mmu(&self) -> Option<&'a str> {
        self.node.prop_str("mmu-type")
    }

    /// The hart's `interrupt-controller` child (`riscv,cpu-intc`) phandle.
    pub fn intc_phandle(&self) -> Option<u32> {
        self.node.children().find(|c| c.is_compatible("riscv,cpu-intc")).and_then(|c| c.phandle())
    }

    pub fn phandle(&self) -> Option<u32> {
        self.node.phandle()
    }
}

impl CpuMap<'_> {
    /// The cluster index (order of `clusterN` children) of the cpu with `phandle`.
    pub fn cluster_of(&self, cpu_phandle: u32) -> Option<u32> {
        self.node.children().enumerate().find_map(|(ci, cluster)| {
            cluster
                .children()
                .any(|core| core.prop_u32("cpu") == Some(cpu_phandle))
                .then_some(ci as u32)
        })
    }

    pub fn clusters(&self) -> usize {
        self.node.children().count()
    }
}

impl<'a> Chosen<'a> {
    pub fn stdout_path(&self) -> Option<&'a str> {
        self.node.prop_str("stdout-path")
    }

    pub fn bootargs(&self) -> Option<&'a str> {
        self.node.prop_str("bootargs")
    }

    /// `nexus,<name>` as a string (RFC-0098 C2).
    pub fn nexus_str(&self, name: &str) -> Option<&'a str> {
        let mut key = NexusKey::new(name)?;
        self.node.prop_str(key.as_str())
    }

    /// `nexus,<name>` as a u64.
    pub fn nexus_u64(&self, name: &str) -> Option<u64> {
        let mut key = NexusKey::new(name)?;
        self.node.prop_u64(key.as_str())
    }

    /// `nexus,<name>` as raw bytes (the measured boot record travels this way,
    /// RFC-0098 C2 / ADR-0059 v1 layout).
    pub fn nexus_bytes(&self, name: &str) -> Option<&'a [u8]> {
        let mut key = NexusKey::new(name)?;
        self.node.prop(key.as_str())
    }

    pub fn node(&self) -> Node<'a> {
        self.node
    }
}

/// `"nexus," + name` without an allocator: a fixed buffer, refused when the
/// name is longer than a property name should be.
pub(crate) struct NexusKey {
    buf: [u8; 48],
    len: usize,
}

impl NexusKey {
    pub(crate) fn new(name: &str) -> Option<Self> {
        const PREFIX: &[u8] = b"nexus,";
        if name.len() + PREFIX.len() > 48 {
            return None;
        }
        let mut buf = [0u8; 48];
        buf[..PREFIX.len()].copy_from_slice(PREFIX);
        buf[PREFIX.len()..PREFIX.len() + name.len()].copy_from_slice(name.as_bytes());
        Some(NexusKey { buf, len: PREFIX.len() + name.len() })
    }

    pub(crate) fn as_str(&mut self) -> &str {
        // Built from two `str`s, so it is valid UTF-8 by construction.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("nexus,")
    }
}

/// The S-mode external-interrupt context of each hart on a PLIC, from
/// `interrupts-extended` = `<&intc cause>` pairs: the pair index is the context
/// number, cause 9 is S-mode external, and the intc's parent is the hart.
pub struct PlicContexts<'a> {
    fdt: Fdt<'a>,
    prop: &'a [u8],
    i: usize,
}

impl<'a> Node<'a> {
    /// `(hart, context)` for every S-mode context this PLIC exposes.
    pub fn plic_s_contexts(&self) -> PlicContexts<'a> {
        PlicContexts { fdt: self.fdt, prop: self.prop("interrupts-extended").unwrap_or(&[]), i: 0 }
    }

    /// `riscv,ndev` — the number of interrupt sources.
    pub fn plic_ndev(&self) -> Option<u32> {
        self.prop_u32("riscv,ndev")
    }
}

impl Iterator for PlicContexts<'_> {
    type Item = (u32, u32);
    fn next(&mut self) -> Option<(u32, u32)> {
        const S_EXTERNAL: u32 = 9;
        loop {
            let off = self.i * 8;
            if self.prop.len() < off + 8 {
                return None;
            }
            let ctx = self.i as u32;
            self.i += 1;
            let p = &self.prop[off..off + 8];
            let intc = u32::from_be_bytes([p[0], p[1], p[2], p[3]]);
            let cause = u32::from_be_bytes([p[4], p[5], p[6], p[7]]);
            if cause != S_EXTERNAL {
                continue;
            }
            let hart = self
                .fdt
                .node_by_phandle(intc)
                .and_then(|n| n.parent())
                .and_then(|cpu| cpu.prop_cell("reg", 0));
            if let Some(hart) = hart {
                return Some((hart, ctx));
            }
        }
    }
}
