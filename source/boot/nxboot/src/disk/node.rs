// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SD hosts the tree lists as nodes — the board's K1 hosts (TASK-0246B P2). Which of
//! them may hold the boot disk: a host that is a disk kind at all (`storage::boot_disk`: enabled,
//! and not marked `no-mmc` — the board's microSD slot and SDIO function are), lowest address
//! first. And the configuration a host is read with: its node's own
//! (`storage::sdhci::host_config`), once `nexus-soc` has brought its glue up — power domain,
//! resets, clocks, every write read back — and read its `io` clock, the K1's base clock. The
//! loader runs this before any service exists, for the one candidate it is about to open, over
//! the provider windows the tree names; `socd` runs the same operations while the OS runs
//! (RFC-0106).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/loader_flow.rs` — the board offers only its eMMC host; from the
//!   measured register state nothing is written and the `io` clock reads 375 MHz, from cold the
//!   documented bits; a whole boot decision over the K1 machine at that clock; glue that does
//!   not read back, a host without an `io` clock and a bus no host has are refused by name

use alloc::vec::Vec;

use nexus_fdt::{Fdt, Node};
use nexus_hal::Bus;
use nexus_soc::{BringUpError, Fault, PlanError, Providers};
use storage::boot_disk::{BootDisk, Kind, Place};
use storage_sdhci::HostConfig;

/// Why a host the tree lists is not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Its node names no bus a host can drive (`bus-width`).
    HostConfig,
    /// Its glue binds a provider, an id or a power domain the tables do not cover.
    GlueUnsupported(PlanError),
    /// A glue step failed on the bus: the register and what it read.
    Glue(Fault),
    /// It names no `io` clock the tables can rate — the K1's base clock.
    NoIoClock,
}

impl Refusal {
    /// The reason the loader's skip line names.
    pub fn name(&self) -> &'static str {
        match self {
            Refusal::HostConfig => "host config",
            Refusal::GlueUnsupported(_) => "soc glue unsupported",
            Refusal::Glue(_) => "soc glue failed",
            Refusal::NoIoClock => "no io clock",
        }
    }
}

/// The SD hosts the tree lists as nodes that may hold the boot disk, lowest address first.
pub fn emmc_hosts<'a>(fdt: &Fdt<'a>) -> Vec<(usize, Node<'a>)> {
    let mut hosts: Vec<(usize, Node<'a>)> = fdt
        .all_nodes()
        .filter(|node| Kind::of_node(node) == Some(Kind::SdhciK1))
        .filter_map(|node| {
            let reg = node.reg(0).ok().flatten()?;
            Some((usize::try_from(reg.addr).ok()?, node))
        })
        .collect();
    hosts.sort_unstable_by_key(|(base, _)| *base);
    hosts
}

/// The configuration `host` is read with, after its glue is up. `bus` reaches the provider
/// windows at the addresses the tree names (the loader runs untranslated).
pub fn open_config<B: Bus>(fdt: &Fdt<'_>, host: Node<'_>, bus: &B) -> Result<HostConfig, Refusal> {
    let disk = BootDisk { place: Place::Node(host), kind: Kind::SdhciK1 };
    let mut config = storage::sdhci::host_config(&disk).ok_or(Refusal::HostConfig)?;
    let providers = Providers::from_tree(fdt, |node| {
        node.reg(0).ok().flatten().and_then(|reg| usize::try_from(reg.addr).ok())
    });
    nexus_soc::bring_up(host, &providers, bus).map_err(|error| match error {
        BringUpError::Plan(why) => Refusal::GlueUnsupported(why),
        BringUpError::Fault(fault) => Refusal::Glue(fault),
    })?;
    // The K1's capability register names no base clock: its `io` clock's rate is it.
    let hz = nexus_soc::clock_rate(host, "io", &providers, bus).map_err(|_| Refusal::NoIoClock)?;
    let hz = u32::try_from(hz).ok().filter(|hz| *hz > 0).ok_or(Refusal::NoIoClock)?;
    config.base_clock_hz = Some(hz);
    Ok(config)
}
