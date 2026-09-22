// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! CONTEXT: The flattened device tree is the ONE hardware truth (RFC-0098, TASK-0244).
//! nxboot, the kernel and init read every platform value — memory banks, harts and
//! their timebase, the interrupt controller, the console, every device's registers
//! and interrupt lines — from the tree the previous boot stage handed over in `a1`,
//! and from nowhere else. This crate is the only code that understands the format.
//!
//! WHY BOUNDED AND ALLOCATION-FREE: the tree is trusted input from the boot chain
//! (like the kernel image itself), but it is still parsed with every offset and
//! length checked — a malformed tree is a typed error and a loud boot failure, never
//! a wild read. No `alloc`: nxboot and the kernel's early boot have no heap when they
//! read it; iteration re-walks the structure block instead of building an index, which
//! is O(nodes) per query on trees of a few hundred nodes and happens once per boot.
//!
//! WHAT IT UNDERSTANDS (DTB v17, the only version QEMU and the board's chain produce):
//! the header, the memory-reservation block, the structure block (BEGIN/END_NODE,
//! PROP, NOP, END), the strings block; `#address-cells`/`#size-cells` of the parent
//! for `reg` (with a one-level `ranges` translation — both trees use an identity
//! `ranges` on `/soc`), `#interrupt-cells` of the `interrupt-parent` for `interrupts`,
//! phandle lookups, `compatible` matching, `/memory@*`, `/reserved-memory`, `/cpus`
//! (+ `cpu-map` clusters), `/chosen` and `/aliases` path resolution. The writer edits
//! exactly one thing: `/chosen` properties, in place, inside headroom the FIT build
//! reserved (RFC-0098 C2 — nxboot is the only writer).
//!
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (consumers: nxboot, neuron kmain, nexus-init discovery)
//! TEST_COVERAGE: tests/goldens.rs (QEMU virt dump + the board's tree), tests/reject.rs
//! RFC: docs/rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md

mod chosen;
mod header;
mod node;
mod platform;

pub use chosen::ChosenWriter;
pub use header::{Error, Fdt, ReservedEntry};
pub use node::{Node, Prop, Reg, StrList};
pub use platform::{Chosen, Cpu, CpuMap, Cpus, MemoryBank, ReservedRange};

/// The token values of the structure block (DTB spec §5.4.1).
pub(crate) const FDT_BEGIN_NODE: u32 = 0x1;
pub(crate) const FDT_END_NODE: u32 = 0x2;
pub(crate) const FDT_PROP: u32 = 0x3;
pub(crate) const FDT_NOP: u32 = 0x4;
pub(crate) const FDT_END: u32 = 0x9;

/// Deepest nesting the walkers track. Both real trees stay below 8; a tree that
/// nests deeper is refused instead of overflowing a stack (`Error::TooDeep`).
pub(crate) const MAX_DEPTH: usize = 16;
