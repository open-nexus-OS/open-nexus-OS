// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the PCI ECAM device source (RFC-0098 C3, TASK-0246 P3). A
//! `pci-host-ecam-generic` node is a list of devices like any other: [`PciHost`] reads it
//! from the tree (the ECAM window, the bus range, the memory windows from `ranges`, the
//! INTx routes from `interrupt-map`, coherence and DMA reach for the functions behind it),
//! [`plan`] enumerates its root bus through [`ConfigSpace`] (ECAM over `nexus_hal::Bus`),
//! sizes every memory BAR with decoding off and places it — largest first, lowest address
//! first, on at least one page of its own, so a capability for one function never reaches
//! another — turns memory decoding on and routes each INTx pin to its interrupt line. Bus
//! mastering stays off: it is turned on for the function a DMA driver is granted, at the
//! grant ([`enable_bus_master`]). Pure and deterministic: the same tree and the same
//! devices give the same plan, so nxboot and init see one assignment.
//! OWNERS: @runtime @drivers
//! STATUS: Functional (root bus; bridges are recorded, not crossed)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/pci` — QEMU virt's host from its golden tree, malformed hosts, and
//!   the planner over a synthetic config space (placement, isolation, routing, refusals,
//!   determinism)
//! INVARIANTS: no two functions share a page; a BAR is placed only inside a window of its
//!   kind (a 32-bit BAR below 4 GiB); bus mastering is never enabled by the plan; config
//!   data is untrusted: every size and pin is checked, nothing is unwrapped.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod config;
pub mod host;
pub mod plan;

pub use config::{ConfigSpace, Ecam};
pub use host::{HostError, PciHost, Window, WindowKind};
pub use plan::{enable_bus_master, plan, Bar, Function, Plan, PlanError, Refusal};

/// A function's address: bus, device (0..32), function (0..8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bdf {
    /// The bus.
    pub bus: u8,
    /// The device.
    pub dev: u8,
    /// The function.
    pub func: u8,
}
