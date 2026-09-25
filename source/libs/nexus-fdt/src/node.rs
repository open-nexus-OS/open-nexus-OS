// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nodes and properties. A [`Node`] is an offset into the structure block
//! plus the offset of its parent (so `reg` can read the parent's cell sizes without
//! an index); every walk is bounded by the block and by [`crate::MAX_DEPTH`].

use crate::header::{align4, Error, Fdt};
use crate::{FDT_BEGIN_NODE, FDT_END, FDT_END_NODE, FDT_NOP, FDT_PROP, MAX_DEPTH};

/// A `reg` entry translated into the root address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reg {
    pub addr: u64,
    pub size: u64,
}

/// One property: name + raw bytes.
#[derive(Clone, Copy, Debug)]
pub struct Prop<'a> {
    pub name: &'a str,
    pub value: &'a [u8],
}

/// A `<stringlist>` property, iterated string by string.
#[derive(Clone, Copy)]
pub struct StrList<'a>(pub(crate) &'a [u8]);

impl<'a> Iterator for StrList<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        if self.0.is_empty() {
            return None;
        }
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        let s = core::str::from_utf8(&self.0[..end]).ok()?;
        self.0 = if end < self.0.len() { &self.0[end + 1..] } else { &[] };
        Some(s)
    }
}

/// A node of the tree.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    pub(crate) fdt: Fdt<'a>,
    /// Offset of this node's `FDT_BEGIN_NODE` token.
    pub(crate) offset: usize,
    /// Offset of the parent's `FDT_BEGIN_NODE`; `None` for the root.
    pub(crate) parent: Option<usize>,
    pub(crate) name: &'a str,
}

impl<'a> Fdt<'a> {
    /// The root node.
    pub fn root(&self) -> Result<Node<'a>, Error> {
        let off = self.off_struct;
        let (name, _) = self.node_header(off)?;
        Ok(Node { fdt: *self, offset: off, parent: None, name })
    }

    /// Validate a `FDT_BEGIN_NODE` at `off`; return the name and the offset of the
    /// first token after it.
    pub(crate) fn node_header(&self, off: usize) -> Result<(&'a str, usize), Error> {
        if self.u32_at(off)? != FDT_BEGIN_NODE {
            return Err(Error::Structure);
        }
        let (name, after) = self.struct_str(off + 4)?;
        Ok((name, align4(after)))
    }

    /// Depth-first walk over EVERY node, in structure order.
    pub fn all_nodes(&self) -> impl Iterator<Item = Node<'a>> {
        let fdt = *self;
        let mut walker = Walker::new(fdt);
        core::iter::from_fn(move || walker.next_node())
    }

    /// Every node whose `compatible` list contains one of `wanted`.
    pub fn find_compatible(&self, wanted: &'a [&'a str]) -> impl Iterator<Item = Node<'a>> {
        self.all_nodes().filter(move |n| n.compatible().any(|c| wanted.contains(&c)))
    }

    /// The node whose `phandle` (or `linux,phandle`) equals `phandle`.
    pub fn node_by_phandle(&self, phandle: u32) -> Option<Node<'a>> {
        self.all_nodes().find(|n| n.phandle() == Some(phandle))
    }

    /// The node at an absolute path (`/soc/serial@10000000`), or an `/aliases`
    /// name (`serial0`). A `:` suffix (`serial0:115200n8`) is ignored.
    pub fn node_at_path(&self, path: &str) -> Option<Node<'a>> {
        let path = path.split(':').next().unwrap_or(path);
        let path = if path.starts_with('/') {
            path
        } else {
            // Alias: resolve through /aliases first.
            let aliases = self.node_at_path("/aliases")?;
            let target = aliases.prop_str(path)?;
            return self.node_at_path(target);
        };
        let mut node = self.root().ok()?;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            node = node.children().find(|c| c.name == part)?;
        }
        Some(node)
    }
}

/// The depth-first walker behind [`Fdt::all_nodes`] — a fixed stack of ancestor
/// offsets, so each yielded node knows its parent.
struct Walker<'a> {
    fdt: Fdt<'a>,
    off: usize,
    stack: [usize; MAX_DEPTH],
    depth: usize,
    done: bool,
}

impl<'a> Walker<'a> {
    fn new(fdt: Fdt<'a>) -> Self {
        Walker { fdt, off: fdt.off_struct, stack: [0; MAX_DEPTH], depth: 0, done: false }
    }

    fn next_node(&mut self) -> Option<Node<'a>> {
        if self.done {
            return None;
        }
        let end = self.fdt.off_struct + self.fdt.size_struct;
        while self.off + 4 <= end {
            let tok = self.fdt.u32_at(self.off).ok()?;
            match tok {
                FDT_BEGIN_NODE => {
                    let (name, after) = self.fdt.node_header(self.off).ok()?;
                    if self.depth >= MAX_DEPTH {
                        self.done = true;
                        return None;
                    }
                    let parent =
                        if self.depth == 0 { None } else { Some(self.stack[self.depth - 1]) };
                    let node = Node { fdt: self.fdt, offset: self.off, parent, name };
                    self.stack[self.depth] = self.off;
                    self.depth += 1;
                    self.off = after;
                    return Some(node);
                }
                FDT_END_NODE => {
                    self.depth = self.depth.saturating_sub(1);
                    self.off += 4;
                }
                FDT_PROP => {
                    let len = self.fdt.u32_at(self.off + 4).ok()? as usize;
                    self.off = align4(self.off + 12 + len);
                }
                FDT_NOP => self.off += 4,
                FDT_END => {
                    self.done = true;
                    return None;
                }
                _ => {
                    self.done = true;
                    return None;
                }
            }
        }
        self.done = true;
        None
    }
}

/// One entry of a phandle-specifier list: the provider node and the cells that
/// followed the phandle (a clock/reset/domain id, or nothing for a pad group).
#[derive(Clone, Copy)]
pub struct Specifier<'a> {
    pub provider: Node<'a>,
    cells: &'a [u8],
}

impl Specifier<'_> {
    /// The `i`-th cell after the phandle.
    pub fn arg(&self, i: usize) -> Option<u32> {
        let off = i.checked_mul(4)?;
        (self.cells.len() >= off + 4).then(|| {
            u32::from_be_bytes([
                self.cells[off],
                self.cells[off + 1],
                self.cells[off + 2],
                self.cells[off + 3],
            ])
        })
    }

    /// How many cells followed the phandle.
    pub fn args(&self) -> usize {
        self.cells.len() / 4
    }
}

impl<'a> Node<'a> {
    /// The node's name including its unit address (`serial@10000000`).
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The parent node, `None` for the root.
    pub fn parent(&self) -> Option<Node<'a>> {
        let poff = self.parent?;
        let (name, _) = self.fdt.node_header(poff).ok()?;
        // The grandparent is recovered by a walk; the root's parent is None.
        let grand = self.fdt.all_nodes().find(|n| n.offset == poff).and_then(|n| n.parent);
        Some(Node { fdt: self.fdt, offset: poff, parent: grand, name })
    }

    /// Properties of this node, in order.
    pub fn props(&self) -> impl Iterator<Item = Prop<'a>> {
        let fdt = self.fdt;
        let end = fdt.off_struct + fdt.size_struct;
        let mut off = fdt.node_header(self.offset).map(|(_, a)| a).unwrap_or(end);
        core::iter::from_fn(move || {
            while off + 4 <= end {
                match fdt.u32_at(off).ok()? {
                    FDT_PROP => {
                        let len = fdt.u32_at(off + 4).ok()? as usize;
                        let nameoff = fdt.u32_at(off + 8).ok()? as usize;
                        let name = fdt.string(nameoff).ok()?;
                        let value = fdt.bytes_at(off + 12, len).ok()?;
                        off = align4(off + 12 + len);
                        return Some(Prop { name, value });
                    }
                    FDT_NOP => off += 4,
                    // First child or end of this node: no more own properties.
                    _ => return None,
                }
            }
            None
        })
    }

    /// Direct children, in order.
    pub fn children(&self) -> impl Iterator<Item = Node<'a>> {
        let fdt = self.fdt;
        let me = self.offset;
        let end = fdt.off_struct + fdt.size_struct;
        let mut off = fdt.node_header(me).map(|(_, a)| a).unwrap_or(end);
        let mut depth = 0usize;
        core::iter::from_fn(move || {
            while off + 4 <= end {
                match fdt.u32_at(off).ok()? {
                    FDT_BEGIN_NODE => {
                        let (name, after) = fdt.node_header(off).ok()?;
                        let here = off;
                        off = after;
                        if depth == 0 {
                            depth = 1;
                            return Some(Node { fdt, offset: here, parent: Some(me), name });
                        }
                        depth += 1;
                        if depth > MAX_DEPTH {
                            return None;
                        }
                    }
                    FDT_END_NODE => {
                        if depth == 0 {
                            return None;
                        }
                        depth -= 1;
                        off += 4;
                    }
                    FDT_PROP => {
                        let len = fdt.u32_at(off + 4).ok()? as usize;
                        off = align4(off + 12 + len);
                    }
                    FDT_NOP => off += 4,
                    _ => return None,
                }
            }
            None
        })
    }

    /// Raw bytes of a property.
    pub fn prop(&self, name: &str) -> Option<&'a [u8]> {
        self.props().find(|p| p.name == name).map(|p| p.value)
    }

    /// A `<u32>` property.
    pub fn prop_u32(&self, name: &str) -> Option<u32> {
        let v = self.prop(name)?;
        (v.len() >= 4).then(|| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }

    /// A `<u64>` property (two cells).
    pub fn prop_u64(&self, name: &str) -> Option<u64> {
        let v = self.prop(name)?;
        (v.len() >= 8).then(|| {
            let hi = u32::from_be_bytes([v[0], v[1], v[2], v[3]]) as u64;
            let lo = u32::from_be_bytes([v[4], v[5], v[6], v[7]]) as u64;
            (hi << 32) | lo
        })
    }

    /// A `<string>` property (first string of a list).
    pub fn prop_str(&self, name: &str) -> Option<&'a str> {
        StrList(self.prop(name)?).next()
    }

    /// A `<stringlist>` property.
    pub fn prop_strs(&self, name: &str) -> StrList<'a> {
        StrList(self.prop(name).unwrap_or(&[]))
    }

    /// The `i`-th cell (big-endian u32) of a property.
    pub fn prop_cell(&self, name: &str, i: usize) -> Option<u32> {
        let v = self.prop(name)?;
        let off = i.checked_mul(4)?;
        (v.len() >= off + 4)
            .then(|| u32::from_be_bytes([v[off], v[off + 1], v[off + 2], v[off + 3]]))
    }

    /// The `compatible` list.
    pub fn compatible(&self) -> StrList<'a> {
        self.prop_strs("compatible")
    }

    pub fn is_compatible(&self, s: &str) -> bool {
        self.compatible().any(|c| c == s)
    }

    pub fn phandle(&self) -> Option<u32> {
        self.prop_u32("phandle").or_else(|| self.prop_u32("linux,phandle"))
    }

    /// Whether a bus master behind this node sees the CPU caches (RFC-0098 C4):
    /// the nearest `dma-noncoherent` or `dma-coherent` on the node or an ancestor
    /// bus decides; neither means coherent (the RISC-V binding's default — a
    /// non-coherent master is the marked exception, the K1 marks its `soc` bus).
    pub fn dma_coherent(&self) -> bool {
        let mut at = Some(*self);
        while let Some(node) = at {
            if node.prop("dma-noncoherent").is_some() {
                return false;
            }
            if node.prop("dma-coherent").is_some() {
                return true;
            }
            at = node.parent();
        }
        true
    }

    /// `status` is "okay" (or absent).
    pub fn is_enabled(&self) -> bool {
        matches!(self.prop_str("status"), None | Some("okay") | Some("ok"))
    }

    pub(crate) fn address_cells(&self) -> usize {
        self.prop_u32("#address-cells").map(|v| v as usize).unwrap_or(2)
    }

    pub(crate) fn size_cells(&self) -> usize {
        self.prop_u32("#size-cells").map(|v| v as usize).unwrap_or(1)
    }

    /// The `i`-th `reg` entry, sized by the PARENT's cells and translated through
    /// the `ranges` of every bus up to the root (the board nests its masters in
    /// `/soc/<name>-bus`, TASK-0246 P1); an empty `ranges` is the identity, an
    /// absent one means "not translatable" and that level keeps the address.
    pub fn reg(&self, i: usize) -> Result<Option<Reg>, Error> {
        let parent = match self.parent() {
            Some(p) => p,
            None => return Ok(None),
        };
        let ac = parent.address_cells();
        let sc = parent.size_cells();
        let v = match self.prop("reg") {
            Some(v) => v,
            None => return Ok(None),
        };
        let entry = (ac + sc) * 4;
        if entry == 0 {
            return Ok(None);
        }
        let off = i * entry;
        if off >= v.len() {
            return Ok(None);
        }
        if v.len() < off + entry {
            return Err(Error::ShortProp);
        }
        let addr = cells(&v[off..off + ac * 4]);
        let size = cells(&v[off + ac * 4..off + entry]);
        Ok(Some(Reg { addr: self.cpu_address(addr)?, size }))
    }

    /// An address in this node's parent bus space — what its `reg`, and for a bus the parent
    /// side of its `ranges`, hold — translated through every level to the CPU's.
    pub fn cpu_address(&self, addr: u64) -> Result<u64, Error> {
        let mut addr = addr;
        let mut level = self.parent();
        while let Some(bus) = level {
            addr = bus.translate(addr)?;
            level = bus.parent();
        }
        Ok(addr)
    }

    /// Translate a child address through this node's `ranges` into ITS parent's
    /// space (one level).
    fn translate(&self, addr: u64) -> Result<u64, Error> {
        let ranges = match self.prop("ranges") {
            Some(r) if !r.is_empty() => r,
            // Absent: not translatable; empty: the identity. Both keep the address.
            _ => return Ok(addr),
        };
        let parent = match self.parent() {
            Some(p) => p,
            None => return Ok(addr),
        };
        let cac = self.address_cells();
        let pac = parent.address_cells();
        let sc = self.size_cells();
        let entry = (cac + pac + sc) * 4;
        if entry == 0 || ranges.len() % entry != 0 {
            return Err(Error::ShortProp);
        }
        for chunk in ranges.chunks(entry) {
            let child = cells(&chunk[..cac * 4]);
            let par = cells(&chunk[cac * 4..(cac + pac) * 4]);
            let len = cells(&chunk[(cac + pac) * 4..]);
            if addr >= child && addr - child < len {
                return Ok(par + (addr - child));
            }
        }
        Ok(addr)
    }

    /// The entries of a phandle-specifier list — `clocks`, `resets`,
    /// `power-domains`, `pinctrl-0` (RFC-0106): each is a phandle followed by the
    /// provider's `#<cells_prop>` cells (0 when the provider has no such property,
    /// as a pad group has none). Stops at the first phandle without a node or a
    /// tail shorter than the provider's cell count — never a wild read.
    pub fn specifiers(
        &self,
        prop: &str,
        cells_prop: &'a str,
    ) -> impl Iterator<Item = Specifier<'a>> + 'a {
        let fdt = self.fdt;
        let v = self.prop(prop).unwrap_or(&[]);
        let mut off = 0usize;
        core::iter::from_fn(move || {
            if v.len() < off + 4 {
                return None;
            }
            let ph = u32::from_be_bytes([v[off], v[off + 1], v[off + 2], v[off + 3]]);
            let provider = fdt.node_by_phandle(ph)?;
            let cells = provider.prop_u32(cells_prop).unwrap_or(0) as usize;
            let bytes = cells.checked_mul(4)?;
            let start = off + 4;
            let end = start.checked_add(bytes)?;
            if v.len() < end {
                return None;
            }
            off = end;
            Some(Specifier { provider, cells: &v[start..end] })
        })
    }

    /// The entry of `prop` whose position in `names_prop` (`clock-names`,
    /// `reset-names`) carries `name`.
    pub fn specifier_named(
        &self,
        prop: &str,
        cells_prop: &'a str,
        names_prop: &str,
        name: &str,
    ) -> Option<Specifier<'a>> {
        let index = self.prop_strs(names_prop).position(|n| n == name)?;
        self.specifiers(prop, cells_prop).nth(index)
    }

    /// The interrupt lines, as `#interrupt-cells`-sized groups of the
    /// `interrupt-parent` (default 1 cell): yields the FIRST cell of each group,
    /// which on every PLIC is the source number.
    pub fn interrupts(&self) -> impl Iterator<Item = u32> + 'a {
        let cells_per = self.interrupt_cells();
        let v = self.prop("interrupts").unwrap_or(&[]);
        let mut off = 0usize;
        core::iter::from_fn(move || {
            if cells_per == 0 || v.len() < off + cells_per * 4 {
                return None;
            }
            let src = u32::from_be_bytes([v[off], v[off + 1], v[off + 2], v[off + 3]]);
            off += cells_per * 4;
            Some(src)
        })
    }

    /// `#interrupt-cells` of the resolved `interrupt-parent` (own, then ancestors
    /// up to [`MAX_DEPTH`], then the phandle target); 1 when unknown.
    fn interrupt_cells(&self) -> usize {
        let mut node = *self;
        for _ in 0..MAX_DEPTH {
            if let Some(ph) = node.prop_u32("interrupt-parent") {
                return self
                    .fdt
                    .node_by_phandle(ph)
                    .and_then(|p| p.prop_u32("#interrupt-cells"))
                    .map(|c| c as usize)
                    .unwrap_or(1);
            }
            match node.parent() {
                Some(p) => node = p,
                None => break,
            }
        }
        1
    }
}

/// Big-endian cells → u64 (the last two cells count; wider values are refused by
/// callers through `ShortProp` where they matter).
pub(crate) fn cells(v: &[u8]) -> u64 {
    let mut out = 0u64;
    for chunk in v.chunks(4) {
        if chunk.len() == 4 {
            out = (out << 32) | u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as u64;
        }
    }
    out
}
