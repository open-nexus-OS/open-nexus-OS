// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE single authority for the RFC-0089 §2 GPT disk layout —
//! shared by `nx image` (host builder), `virtioblkd` (TASK-0315 partition
//! server) and `nxboot` (TASK-0289 loader), so the three can never drift
//! on offsets or names. Sizes are the contract; LBAs derive from them
//! deterministically (1 MiB alignment). Growth is a conscious act gated
//! by `scripts/check-image-budgets.sh`, never a silent edit here.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable (contract: RFC-0089 §2)
//! TEST_COVERAGE: unit tests below + nx image build/verify round-trip
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use alloc::string::ToString;
use alloc::vec::Vec;

use crate::gpt::{
    Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB, GUID_NEXUS_DATA, GUID_NEXUS_STATE, GUID_NEXUS_SYS,
};

const MIB: u64 = 1024 * 1024;

/// One planned partition (name + type + size; placement is derived).
pub struct PartitionSpec {
    pub name: &'static str,
    pub type_guid: [u8; 16],
    pub size_bytes: u64,
}

/// RFC-0089 §2 (normative order). `system-a/b` are RESERVED for the
/// Phase-B bundle-set volumes — empty on purpose, so the disk never needs
/// repartitioning when that phase lands.
pub const NEXUS_DISK_LAYOUT: &[PartitionSpec] = &[
    PartitionSpec { name: "bsb", type_guid: GUID_NEXUS_BSB, size_bytes: MIB },
    PartitionSpec { name: "boot-a", type_guid: GUID_NEXUS_BOOT, size_bytes: 56 * MIB },
    PartitionSpec { name: "boot-b", type_guid: GUID_NEXUS_BOOT, size_bytes: 56 * MIB },
    PartitionSpec { name: "system-a", type_guid: GUID_NEXUS_SYS, size_bytes: 32 * MIB },
    PartitionSpec { name: "system-b", type_guid: GUID_NEXUS_SYS, size_bytes: 32 * MIB },
    PartitionSpec { name: "state", type_guid: GUID_NEXUS_STATE, size_bytes: 64 * MIB },
    PartitionSpec { name: "data", type_guid: GUID_NEXUS_DATA, size_bytes: 128 * MIB },
];

/// Total raw image size (partitions + headers + alignment headroom).
pub const NEXUS_DISK_BYTES: u64 = 384 * MIB;

/// First usable LBA (1 MiB aligned; GPT header + entries live below).
pub const FIRST_PARTITION_LBA: u64 = 2048;

/// Alignment for every partition start (1 MiB in 512-byte sectors).
const ALIGN_LBA: u64 = 2048;

/// Derives the concrete partition table from the layout (512-byte
/// sectors). Deterministic; errors as None when the layout outgrows
/// `NEXUS_DISK_BYTES` (the budget gate mirrors this check host-side).
pub fn plan() -> Option<Vec<Partition>> {
    let mut out = Vec::with_capacity(NEXUS_DISK_LAYOUT.len());
    let mut next = FIRST_PARTITION_LBA;
    for spec in NEXUS_DISK_LAYOUT {
        let sectors = spec.size_bytes.div_ceil(512);
        let first = next;
        let last = first + sectors - 1;
        out.push(Partition {
            type_guid: spec.type_guid,
            first_lba: first,
            last_lba: last,
            name: spec.name.to_string(),
        });
        // Next start: 1 MiB aligned.
        next = (last + 1).div_ceil(ALIGN_LBA) * ALIGN_LBA;
    }
    if next * 512 > NEXUS_DISK_BYTES {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_fits_and_is_aligned_and_disjoint() {
        let parts = plan().expect("layout fits");
        assert_eq!(parts.len(), NEXUS_DISK_LAYOUT.len());
        let mut prev_end = 0u64;
        for p in &parts {
            assert_eq!(p.first_lba % ALIGN_LBA, 0, "{} start aligned", p.name);
            assert!(p.first_lba > prev_end, "{} does not overlap", p.name);
            prev_end = p.last_lba;
        }
        assert!(prev_end * 512 <= NEXUS_DISK_BYTES);
    }

    #[test]
    fn plan_is_deterministic() {
        let a = plan().expect("a");
        let b = plan().expect("b");
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!((x.first_lba, x.last_lba, &x.name), (y.first_lba, y.last_lba, &y.name));
        }
    }
}
