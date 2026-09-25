// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot disk (RFC-0098 C5, ADR-0067, TASK-0246 P4b): the medium the boot came
//! from, as the loader records it in `/chosen/nexus,boot-disk`, and the one device the block
//! owner is granted. A record is an absolute path — a node of the tree
//! (`/soc/virtio_mmio@10008000`, `/soc/storage-bus/mmc@d4281000`), or a function on the root
//! bus of an ECAM host, named the Open Firmware way as a child of that host:
//! `<generic name>@<device>,<function>` (`/soc/pci@30000000/mmc@1,0`). The generic name says
//! what the function is, so every stage reads the same kind from the same record. Three
//! kinds exist: a virtio transport, the K1's SD/MMC host, an SD host controller behind PCI;
//! anything else is refused by name. The loader writes the record (`record_for_node`,
//! `record_for_pci_sd_host`); init resolves it to the grant; the owner resolves it again
//! against the window it was granted. Pure: the tree is the only input.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (the record is RFC-0098's contract)
//! TEST_COVERAGE: `tests/boot_disk.rs` (records on QEMU virt's and the board's trees, the
//!   round trip, every refusal)

use nexus_fdt::{Fdt, Node};

/// The `/chosen/nexus,<key>` the loader writes the record in.
pub const CHOSEN_KEY: &str = "boot-disk";
/// The longest record a stage writes or accepts (the `/chosen` writer's string bound).
pub const MAX_RECORD: usize = 127;
/// The generic name of an SD/MMC host controller (Devicetree Specification, generic names):
/// the name a record gives a PCI function of class [`CLASS_SD_HOST`].
pub const PCI_SD_HOST_NAME: &str = "mmc";
/// Base class and subclass of an SD host controller.
pub const CLASS_SD_HOST: u16 = 0x0805;

const ECAM_HOST: &str = "pci-host-ecam-generic";
const VIRTIO_MMIO: &str = "virtio,mmio";
const K1_SDHCI: &str = "spacemit,k1-sdhci";

/// What a record names: the device the block owner drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A `virtio,mmio` transport. That it carries a block device (virtio device id 2) is
    /// checked by whoever maps its window.
    VirtioBlk,
    /// The K1's SD/MMC host (`spacemit,k1-sdhci`): the SDHCI core with the K1 layer.
    SdhciK1,
    /// An SD host controller behind an ECAM host (class 0805): the standard SDHCI core.
    SdhciPci,
}

impl Kind {
    /// The RFC-0017 class the grant of this disk is asked for.
    pub fn policy_class(self) -> &'static str {
        match self {
            Kind::VirtioBlk => "device.mmio.blk",
            Kind::SdhciK1 | Kind::SdhciPci => "device.mmio.mmc",
        }
    }

    /// The name markers use.
    pub fn name(self) -> &'static str {
        match self {
            Kind::VirtioBlk => "virtio-blk",
            Kind::SdhciK1 => K1_SDHCI,
            Kind::SdhciPci => "sdhci-pci",
        }
    }

    /// The kind of an enabled tree node, by its `compatible`.
    pub fn of_node(node: &Node<'_>) -> Option<Kind> {
        if !node.is_enabled() {
            return None;
        }
        node.compatible().find_map(|c| match c {
            VIRTIO_MMIO => Some(Kind::VirtioBlk),
            K1_SDHCI => Some(Kind::SdhciK1),
            _ => None,
        })
    }
}

/// Where the disk is.
#[derive(Clone, Copy)]
pub enum Place<'a> {
    /// A node of the tree.
    Node(Node<'a>),
    /// A function on the root bus of an ECAM host (a PCI function has no node of its own).
    Pci {
        /// The `pci-host-ecam-generic` node.
        host: Node<'a>,
        /// Device number (0..32).
        dev: u8,
        /// Function number (0..8).
        func: u8,
    },
}

/// A resolved record.
#[derive(Clone, Copy)]
pub struct BootDisk<'a> {
    pub place: Place<'a>,
    pub kind: Kind,
}

/// Why a record is refused. Each refusal names what was wrong; none is repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// The property is empty.
    Empty,
    /// Longer than [`MAX_RECORD`].
    TooLong,
    /// Not one NUL-terminated UTF-8 string.
    Encoding,
    /// Not an absolute path (an alias or a relative name).
    NotAbsolute,
    /// An empty component, a trailing `/`, or a `:` (a record carries no options).
    Syntax,
    /// No node at the path, and no ECAM host above its last component.
    NoSuchNode,
    /// The node — or the ECAM host above a function — is disabled.
    Disabled,
    /// The node or the function's generic name is not a disk this system drives.
    NotADisk,
    /// The function's unit address is not `<device>,<function>` in lower-case hex with
    /// device < 32 and function < 8.
    PciAddress,
}

impl Reject {
    /// The name markers use.
    pub fn name(self) -> &'static str {
        match self {
            Reject::Empty => "empty",
            Reject::TooLong => "too-long",
            Reject::Encoding => "encoding",
            Reject::NotAbsolute => "not-absolute",
            Reject::Syntax => "syntax",
            Reject::NoSuchNode => "no-such-node",
            Reject::Disabled => "disabled",
            Reject::NotADisk => "not-a-disk",
            Reject::PciAddress => "pci-address",
        }
    }
}

/// The record the tree carries: `Ok(None)` when the loader wrote none (a direct-kernel boot,
/// or a loader that predates the record).
pub fn resolve<'a>(fdt: &Fdt<'a>) -> Result<Option<BootDisk<'a>>, Reject> {
    let Ok(chosen) = fdt.chosen() else { return Ok(None) };
    let Some(raw) = chosen.nexus_bytes(CHOSEN_KEY) else { return Ok(None) };
    parse(fdt, decode(raw)?).map(Some)
}

/// One record, checked and resolved against `fdt`.
pub fn parse<'a>(fdt: &Fdt<'a>, record: &str) -> Result<BootDisk<'a>, Reject> {
    if record.is_empty() {
        return Err(Reject::Empty);
    }
    if record.len() > MAX_RECORD {
        return Err(Reject::TooLong);
    }
    if !record.starts_with('/') {
        return Err(Reject::NotAbsolute);
    }
    if record.len() == 1
        || record.ends_with('/')
        || record.contains("//")
        || record.contains(':')
        || record.contains('\0')
    {
        return Err(Reject::Syntax);
    }
    if let Some(node) = fdt.node_at_path(record) {
        if !node.is_enabled() {
            return Err(Reject::Disabled);
        }
        let kind = Kind::of_node(&node).ok_or(Reject::NotADisk)?;
        return Ok(BootDisk { place: Place::Node(node), kind });
    }
    // Not a node: a function on the root bus of the ECAM host above it, or nothing at all.
    let split = record.rfind('/').ok_or(Reject::Syntax)?;
    let (above, last) = (&record[..split], &record[split + 1..]);
    let host = if above.is_empty() { None } else { fdt.node_at_path(above) };
    let Some(host) = host.filter(|h| h.compatible().any(|c| c == ECAM_HOST)) else {
        return Err(Reject::NoSuchNode);
    };
    if !host.is_enabled() {
        return Err(Reject::Disabled);
    }
    let (name, unit) = last.split_once('@').ok_or(Reject::PciAddress)?;
    let (dev, func) = unit_address(unit).ok_or(Reject::PciAddress)?;
    if name != PCI_SD_HOST_NAME {
        return Err(Reject::NotADisk);
    }
    Ok(BootDisk { place: Place::Pci { host, dev, func }, kind: Kind::SdhciPci })
}

/// A disk node whose first register window starts at `base` (a CPU address) — how the owner
/// finds its disk when the loader wrote no record.
pub fn node_at_window<'a>(fdt: &Fdt<'a>, base: u64) -> Option<BootDisk<'a>> {
    fdt.all_nodes().find_map(|node| {
        let kind = Kind::of_node(&node)?;
        let reg = node.reg(0).ok().flatten()?;
        (reg.addr == base).then_some(BootDisk { place: Place::Node(node), kind })
    })
}

/// The record naming `node`.
pub fn record_for_node<'b>(node: &Node<'_>, out: &'b mut [u8]) -> Option<&'b str> {
    let out = out.get_mut(..MAX_RECORD.min(out.len()))?;
    node.path_into(out)
}

/// The record naming the SD host controller at `dev`,`func` on `host`'s root bus.
pub fn record_for_pci_sd_host<'b>(
    host: &Node<'_>,
    dev: u8,
    func: u8,
    out: &'b mut [u8],
) -> Option<&'b str> {
    if dev >= 32 || func >= 8 {
        return None;
    }
    let out = out.get_mut(..MAX_RECORD.min(out.len()))?;
    let mut len = host.path_into(out)?.len();
    let mut push = |bytes: &[u8]| -> Option<()> {
        out.get_mut(len..len + bytes.len())?.copy_from_slice(bytes);
        len += bytes.len();
        Some(())
    };
    push(b"/")?;
    push(PCI_SD_HOST_NAME.as_bytes())?;
    push(b"@")?;
    let mut hex = [0u8; 2];
    push(to_hex(dev, &mut hex))?;
    push(b",")?;
    push(to_hex(func, &mut hex))?;
    core::str::from_utf8(&out[..len]).ok()
}

/// One NUL-terminated UTF-8 string, nothing after the terminator.
fn decode(raw: &[u8]) -> Result<&str, Reject> {
    let Some((&0, body)) = raw.split_last() else {
        return Err(if raw.is_empty() { Reject::Empty } else { Reject::Encoding });
    };
    if body.contains(&0) {
        return Err(Reject::Encoding);
    }
    core::str::from_utf8(body).map_err(|_| Reject::Encoding)
}

/// `<device>,<function>` in lower-case hex without leading zeros (the unit address the
/// writer produces — one spelling per function).
fn unit_address(unit: &str) -> Option<(u8, u8)> {
    let (dev, func) = unit.split_once(',')?;
    let dev = hex_u8(dev)?;
    let func = hex_u8(func)?;
    (dev < 32 && func < 8).then_some((dev, func))
}

fn hex_u8(s: &str) -> Option<u8> {
    let canonical = !s.is_empty()
        && s.len() <= 2
        && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && (s.len() == 1 || !s.starts_with('0'));
    if !canonical {
        return None;
    }
    u8::from_str_radix(s, 16).ok()
}

fn to_hex(v: u8, buf: &mut [u8; 2]) -> &[u8] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    if v < 16 {
        buf[0] = DIGITS[usize::from(v)];
        &buf[..1]
    } else {
        buf[0] = DIGITS[usize::from(v >> 4)];
        buf[1] = DIGITS[usize::from(v & 0xF)];
        &buf[..2]
    }
}
