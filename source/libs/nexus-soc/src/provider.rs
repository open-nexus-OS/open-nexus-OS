// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The providers a tree names (by compatible) and where their windows are.

use nexus_fdt::{Fdt, Node};

/// The K1 syscon windows the tables know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    Apbc,
    Apmu,
    Mpmu,
    Apbc2,
    Pll,
    Pinctrl,
    /// The GPIO block (TASK-0328 U3): the lines a node's supplies and resets are driven on —
    /// an on-board USB hub's power and VBUS. The glue owner drives them like a pad: an
    /// output set, read back.
    Gpio,
}

impl ProviderKind {
    /// The short name markers use for the window (`apmu+0x3f4`).
    pub const fn name(self) -> &'static str {
        match self {
            ProviderKind::Apbc => "apbc",
            ProviderKind::Apmu => "apmu",
            ProviderKind::Mpmu => "mpmu",
            ProviderKind::Apbc2 => "apbc2",
            ProviderKind::Pll => "pll",
            ProviderKind::Pinctrl => "pinctrl",
            ProviderKind::Gpio => "gpio",
        }
    }

    /// The kind of a node, from its compatible list.
    pub fn of(node: Node<'_>) -> Option<Self> {
        const KINDS: [(&str, ProviderKind); 7] = [
            ("spacemit,k1-syscon-apbc", ProviderKind::Apbc),
            ("spacemit,k1-syscon-apmu", ProviderKind::Apmu),
            ("spacemit,k1-syscon-mpmu", ProviderKind::Mpmu),
            ("spacemit,k1-syscon-apbc2", ProviderKind::Apbc2),
            ("spacemit,k1-pll", ProviderKind::Pll),
            ("spacemit,k1-pinctrl", ProviderKind::Pinctrl),
            ("spacemit,k1-gpio", ProviderKind::Gpio),
        ];
        KINDS.iter().find(|(c, _)| node.is_compatible(c)).map(|(_, k)| *k)
    }
}

/// One provider: its kind and the base address its window is reachable at
/// (physical from the tree in tests; the mapped window inside `socd`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provider {
    pub kind: ProviderKind,
    pub base: usize,
}

/// The providers of one tree (at most one per kind).
#[derive(Clone, Copy, Debug, Default)]
pub struct Providers {
    slots: [Option<Provider>; ProviderKind::COUNT],
}

impl ProviderKind {
    /// How many kinds there are (the size of every per-kind table and slot block).
    pub const COUNT: usize = 7;
}

impl Providers {
    pub const fn new() -> Self {
        Providers { slots: [None; ProviderKind::COUNT] }
    }

    /// Every provider the tree lists, its base taken from `base_of` (the node's
    /// `reg` in tests, the granted window's mapping in `socd`). A kind listed
    /// twice keeps the first.
    pub fn from_tree(fdt: &Fdt<'_>, mut base_of: impl FnMut(Node<'_>) -> Option<usize>) -> Self {
        let mut p = Providers::new();
        for node in fdt.all_nodes() {
            let Some(kind) = ProviderKind::of(node) else { continue };
            if p.get(kind).is_some() {
                continue;
            }
            if let Some(base) = base_of(node) {
                p.set(Provider { kind, base });
            }
        }
        p
    }

    pub fn set(&mut self, provider: Provider) {
        self.slots[provider.kind as usize] = Some(provider);
    }

    pub fn get(&self, kind: ProviderKind) -> Option<Provider> {
        self.slots[kind as usize]
    }

    /// How many providers are known (0 = a tree without SoC glue, e.g. QEMU virt).
    pub fn count(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// The window that holds `addr` and the offset inside it — the provider with the highest
    /// base at or below `addr` (windows do not overlap, and every planned address is a base
    /// plus a table offset). For naming a register in a marker, never for access.
    pub fn locate(&self, addr: usize) -> Option<(ProviderKind, usize)> {
        self.slots
            .iter()
            .flatten()
            .filter(|p| p.base <= addr)
            .max_by_key(|p| p.base)
            .map(|p| (p.kind, addr - p.base))
    }
}
