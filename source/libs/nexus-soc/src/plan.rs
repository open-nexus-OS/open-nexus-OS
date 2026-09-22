// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner: a consumer node + its providers → the ordered, bounded steps
//! that bring it up (RFC-0106 order: power domain, resets released, clocks on,
//! pads). Pure: no bus, host-tested against the goldens.

use nexus_fdt::Node;

use crate::provider::{ProviderKind, Providers};
use crate::table;

/// A plan never exceeds this many steps (the display controller needs 8).
pub const MAX_STEPS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The power domain is one the SoC keeps on (BUS); nothing to write.
    DomainAssumedOn {
        id: u32,
    },
    ReleaseReset {
        addr: usize,
        mask: u32,
        assert_sets: bool,
    },
    GateOn {
        addr: usize,
        mask: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The node binds a provider the tree has no window for.
    ProviderUnknown(ProviderKind),
    /// A clock/reset id the tables do not know (provider, id).
    IdUnknown(ProviderKind, u32),
    /// A power domain that needs a measured protocol first (TASK-0245B P3).
    DomainUnsupported(u32),
    /// A provider node of an unknown kind.
    ProviderKindUnknown,
    TooManySteps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    steps: [Option<Step>; MAX_STEPS],
    len: usize,
}

impl Plan {
    const fn empty() -> Self {
        Plan { steps: [None; MAX_STEPS], len: 0 }
    }

    fn push(&mut self, step: Step) -> Result<(), PlanError> {
        if self.len >= MAX_STEPS {
            return Err(PlanError::TooManySteps);
        }
        self.steps[self.len] = Some(step);
        self.len += 1;
        Ok(())
    }

    pub fn steps(&self) -> impl Iterator<Item = &Step> {
        self.steps[..self.len].iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the node binds nothing (a tree without SoC glue): `NotNeeded`.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// The domains the SoC keeps on without a write (measured: every Block-1
/// consumer lives in BUS).
const ALWAYS_ON_DOMAINS: [u32; 1] = [0];

/// Plan the bring-up of `node`.
pub fn plan(node: Node<'_>, providers: &Providers) -> Result<Plan, PlanError> {
    let mut p = Plan::empty();
    for spec in node.specifiers("power-domains", "#power-domain-cells") {
        let id = spec.arg(0).unwrap_or(0);
        if !ALWAYS_ON_DOMAINS.contains(&id) {
            return Err(PlanError::DomainUnsupported(id));
        }
        p.push(Step::DomainAssumedOn { id })?;
    }
    for spec in node.specifiers("resets", "#reset-cells") {
        let kind = ProviderKind::of(spec.provider).ok_or(PlanError::ProviderKindUnknown)?;
        let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
        let id = spec.arg(0).unwrap_or(u32::MAX);
        let entry = table::reset(kind, id).ok_or(PlanError::IdUnknown(kind, id))?;
        p.push(Step::ReleaseReset {
            addr: base + entry.reg as usize,
            mask: entry.mask,
            assert_sets: entry.assert_sets,
        })?;
    }
    for spec in node.specifiers("clocks", "#clock-cells") {
        let kind = ProviderKind::of(spec.provider).ok_or(PlanError::ProviderKindUnknown)?;
        let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
        let id = spec.arg(0).unwrap_or(u32::MAX);
        let entry = table::clock(kind, id).ok_or(PlanError::IdUnknown(kind, id))?;
        if entry.gate != 0 {
            p.push(Step::GateOn { addr: base + entry.reg as usize, mask: entry.gate })?;
        }
    }
    Ok(p)
}
