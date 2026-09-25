// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's PCI device source (RFC-0098 C3, TASK-0246 P3). Every enabled
//! `pci-host-ecam-generic` node of the tree is read (`nexus_pci::PciHost`), its root bus's
//! configuration space mapped through a device capability of its own, and planned
//! (`nexus_pci::plan`: every BAR on pages of its own, memory decoding on, INTx routed, bus
//! mastering off) — the planner nxboot runs too, so both see one assignment. The first SD
//! host controller (class 0805) becomes a `DeviceWindow` like a tree node's: its BAR's
//! pages, its line, the host's coherence and DMA reach — the device `blkd` is granted
//! (TASK-0246 P4). One line names what was found; a malformed host is named in a FAIL line
//! and skipped, so the rest of the boot does not depend on it.
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
use crate::bootstrap::helpers::debug_write_bytes;

/// The class code of an SD host controller.
const SD_HOST: u16 = 0x0805;

/// The SD host controller's capability register (SDHCI `CAPS`): read through the placed BAR,
/// it proves the function decodes where the plan put it.
const SDHCI_CAPS: usize = 0x40;

/// What init found behind the tree's ECAM hosts.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PciDevices {
    /// ECAM hosts in the tree.
    pub hosts: usize,
    /// Functions that answered on their root buses.
    pub functions: usize,
    /// The first SD host controller, the window it is granted by, and its capability
    /// register as read through that window.
    pub sd: Option<(Bdf, DeviceWindow, u32)>,
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

/// One marker line, written with one call (a line written in pieces can be torn by a
/// service printing at the same time).
struct Line {
    buf: [u8; 192],
    len: usize,
}

impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let take = s.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

impl Line {
    fn new() -> Self {
        Self { buf: [0; 192], len: 0 }
    }

    fn emit(mut self) {
        let _ = self.write_str("\n");
        debug_write_bytes(&self.buf[..self.len]);
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
    let (base, len) = host.root_config();
    let (base, len) =
        (usize::try_from(base).map_err(|_| "ecam")?, usize::try_from(len).map_err(|_| "ecam")?);
    let config = DeviceWindow { base, len, irq: 0, dma_noncoherent: false, reach: DmaReach::All };
    let desc = config.desc().map_err(|_| "ecam descriptor")?;
    let cap =
        nexus_abi::device_mmio_cap_create(&desc, usize::MAX).map_err(|_| "ecam capability")?;
    let va = match nexus_abi::mmio_map_auto(cap, 0, len) {
        Ok(va) => va,
        Err(_) => {
            let _ = nexus_abi::cap_close(cap);
            return Err("ecam map");
        }
    };
    let planned = nexus_pci::plan(&host, &Ecam::new(Mmio, va, host.first_bus, 1));
    let _ = nexus_abi::vm_unmap(va, len);
    let _ = nexus_abi::cap_close(cap);
    let plan = planned.map_err(|_| "too-many-functions")?;
    found.functions += plan.len();
    let sd = plan.of_class(SD_HOST).find_map(|f| Some((f.bdf, f.bars[0]?, f.irq)));
    if let (None, Some((bdf, bar, irq))) = (found.sd, sd) {
        let base = usize::try_from(bar.cpu).map_err(|_| "bar")?;
        let len = usize::try_from(bar.span).map_err(|_| "bar")?;
        let window =
            DeviceWindow { base, len, irq, dma_noncoherent: !host.coherent, reach: host.reach };
        found.sd = Some((bdf, window, read_word(&window, SDHCI_CAPS)?));
    }
    Ok(())
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
    let _ = match found.sd {
        Some((b, w, caps)) => write!(
            line,
            " sd={:02x}:{:02x}.{} bar=0x{:x} irq={} caps=0x{:08x})",
            b.bus, b.dev, b.func, w.base, w.irq, caps
        ),
        None => line.write_str(" sd=none)"),
    };
    line.emit();
}
