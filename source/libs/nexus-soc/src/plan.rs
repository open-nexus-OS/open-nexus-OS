// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner: a consumer node + its providers → the ordered, bounded steps
//! that bring it up (RFC-0106 order: power domain, resets released, clocks on,
//! the rates the node's binding demands; pads follow). Pure: no bus,
//! host-tested against the goldens.

use nexus_fdt::Node;

use crate::provider::{ProviderKind, Providers};
use crate::table::{self, ClockEntry, DomainOn};

/// A plan never exceeds this many steps; a node that would is refused (`TooManySteps`).
pub const MAX_STEPS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The power domain is one the SoC keeps on (BUS); nothing to write.
    DomainAssumedOn {
        id: u32,
    },
    /// A hardware-sequenced power domain: `mode` in `ctrl` hands it to the power
    /// sequencer, a rising `request` asks for power, `on` in `status` reports it up.
    DomainOn {
        id: u32,
        ctrl: usize,
        mode: u32,
        request: u32,
        status: usize,
        on: u32,
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
    /// Select the parent and divider that make the rate the binding demands
    /// (`assigned-clock-rates`) and trigger the frequency change.
    SetRate {
        clock: &'static ClockEntry,
        window: usize,
        mux: u32,
        div: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The node binds a provider the tree has no window for.
    ProviderUnknown(ProviderKind),
    /// A clock/reset id the tables do not know (provider, id).
    IdUnknown(ProviderKind, u32),
    /// A power domain whose protocol the tables do not hold (not measured yet).
    DomainUnsupported(u32),
    /// A demanded rate no parent and divider of the clock make exactly (provider, clock id).
    RateUnreachable(ProviderKind, u32),
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

    /// Every register the plan reads or writes, each once, in step order — what a caller
    /// reads before and after the bring-up to show what it found and what it left.
    pub fn registers(&self) -> Registers {
        let mut r = Registers { addrs: [0; MAX_REGISTERS], len: 0 };
        for step in self.steps() {
            match *step {
                Step::DomainAssumedOn { .. } => {}
                Step::DomainOn { ctrl, status, .. } => {
                    r.add(ctrl);
                    r.add(status);
                }
                Step::ReleaseReset { addr, .. } | Step::GateOn { addr, .. } => r.add(addr),
                Step::SetRate { clock, window, .. } => {
                    r.add(window + clock.reg as usize);
                    if clock.fc != 0 {
                        r.add(window + clock.fc_reg as usize);
                    }
                }
            }
        }
        r
    }
}

/// At most two registers per step.
const MAX_REGISTERS: usize = 2 * MAX_STEPS;

/// The distinct registers of a plan, in step order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registers {
    addrs: [usize; MAX_REGISTERS],
    len: usize,
}

impl Registers {
    fn add(&mut self, addr: usize) {
        if !self.addrs[..self.len].contains(&addr) && self.len < MAX_REGISTERS {
            self.addrs[self.len] = addr;
            self.len += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.addrs[..self.len].iter().copied()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Plan the bring-up of `node`.
pub fn plan(node: Node<'_>, providers: &Providers) -> Result<Plan, PlanError> {
    let mut p = Plan::empty();
    for spec in node.specifiers("power-domains", "#power-domain-cells") {
        let id = spec.arg(0).unwrap_or(0);
        let kind = ProviderKind::of(spec.provider).ok_or(PlanError::ProviderKindUnknown)?;
        let entry = table::domain(kind, id).ok_or(PlanError::DomainUnsupported(id))?;
        match entry.on {
            DomainOn::Always => p.push(Step::DomainAssumedOn { id })?,
            DomainOn::Sequenced { ctrl, mode, request, status, on } => {
                let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
                p.push(Step::DomainOn {
                    id,
                    ctrl: base + ctrl as usize,
                    mode,
                    request,
                    status: base + status as usize,
                    on,
                })?;
            }
        }
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
    // The rates the binding demands, after the gates: the consumer is idle (released, not yet
    // driven), and a running clock is the state the frequency change is known to complete in.
    for (i, spec) in node.specifiers("assigned-clocks", "#clock-cells").enumerate() {
        // A missing or zero rate leaves that clock's rate alone (the binding's rule).
        let hz = node.prop_cell("assigned-clock-rates", i).unwrap_or(0);
        if hz == 0 {
            continue;
        }
        let kind = ProviderKind::of(spec.provider).ok_or(PlanError::ProviderKindUnknown)?;
        let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
        let id = spec.arg(0).unwrap_or(u32::MAX);
        let entry = table::clock(kind, id).ok_or(PlanError::IdUnknown(kind, id))?;
        let (mux, div) = entry.select(u64::from(hz)).ok_or(PlanError::RateUnreachable(kind, id))?;
        p.push(Step::SetRate { clock: entry, window: base, mux, div })?;
    }
    Ok(p)
}
