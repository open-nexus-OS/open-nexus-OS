// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A consumer node's glue, whole: bring it up (plan, then execute; an empty plan means the tree
//! binds nothing — `NotNeeded`) and read one of its clocks' rate by name. These are the two
//! operations every caller runs — `socd` for the OS's drivers, the loader for its boot disk
//! before any service exists (RFC-0106) — so what "up" and "the rate" mean lives here once.

use nexus_fdt::Node;
use nexus_hal::Bus;

use crate::ops::{Executor, Fault, Report};
use crate::plan::{plan, PlanError};
use crate::provider::{ProviderKind, Providers};
use crate::table;

/// What a bring-up did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BringUp {
    /// The node binds no glue (a tree without providers, e.g. QEMU virt): nothing to do.
    NotNeeded,
    /// Every step ran and read back.
    Up(Report),
}

/// Why a node was not brought up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BringUpError {
    /// Refused before any bus access: a provider, id or power domain the tables do not cover.
    Plan(PlanError),
    /// A step failed on the bus: the register and what it read.
    Fault(Fault),
}

/// Bring `node` up — power domain, resets released, clocks on — every write read back; a step
/// already satisfied writes nothing.
pub fn bring_up<B: Bus>(
    node: Node<'_>,
    providers: &Providers,
    bus: &B,
) -> Result<BringUp, BringUpError> {
    let plan = plan(node, providers).map_err(BringUpError::Plan)?;
    if plan.is_empty() {
        return Ok(BringUp::NotNeeded);
    }
    Executor::new(bus).execute(&plan).map(BringUp::Up).map_err(BringUpError::Fault)
}

/// Why a clock has no rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateError {
    /// The node names no clock by that name (`clock-names`).
    NoSuchClock,
    /// The clock's provider is of a kind the tables do not know.
    ProviderKindUnknown,
    /// The tree has no window for the provider.
    ProviderUnknown(ProviderKind),
    /// The provider's table does not know the id.
    IdUnknown(ProviderKind, u32),
}

/// The rate of `node`'s clock `name` as its registers say: the selected parent divided by the
/// divider, or the fixed parent of a gate-only clock (0 = unknown).
pub fn clock_rate<B: Bus>(
    node: Node<'_>,
    name: &str,
    providers: &Providers,
    bus: &B,
) -> Result<u64, RateError> {
    let spec = node
        .specifier_named("clocks", "#clock-cells", "clock-names", name)
        .ok_or(RateError::NoSuchClock)?;
    let kind = ProviderKind::of(spec.provider).ok_or(RateError::ProviderKindUnknown)?;
    let provider = providers.get(kind).ok_or(RateError::ProviderUnknown(kind))?;
    let id = spec.arg(0).unwrap_or(u32::MAX);
    let entry = table::clock(kind, id).ok_or(RateError::IdUnknown(kind, id))?;
    Ok(Executor::new(bus).rate(entry, provider.base))
}
