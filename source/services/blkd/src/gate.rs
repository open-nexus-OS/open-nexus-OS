// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the partition gate (ADR-0044: one owner of the disk, least privilege per store):
//! which sender may do what to which partition, decided on the KERNEL-ATTRIBUTED sender id —
//! never a name in a payload — and deny-by-default. Op-aware (RFC-0089 §12.5): reads and
//! writes of the system volumes have different holders. Pure, so the host proves the whole
//! matrix (`tests/gate.rs`).
//! OWNERS: @runtime

use storage::blockproto;

/// The services the gate grants anything to, by their kernel-attributed ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gates {
    statefsd: u64,
    vfsd: u64,
    bootctld: u64,
    updated: u64,
    /// TASK-0321 (RFC-0089 §12.5): the system-volume verifier/reader.
    bundlemgrd: u64,
}

impl Gates {
    /// The gate of this system: the ids the kernel attributes to its storage services.
    pub fn system() -> Self {
        let id = nexus_abi::service_id_from_name;
        Self {
            statefsd: id(b"statefsd"),
            vfsd: id(b"vfsd"),
            bootctld: id(b"bootctld"),
            updated: id(b"updated"),
            bundlemgrd: id(b"bundlemgrd"),
        }
    }

    /// True when `sender` may perform `op` on partition `part`. Op-aware (RFC-0089 §12.5):
    /// READ/INFO vs WRITE/SYNC can have different holders — the system volumes are read by
    /// bundlemgrd (and updated, for unchanged-bundle reuse) but written only by updated.
    pub fn allowed(&self, sender: u64, part: u8, op: u8) -> bool {
        let read_only = matches!(
            op,
            blockproto::OP_READ
                | blockproto::OP_INFO
                | blockproto::OP_ARM_VMO
                | blockproto::OP_READ_VMO
                | blockproto::OP_RELEASE_VMO
        );
        match part {
            blockproto::PART_STATE => sender == self.statefsd,
            blockproto::PART_DATA => sender == self.vfsd,
            // TASK-0036-B: the BSB runtime writer is bootctld and ONLY bootctld (ADR-0058;
            // the loader writes pre-OS, nx image at the factory).
            blockproto::PART_BSB => sender == self.bootctld,
            // TASK-0179 (RFC-0089 §2): slot partitions are written only by updated (the
            // engine itself refuses the ACTIVE slot; this gate scopes the sender, the engine
            // scopes the slot).
            blockproto::PART_BOOT_A | blockproto::PART_BOOT_B => sender == self.updated,
            // TASK-0321 (RFC-0089 §12.5): system volumes — bundlemgrd verifies + serves
            // (READ), updated assembles the INACTIVE one (WRITE; the engine scopes the slot)
            // and reads the ACTIVE one for reuse.
            blockproto::PART_SYSTEM_A | blockproto::PART_SYSTEM_B => {
                sender == self.updated || (read_only && sender == self.bundlemgrd)
            }
            // Anything else: deny-by-default.
            _ => false,
        }
    }
}
