// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The planner: a consumer node + its providers → the ordered, bounded steps
//! that bring it up (RFC-0106 order: power domain, resets released, clocks on,
//! the rates the node's binding demands; pads follow). Pure: no bus,
//! host-tested against the goldens.

use nexus_fdt::Node;

use crate::pad::{pad_bits, split_pinmux, Bias, PAD_OWNED};
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
    /// One pad of a group the node names (`pinctrl-0`): the owned fields (`pad::PAD_OWNED` —
    /// function, pull, edge-detect clear) set to `value`, the rest of the word left as found.
    PadSet {
        addr: usize,
        mask: u32,
        value: u32,
    },
    /// The `mask` bits of a provider word the node's binding names (`nexus,glue-words`: the
    /// controller glue a vendor driver writes as one value — the USB host's word, measured
    /// `0x0b008000`, whose bits 24..25 ignore a write: status, not glue; the mask names the
    /// bits that are ours), set to `value`, the rest of the word left as found.
    SetWord {
        addr: usize,
        mask: u32,
        value: u32,
    },
    /// One GPIO line driven as an output (TASK-0328 U3): a supply's enable, a hub's reset
    /// line — `hub-gpios`, `vbus-gpios`. `bank` is the bank's register base.
    GpioOut {
        bank: usize,
        bit: u32,
        high: bool,
    },
    /// Hold for `ms` milliseconds before the next step: a supply's start-up delay
    /// (`vbus-delay-ms`), spent on the executor's pause, never a spin.
    Settle {
        ms: u32,
    },
}

/// The longest settle a binding may ask for (a start-up delay, not a boot stage).
pub const SETTLE_MAX_MS: u32 = 1000;

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
    /// A pad the pad map does not place (pin number) — not measured yet.
    PadUnknown(u32),
    /// A provider node of an unknown kind.
    ProviderKindUnknown,
    TooManySteps,
    /// A GPIO bank the block does not have, or a line past its 32.
    GpioUnknown(u32),
    /// A settle longer than [`SETTLE_MAX_MS`].
    SettleTooLong(u32),
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
                Step::PadSet { addr, .. } | Step::SetWord { addr, .. } => r.add(addr),
                Step::GpioOut { bank, .. } => {
                    r.add(bank + table::k1::GPIO_LEVEL as usize);
                    r.add(bank + table::k1::GPIO_DIRECTION as usize);
                }
                Step::Settle { .. } => {}
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
    // The glue words (TASK-0328 U3): `<&provider offset mask value>` (the provider's
    // `#nexus,glue-cells = <3>`), the masked bits set after the clocks run — a vendor driver's
    // one-word controller glue, measured on the stock system; the mask leaves a word's status
    // bits alone (a board cycle showed the USB word's bits 24..25 ignore a write).
    for spec in node.specifiers("nexus,glue-words", "#nexus,glue-cells") {
        let kind = ProviderKind::of(spec.provider).ok_or(PlanError::ProviderKindUnknown)?;
        let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
        let (Some(offset), Some(mask), Some(value)) = (spec.arg(0), spec.arg(1), spec.arg(2))
        else {
            return Err(PlanError::ProviderKindUnknown);
        };
        if mask == 0 {
            return Err(PlanError::ProviderKindUnknown);
        }
        p.push(Step::SetWord { addr: base + offset as usize, mask, value })?;
    }
    // The pads, last (RFC-0106 order): every pin of every group the node names in `pinctrl-0`
    // — a group is a pad controller's child whose pin nodes carry `pinmux` cells and a bias.
    for spec in node.specifiers("pinctrl-0", "#pinctrl-cells") {
        let group = spec.provider;
        let controller = group.parent().ok_or(PlanError::ProviderKindUnknown)?;
        let kind = ProviderKind::of(controller).ok_or(PlanError::ProviderKindUnknown)?;
        let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
        for pins in group.children().chain(core::iter::once(group)) {
            let bias = Bias::of(pins);
            for cell in (0..).map_while(|i| pins.prop_cell("pinmux", i)) {
                let (pin, func) = split_pinmux(cell);
                let offset = table::pad_offset(kind, pin).ok_or(PlanError::PadUnknown(pin))?;
                p.push(Step::PadSet {
                    addr: base + offset as usize,
                    mask: PAD_OWNED,
                    value: pad_bits(func, bias),
                })?;
            }
        }
    }
    // An on-board hub's lines (TASK-0328 U3, the stock binding's shape): its `hub-gpios`
    // driven, the `vbus-delay-ms` settle, then its `vbus-gpios` — after the pads, which put the
    // lines on the GPIO function. `<&gpio bank line flags>`: an active-low line is driven low.
    gpio_lines(node, "hub-gpios", providers, &mut p)?;
    if let Some(ms) = node.prop_u32("vbus-delay-ms") {
        if ms > SETTLE_MAX_MS {
            return Err(PlanError::SettleTooLong(ms));
        }
        if ms > 0 && node.prop("vbus-gpios").is_some() {
            p.push(Step::Settle { ms })?;
        }
    }
    gpio_lines(node, "vbus-gpios", providers, &mut p)?;
    Ok(p)
}

/// Every line of `prop` as an output step: the bank's base from the GPIO provider and its
/// table, the line as a bit, the level from the binding's active-low flag (bit 0).
fn gpio_lines(
    node: Node<'_>,
    prop: &str,
    providers: &Providers,
    p: &mut Plan,
) -> Result<(), PlanError> {
    for spec in node.specifiers(prop, "#gpio-cells") {
        let kind = ProviderKind::of(spec.provider).ok_or(PlanError::ProviderKindUnknown)?;
        let base = providers.get(kind).ok_or(PlanError::ProviderUnknown(kind))?.base;
        let (bank, line, flags) = (
            spec.arg(0).unwrap_or(u32::MAX),
            spec.arg(1).unwrap_or(u32::MAX),
            spec.arg(2).unwrap_or(0),
        );
        let offset = table::gpio_bank(kind, bank).ok_or(PlanError::GpioUnknown(bank))?;
        if line >= 32 {
            return Err(PlanError::GpioUnknown(line));
        }
        p.push(Step::GpioOut {
            bank: base + offset as usize,
            bit: 1 << line,
            high: flags & 1 == 0,
        })?;
    }
    Ok(())
}
