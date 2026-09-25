// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Which disk the loader boots from (TASK-0246B P1, RFC-0098 C5). The target half lists
//! the candidates in the rule's order — virtio block transports by address, SD hosts in the tree
//! that may hold an eMMC by address, SD hosts behind an ECAM host in the planner's order — and
//! [`pick`] takes the first that opens and carries a valid BSB (`flow::read_boot_state`, the read
//! every boot decision starts with); each one skipped is reported with why. The record the loader
//! then writes names that disk, and the OS grants exactly it. Host-tested against fixture disks.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/boot_disk.rs` (the rule), `tests/sdhci_reader.rs` (the SDHCI reader
//!   under a whole boot decision, against the behavioural controller + eMMC)

pub mod sdhci;

use storage::BlockDevice;

use crate::flow::{read_boot_state, FlowError};

/// Why a candidate is not the boot disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// It did not open (no card answered, the transport is not a block device …).
    NoDevice(&'static str),
    /// It opened, and carries no GPT with our `bsb` partition.
    NoLayout,
    /// It carries our layout, and neither BSB block is valid.
    NoValidBsb,
}

/// True when `dev` carries our layout and a valid BSB — the mark of the boot medium.
pub fn carries_bsb<D: BlockDevice>(dev: &D) -> Result<(), Skip> {
    match read_boot_state(dev) {
        Ok(_) => Ok(()),
        Err(FlowError::BsbInvalid) => Err(Skip::NoValidBsb),
        Err(_) => Err(Skip::NoLayout),
    }
}

/// The first of `candidates` that `open` turns into a disk carrying a valid BSB, with its
/// position; every candidate before it is reported to `skipped`.
pub fn pick<C, D: BlockDevice>(
    candidates: impl IntoIterator<Item = C>,
    mut open: impl FnMut(&C) -> Result<D, &'static str>,
    mut skipped: impl FnMut(&C, Skip),
) -> Option<(C, D)> {
    for candidate in candidates {
        match open(&candidate) {
            Err(why) => skipped(&candidate, Skip::NoDevice(why)),
            Ok(disk) => match carries_bsb(&disk) {
                Ok(()) => return Some((candidate, disk)),
                Err(skip) => skipped(&candidate, skip),
            },
        }
    }
    None
}
