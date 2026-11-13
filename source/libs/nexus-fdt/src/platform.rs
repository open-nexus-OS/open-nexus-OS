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
    /// The hart id: `reg`, as wide as `/cpus` says (`#address-cells` of 1 or 2).
    pub hart: u64,
}

/// A cpu node's hart id: its `reg`, read with the `#address-cells` of its parent (`/cpus`).
/// Hart ids are as wide as the machine's registers and sparse on real SoCs; the first cell
/// alone is the id only on a tree with one address cell.
pub(crate) fn hart_id(cpu: &Node<'_>) -> Option<u64> {
    match cpu.parent()?.address_cells() {
        1 => cpu.prop_cell("reg", 0).map(u64::from),
        2 => cpu.prop_u64("reg"),
        _ => None,
    }
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

    /// `/cpus`, refused when absent or without a timebase. The timebase is `/cpus`'s
    /// `timebase-frequency`, or — on a tree that states it per hart — the first cpu node's.
    pub fn cpus(&self) -> Result<Cpus<'a>, Error> {
        let node = self.node_at_path("/cpus").ok_or(Error::NotFound)?;
        let per_cpu = || node.children().find_map(|c| c.prop_u32("timebase-frequency"));
        let timebase_hz =
            node.prop_u32("timebase-frequency").or_else(per_cpu).ok_or(Error::NotFound)?;
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
    /// Every enabled `device_type = "cpu"` child with a `reg` (its hart id). A cpu node
    /// with `status = "disabled"` is not a hart the OS may start: it stays out of the
    /// count, the mask and the ISA probes, the canonical way a tree pins a boot to fewer
    /// harts.
    pub fn harts(&self) -> impl Iterator<Item = Cpu<'a>> {
        self.node
            .children()
            .filter(|c| c.prop_str("device_type") == Some("cpu") && c.is_enabled())
            .filter_map(|c| hart_id(&c).map(|hart| Cpu { node: c, hart }))
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

    /// True when the hart states `name` by either binding: an entry of
    /// `riscv,isa-extensions`, or in the legacy `riscv,isa` string a `_name` part or — for a
    /// single letter — a letter after the base (`rv64imafdc`; `g` stands for `imafd`).
    pub fn has_extension(&self, name: &str) -> bool {
        if self.isa_extensions().any(|e| e == name) {
            return true;
        }
        let Some(isa) = self.node.prop_str("riscv,isa") else { return false };
        let mut parts = isa.split('_');
        let letters = parts.next().and_then(|base| base.get(4..)).unwrap_or("");
        let by_letter = name.len() == 1
            && (letters.contains(name) || (letters.contains('g') && "imafd".contains(name)));
        by_letter || parts.any(|part| part == name)
    }

    /// `mmu-type` (`riscv,sv39` …).
    pub fn mmu(&self) -> Option<&'a str> {
        self.node.prop_str("mmu-type")
    }

    /// Whether the hart can run a paged supervisor: it states an `mmu-type` other than
    /// `riscv,none`. A monitor core without an MMU is in the tree and enabled on some SoCs;
    /// it is not a hart an OS schedules on.
    pub fn has_mmu(&self) -> bool {
        self.mmu().is_some_and(|mmu| mmu != "riscv,none")
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
    /// The cluster of the cpu with `phandle`: the index, in tree order, of the innermost
    /// `cluster*` node that holds it. The binding nests — `socketN` above clusters, clusters
    /// inside clusters, `threadN` below a core — and a tree may use any of it.
    pub fn cluster_of(&self, cpu_phandle: u32) -> Option<u32> {
        let mut next = 0;
        find_cluster(self.node, Some(cpu_phandle), &mut next, 0)
    }

    /// The number of innermost clusters.
    pub fn clusters(&self) -> usize {
        let mut next = 0;
        let _ = find_cluster(self.node, None, &mut next, 0);
        next as usize
    }
}

fn named(node: &Node<'_>, prefix: &str) -> bool {
    node.name().starts_with(prefix)
}

/// Walks the innermost clusters below `node` in tree order, numbering them through `next`,
/// and answers the number of the one holding the cpu `phandle` (`None`: only count). The
/// depth is the tree's, which the parser bounds ([`crate::MAX_DEPTH`]).
fn find_cluster(node: Node<'_>, phandle: Option<u32>, next: &mut u32, depth: usize) -> Option<u32> {
    if depth > crate::MAX_DEPTH {
        return None;
    }
    let mut found = None;
    for child in node.children() {
        let nests = child.children().any(|c| named(&c, "cluster"));
        let hit = if named(&child, "socket") || (named(&child, "cluster") && nests) {
            find_cluster(child, phandle, next, depth + 1)
        } else if named(&child, "cluster") {
            let index = *next;
            *next += 1;
            let names = |n: &Node<'_>| phandle.is_some() && n.prop_u32("cpu") == phandle;
            let holds =
                child.children().any(|core| names(&core) || core.children().any(|t| names(&t)));
            holds.then_some(index)
        } else {
            None
        };
        // The walk goes on after a hit: the count comes from the same walk.
        found = found.or(hit);
    }
    found
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

    /// The boot LED (TASK-0260B P3): `nexus,boot-led = <&gpio bank line flags>` — the GPIO
    /// controller node and the line the loader and the kernel pulse their milestones on. `None`
    /// on a tree without one (QEMU), or one whose phandle names no node.
    pub fn boot_led(&self) -> Option<BootLed<'a>> {
        let mut key = NexusKey::new("boot-led")?;
        let key = key.as_str();
        let phandle = self.node.prop_cell(key, 0)?;
        let bank = self.node.prop_cell(key, 1)?;
        let line = self.node.prop_cell(key, 2)?;
        let flags = self.node.prop_cell(key, 3).unwrap_or(0);
        let gpio = self.node.fdt.node_by_phandle(phandle)?;
        Some(BootLed { gpio, bank, line, flags })
    }

    /// The boot LED's pad register and the value that muxes it as a GPIO
    /// (`nexus,boot-led-pad = <reg value>`, measured on the board): `(reg, value)`.
    pub fn boot_led_pad(&self) -> Option<(u64, u32)> {
        let mut key = NexusKey::new("boot-led-pad")?;
        let key = key.as_str();
        Some((u64::from(self.node.prop_cell(key, 0)?), self.node.prop_cell(key, 1)?))
    }

    /// DEPRECATED diagnostic (2026-10-04): `nexus,boot-led-ladder` (an empty property) asks the
    /// loader and the kernel for the slow milestone ladder of the desk without a serial adapter
    /// — pulse groups worth ~22 s of busy waits per boot. The boot trace on the eMMC (RFC-0107)
    /// and the picture are the observation channels now; the ladder stays for the one case
    /// they cannot see: a kernel that stops before the trace reaches the disk. Absent (the
    /// default), the LED only marks "kernel coming up" (on) and "runtime running" (off).
    pub fn boot_led_ladder(&self) -> bool {
        let Some(mut key) = NexusKey::new("boot-led-ladder") else {
            return false;
        };
        self.node.prop(key.as_str()).is_some()
    }
}

/// The boot LED as `/chosen` names it: a line of a GPIO controller node.
#[derive(Clone, Copy)]
pub struct BootLed<'a> {
    /// The GPIO controller (`reg` is its window).
    pub gpio: Node<'a>,
    /// The controller's bank (32 lines each).
    pub bank: u32,
    /// The line in the bank.
    pub line: u32,
    /// The binding's flags cell (0 = active high).
    pub flags: u32,
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
/// number, cause 9 is S-mode external, and the intc's parent is the hart. The pairs come in
/// the controller's order, which is not the harts' on every SoC.
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
    type Item = (u64, u32);
    fn next(&mut self) -> Option<(u64, u32)> {
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
                .and_then(|cpu| hart_id(&cpu));
            if let Some(hart) = hart {
                return Some((hart, ctx));
            }
        }
    }
}
