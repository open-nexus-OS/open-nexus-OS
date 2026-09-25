// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nexus-pci against QEMU virt's golden tree (the host as the tree describes it,
//! and its INTx routes), a fixture of malformed hosts refused by name, and the planner over
//! a synthetic configuration space whose BARs decode like hardware (placement, page
//! isolation, the 64-bit window, bridges, refusals, determinism, bus mastering at the grant
//! alone).
//! OWNERS: @runtime @drivers

mod host;
mod mock;
mod plan;
