// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE single authority for the RFC-0089 §2 GPT disk layout —
//! shared by `nx image` (host builder), `blkd` (partition server) and
//! `nxboot` (loader), so the three can never drift on offsets, names or
//! types. The disk starts with the part of the board's boot chain that lives
//! in the user area (TASK-0260 P1/P2, RFC-0089 §2 amendment 2026-09-26):
//! GPT partitions 1–2, `opensbi` and `uboot`, which the SPL — itself in the
//! eMMC's boot0 hardware partition, next to the boot-ROM header — finds by
//! NAME (the vendor SPL's configuration, docs/board/measurements/
//! 2026-09-26-boot-medium) — zero on QEMU. Our volumes follow from the next
//! 1 MiB boundary. Sizes are the contract; LBAs derive from them
//! deterministically. Growth is a conscious act gated by
//! `scripts/check-image-budgets.sh`, never a silent edit here.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable (contract: RFC-0089 §2)
//! TEST_COVERAGE: unit tests below + nx image build/verify round-trip
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use alloc::string::ToString;
use alloc::vec::Vec;

use crate::gpt::{
    first_usable_lba, last_usable_lba, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB, GUID_NEXUS_DATA,
    GUID_NEXUS_FW, GUID_NEXUS_STATE, GUID_NEXUS_SYS,
};

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const SECTOR: u64 = 512;

/// Where a partition starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// At this byte offset — the boot-ROM head, where the vendor's stages look.
    At(u64),
    /// At the next 1 MiB boundary after the partition before it.
    Next,
}

/// One planned partition (name + type + size + where it starts).
pub struct PartitionSpec {
    pub name: &'static str,
    pub type_guid: [u8; 16],
    pub size_bytes: u64,
    pub place: Place,
}

/// The disk, in GPT order. The head's entries 1–2 are the board's boot firmware the SPL loads
/// by name: OpenSBI (`opensbi`) and the loader's FIT (`uboot`, the SPL's name for its payload
/// slot), at the offsets the vendor's medium uses. Then RFC-0089 §2's volumes
/// (normative order); `system-a/b` are RESERVED for the Phase-B bundle-set volumes — empty on
/// purpose, so the disk never needs repartitioning when that phase lands. `swap` (M7) is
/// appended last once M7 has measured its size.
pub const NEXUS_DISK_LAYOUT: &[PartitionSpec] = &[
    PartitionSpec {
        name: "opensbi",
        type_guid: GUID_NEXUS_FW,
        size_bytes: MIB,
        place: Place::At(MIB),
    },
    PartitionSpec {
        name: "uboot",
        type_guid: GUID_NEXUS_FW,
        size_bytes: 2 * MIB,
        place: Place::At(2 * MIB),
    },
    PartitionSpec { name: "bsb", type_guid: GUID_NEXUS_BSB, size_bytes: MIB, place: Place::Next },
    PartitionSpec {
        name: "boot-a",
        type_guid: GUID_NEXUS_BOOT,
        size_bytes: 56 * MIB,
        place: Place::Next,
    },
    PartitionSpec {
        name: "boot-b",
        type_guid: GUID_NEXUS_BOOT,
        size_bytes: 56 * MIB,
        place: Place::Next,
    },
    PartitionSpec {
        name: "system-a",
        type_guid: GUID_NEXUS_SYS,
        size_bytes: 32 * MIB,
        place: Place::Next,
    },
    PartitionSpec {
        name: "system-b",
        type_guid: GUID_NEXUS_SYS,
        size_bytes: 32 * MIB,
        place: Place::Next,
    },
    PartitionSpec {
        name: "state",
        type_guid: GUID_NEXUS_STATE,
        size_bytes: 64 * MIB,
        place: Place::Next,
    },
    PartitionSpec {
        name: "data",
        type_guid: GUID_NEXUS_DATA,
        size_bytes: 128 * MIB,
        place: Place::Next,
    },
];

/// The smallest disk the layout is built for (QEMU's image); a board's disk is larger.
pub const NEXUS_DISK_BYTES: u64 = 384 * MIB;

/// Alignment of every volume start (1 MiB in 512-byte sectors).
const ALIGN_LBA: u64 = 2048;

/// Derives the concrete partition table from the layout (512-byte sectors). Deterministic;
/// `None` when a fixed entry falls behind the one before it or the layout outgrows
/// `NEXUS_DISK_BYTES` (the budget gate mirrors this check host-side).
pub fn plan() -> Option<Vec<Partition>> {
    let mut out = Vec::with_capacity(NEXUS_DISK_LAYOUT.len());
    let mut next = first_usable_lba(SECTOR as usize);
    for spec in NEXUS_DISK_LAYOUT {
        let first = match spec.place {
            Place::At(offset) if offset % SECTOR == 0 && offset / SECTOR >= next => offset / SECTOR,
            Place::At(_) => return None,
            Place::Next => next.div_ceil(ALIGN_LBA) * ALIGN_LBA,
        };
        let last = first + spec.size_bytes.div_ceil(SECTOR) - 1;
        out.push(Partition {
            type_guid: spec.type_guid,
            first_lba: first,
            last_lba: last,
            name: spec.name.to_string(),
        });
        next = last + 1;
    }
    let last_usable = last_usable_lba(NEXUS_DISK_BYTES / SECTOR, SECTOR as usize)?;
    (next - 1 <= last_usable).then_some(out)
}

/// The partition type the layout gives `name`; `None` for a name it does not have.
pub fn type_of(name: &str) -> Option<[u8; 16]> {
    NEXUS_DISK_LAYOUT.iter().find(|spec| spec.name == name).map(|spec| spec.type_guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_is_what_the_spl_loads_by_name() {
        let parts = plan().expect("layout fits");
        let span = |p: &Partition| (p.first_lba * SECTOR, (p.last_lba + 1 - p.first_lba) * SECTOR);
        let head: Vec<(&str, (u64, u64))> =
            parts[..2].iter().map(|p| (p.name.as_str(), span(p))).collect();
        // The names the vendor SPL loads (measured 2026-09-26), where the vendor's medium has them.
        assert_eq!(head, [("opensbi", (MIB, MIB)), ("uboot", (2 * MIB, 2 * MIB))]);
        assert!(parts[..2].iter().all(|p| p.type_guid == GUID_NEXUS_FW));
        assert_eq!((parts[2].name.as_str(), parts[2].first_lba * SECTOR), ("bsb", 4 * MIB));
        assert!(type_of("fsbl").is_none() && type_of("env").is_none(), "boot0 holds the SPL");
    }

    #[test]
    fn plan_fits_and_is_aligned_and_disjoint() {
        let parts = plan().expect("layout fits");
        assert_eq!(parts.len(), NEXUS_DISK_LAYOUT.len());
        let mut prev_end = 0u64;
        for (p, spec) in parts.iter().zip(NEXUS_DISK_LAYOUT) {
            if spec.place == Place::Next {
                assert_eq!(p.first_lba % ALIGN_LBA, 0, "{} start aligned", p.name);
            }
            assert!(p.first_lba > prev_end, "{} does not overlap", p.name);
            prev_end = p.last_lba;
        }
        let last_usable = last_usable_lba(NEXUS_DISK_BYTES / SECTOR, 512).expect("disk");
        assert!(prev_end <= last_usable, "in front of the backup GPT");
    }

    #[test]
    fn plan_is_deterministic() {
        let a = plan().expect("a");
        let b = plan().expect("b");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!((x.first_lba, x.last_lba, &x.name), (y.first_lba, y.last_lba, &y.name));
        }
    }

    #[test]
    fn every_name_has_one_type() {
        assert_eq!(type_of("bsb"), Some(GUID_NEXUS_BSB));
        assert_eq!(type_of("uboot"), Some(GUID_NEXUS_FW));
        assert_eq!(type_of("swap"), None);
        let mut names: Vec<&str> = NEXUS_DISK_LAYOUT.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), NEXUS_DISK_LAYOUT.len(), "names are unique");
    }
}
