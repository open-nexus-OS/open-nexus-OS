// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's PCI device source (RFC-0098 C3, TASK-0246 P3). Every enabled
//! `pci-host-ecam-generic` node of the tree is read (`nexus_pci::PciHost`), its root bus's
//! configuration space mapped through a device capability of its own, and planned
//! (`nexus_pci::plan`: every BAR on pages of its own, memory decoding on, INTx routed, bus
//! mastering off) — the planner nxboot runs too, so both see one assignment. Every SD host
//! controller (class 0805) becomes a `DeviceWindow` like a tree node's: its BAR's pages, its
//! line, the host's coherence and DMA reach — a candidate for the boot disk the loader names
//! (`bootstrap::boot_disk`, TASK-0246 P4b), which is granted and only then given bus
//! mastering ([`enable_bus_master`], that function alone). One line names what was found; a
//! malformed host is named in a FAIL line and skipped, so the rest of the boot does not
//! depend on it.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the planner is host-tested in nexus-pci; this glue by every QEMU boot
//!   (`init: devices from pci ok (…)` is required in every profile)

use core::fmt::Write as _;

use nexus_fdt::{DmaReach, Fdt, Node};
use nexus_hal::Bus;
use nexus_pci::{Bdf, Ecam, HostError, PciHost};

use crate::bootstrap::device_tree::{self, DeviceWindow};
use crate::bootstrap::diag::Line;

/// The class code of an SD host controller.
const SD_HOST: u16 = 0x0805;

/// The SD host controller's capability register (SDHCI `CAPS`): read through the placed BAR,
/// it proves the function decodes where the plan put it.
const SDHCI_CAPS: usize = 0x40;

/// SD host controllers init keeps as boot-disk candidates.
const MAX_SD_HOSTS: usize = 4;

/// An SD host controller the plan placed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SdHost {
    /// Its host's root-bus configuration space (base, bytes).
    pub ecam: (usize, usize),
    /// Its host's root bus.
    pub first_bus: u8,
    /// Where it answers.
    pub bdf: Bdf,
    /// The window it is granted by.
    pub window: DeviceWindow,
    /// Its capability register, read through that window.
    pub caps: u32,
}

/// What init found behind the tree's ECAM hosts.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PciDevices {
    /// ECAM hosts in the tree.
    pub hosts: usize,
    /// Functions that answered on their root buses.
    pub functions: usize,
    /// The SD host controllers, in host and bus order.
    pub sd: [Option<SdHost>; MAX_SD_HOSTS],
}

impl PciDevices {
    /// The SD host controller at `bdf` behind the host whose root-bus configuration space
    /// starts at `ecam_base`.
    pub(crate) fn sd_host(&self, ecam_base: usize, bdf: Bdf) -> Option<SdHost> {
        self.sd.iter().flatten().copied().find(|sd| sd.ecam.0 == ecam_base && sd.bdf == bdf)
    }
}

/// Volatile word access to the mapped configuration window.
struct Mmio;

impl Bus for Mmio {
    fn read(&self, addr: usize) -> u32 {
        // SAFETY: `addr` lies in the configuration window `mmio_map_auto` returned for the
        // host's own capability (`Ecam` never leaves it); an aligned volatile word read.
        unsafe { core::ptr::read_volatile(addr as *const u32) }
    }
    fn write(&self, addr: usize, value: u32) {
        // SAFETY: as above, an aligned volatile word write.
        unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
    }
}

fn host_error(e: HostError) -> &'static str {
    match e {
        HostError::Binding => "binding",
        HostError::Ecam => "ecam",
        HostError::BusRange => "bus-range",
        HostError::Ranges => "ranges",
        HostError::TooManyWindows => "too-many-windows",
        HostError::InterruptMap => "interrupt-map",
        HostError::TooManyRoutes => "too-many-routes",
        HostError::InterruptParent => "interrupt-parent",
        HostError::DmaRanges => "dma-ranges",
    }
}

/// Plan every ECAM host the tree lists.
pub(crate) fn discover() -> PciDevices {
    let mut found = PciDevices::default();
    let Some(fdt) = device_tree::tree() else { return found };
    for node in fdt.find_compatible(&["pci-host-ecam-generic"]).filter(|n| n.is_enabled()) {
        found.hosts += 1;
        if let Err(reason) = plan_host(&fdt, node, &mut found) {
            let mut line = Line::new();
            let _ = write!(
                line,
                "init: devices from pci FAIL (host={} reason={})",
                node.name(),
                reason
            );
            line.emit();
        }
    }
    found
}

fn plan_host(
    fdt: &Fdt<'static>,
    node: Node<'static>,
    found: &mut PciDevices,
) -> Result<(), &'static str> {
    let host = PciHost::from_node(fdt, node).map_err(host_error)?;
    let ecam = ecam_of(&host)?;
    let plan = with_config(ecam, host.first_bus, |cfg| nexus_pci::plan(&host, cfg))?
        .map_err(|_| "too-many-functions")?;
    found.functions += plan.len();
    for f in plan.of_class(SD_HOST) {
        let Some(bar) = f.bars[0] else { continue };
        let Some(free) = found.sd.iter_mut().find(|s| s.is_none()) else { break };
        let base = usize::try_from(bar.cpu).map_err(|_| "bar")?;
        let len = usize::try_from(bar.span).map_err(|_| "bar")?;
        let irq = f.irq;
        let window =
            DeviceWindow { base, len, irq, dma_noncoherent: !host.coherent, reach: host.reach };
        let caps = read_word(&window, SDHCI_CAPS)?;
        *free = Some(SdHost { ecam, first_bus: host.first_bus, bdf: f.bdf, window, caps });
    }
    Ok(())
}

/// The host's root-bus configuration space (base, bytes).
pub(crate) fn ecam_of(host: &PciHost) -> Result<(usize, usize), &'static str> {
    let (base, len) = host.root_config();
    Ok((usize::try_from(base).map_err(|_| "ecam")?, usize::try_from(len).map_err(|_| "ecam")?))
}

/// Runs `f` over a host's root-bus configuration space, mapped through a capability of its
/// own for exactly as long as `f` runs.
fn with_config<T>(
    ecam: (usize, usize),
    first_bus: u8,
    f: impl FnOnce(&Ecam<Mmio>) -> T,
) -> Result<T, &'static str> {
    let (base, len) = ecam;
    let config = DeviceWindow { base, len, irq: 0, dma_noncoherent: false, reach: DmaReach::All };
    let desc = config.desc().map_err(|_| "ecam descriptor")?;
    let cap =
        nexus_abi::device_mmio_cap_create(&desc, usize::MAX).map_err(|_| "ecam capability")?;
    let result = nexus_abi::mmio_map_auto(cap, 0, len).map(|va| {
        let out = f(&Ecam::new(Mmio, va, first_bus, 1));
        let _ = nexus_abi::vm_unmap(va, len);
        out
    });
    let _ = nexus_abi::cap_close(cap);
    result.map_err(|_| "ecam map")
}

/// Bus mastering for the one function a grant just handed out (RFC-0098 C3: the plan never
/// sets it; the grant does, for that function only), read back from its command register.
pub(crate) fn enable_bus_master(sd: &SdHost) -> Result<(), &'static str> {
    use nexus_pci::config::{CMD_MASTER, COMMAND};
    use nexus_pci::ConfigSpace as _;
    let on = with_config(sd.ecam, sd.first_bus, |cfg| {
        nexus_pci::enable_bus_master(cfg, sd.bdf);
        cfg.read(sd.bdf, COMMAND) & CMD_MASTER != 0
    })?;
    if on {
        Ok(())
    } else {
        Err("bus mastering did not stick")
    }
}

/// One register of a window, through a capability minted for exactly that window (the
/// description the grant will repeat, so the kernel keeps one record).
fn read_word(window: &DeviceWindow, offset: usize) -> Result<u32, &'static str> {
    let desc = window.desc().map_err(|_| "bar descriptor")?;
    let cap = nexus_abi::device_mmio_cap_create(&desc, usize::MAX).map_err(|_| "bar capability")?;
    let word = nexus_abi::mmio_map_auto(cap, 0, window.len).map(|va| {
        let word = Mmio.read(va + offset);
        let _ = nexus_abi::vm_unmap(va, window.len);
        word
    });
    let _ = nexus_abi::cap_close(cap);
    word.map_err(|_| "bar map")
}

/// The discovery marker: hosts, functions, and the SD host's function, window, line and
/// capability register.
pub(crate) fn report(found: &PciDevices) {
    let mut line = Line::new();
    let _ = write!(
        line,
        "init: devices from pci ok (hosts={} functions={}",
        found.hosts, found.functions
    );
    let _ = match found.sd[0] {
        Some(sd) => write!(
            line,
            " sd={:02x}:{:02x}.{} bar=0x{:x} irq={} caps=0x{:08x})",
            sd.bdf.bus, sd.bdf.dev, sd.bdf.func, sd.window.base, sd.window.irq, sd.caps
        ),
        None => line.write_str(" sd=none)"),
    };
    line.emit();
}
