// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: BSB runtime projection (TASK-0036-B; RFC-0089 §6, ADR-0058).
//! The Boot Selection Block is a DERIVED artifact: a pure function of the
//! ONE persisted record, projected to the `bsb` partition after every
//! committed mutation (record commits FIRST, BSB second; a crash between
//! the two is healed by the idempotent startup resync). The writer obeys
//! the double-block rule (overwrite the block the current state was NOT
//! read from, seq+1) so a torn write leaves the previous projection
//! authoritative. Projection failure never fails the record commit — the
//! record stays the single authority (ADR-0055) — but it is always LOUD.
//! OWNERS: @reliability @runtime
//! STATUS: Experimental (TASK-0036 Phase B)
//! PUBLIC API: project(), fields_equal(); os half: attach()/sync()
//! TEST_COVERAGE: tests/bsb_projection.rs (golden, idempotency, absorb)
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use bootfmt::bsb::{Bsb, Slot as BsbSlot};

use crate::machine::{BootCtrl, Slot};
use crate::record;

fn to_bsb_slot(slot: Slot) -> BsbSlot {
    match slot {
        Slot::A => BsbSlot::A,
        Slot::B => BsbSlot::B,
    }
}

/// Pure projection of the record (ADR-0058 write matrix, runtime row).
/// `seq` belongs to the writer rule and is filled at write time; the
/// `boot_target` byte projects the target the NEXT boot will honor (the
/// armed one-shot when present, else the persistent target) — the loader
/// treats it as opaque pass-through.
///
/// SLOT MAPPING (the machine and the loader model "active" differently):
/// `BootCtrl::switch` flips `active_slot` to the trial slot immediately
/// and parks the previous slot in `rollback_slot`. The LOADER contract
/// (RFC-0089 §7) boots `next_slot` while tries remain and falls back to
/// `active_slot` on exhaustion — so the BSB's `active_slot` must name the
/// standing KNOWN-GOOD slot: the machine's rollback target while a trial
/// is pending, the machine's active slot otherwise. Projecting the
/// machine's active verbatim would make the exhaustion fallback boot the
/// very slot that just failed its trials.
pub fn project(boot: &BootCtrl) -> Bsb {
    let standing = match (boot.pending_slot(), boot.rollback_slot()) {
        (Some(_), Some(known_good)) => known_good,
        _ => boot.active_slot(),
    };
    Bsb {
        seq: 0,
        active_slot: to_bsb_slot(standing),
        next_slot: boot.pending_slot().map(to_bsb_slot),
        tries_left: boot.tries_left(),
        health_committed: boot.health_ok(),
        boot_target: record::encode_target(boot.next_boot().unwrap_or(boot.boot_target())),
        rollback_min_index: boot.rollback_min_index(),
    }
}

/// Equality over everything but `seq` (the idempotency predicate: equal
/// fields ⇒ no write, seq does not advance).
pub fn fields_equal(a: &Bsb, b: &Bsb) -> bool {
    Bsb { seq: 0, ..*a } == Bsb { seq: 0, ..*b }
}

/// Startup reconciliation verdict (ADR-0058 dual-actor discipline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResyncVerdict {
    /// On-disk projection matches the record — nothing to do.
    Equal,
    /// The drift is exactly a loader actuator effect (trial decrement or
    /// exhaustion clear). Do NOT overwrite it: the loader and the record's
    /// boot-attempt tick both decrement once per boot and converge on the
    /// next committed mutation; re-projecting here would hand a broken
    /// image its tries back and un-converge the boot loop.
    ActuatorPending,
    /// Genuine drift (crash between record commit and projection, torn or
    /// corrupt pair) — re-project and say so.
    Drift,
}

/// Classifies the on-disk pair against the record's projection. `current`
/// is `None` when both blocks are invalid (always a re-seed).
pub fn resync_verdict(current: Option<&Bsb>, desired: &Bsb) -> ResyncVerdict {
    let Some(cur) = current else { return ResyncVerdict::Drift };
    if fields_equal(cur, desired) {
        return ResyncVerdict::Equal;
    }
    // Trial decrement: identical except tries_left is LOWER on disk.
    if cur.next_slot == desired.next_slot
        && cur.tries_left < desired.tries_left
        && fields_equal(&Bsb { tries_left: desired.tries_left, ..*cur }, desired)
    {
        return ResyncVerdict::ActuatorPending;
    }
    // Exhaustion clear: the loader dropped next_slot and zeroed the tries.
    if cur.next_slot.is_none()
        && desired.next_slot.is_some()
        && cur.tries_left == 0
        && fields_equal(
            &Bsb { next_slot: desired.next_slot, tries_left: desired.tries_left, ..*cur },
            desired,
        )
    {
        return ResyncVerdict::ActuatorPending;
    }
    ResyncVerdict::Drift
}

/// The os-lite writer half: blockproto client on the `bsb` partition
/// (init-wired fixed slots, deny-by-default for every other sender).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub(crate) mod sync {
    use bootfmt::bsb::{self, Bsb};
    use storage::remote_blk::RemoteBlockDevice;
    use storage::{blockproto, BlockDevice};

    /// Attach budget: virtioblkd is long up when bootctld (last in the
    /// stage graph) loads its record.
    const ATTACH_BUDGET_NS: u64 = 2_000_000_000;

    pub(crate) enum Outcome {
        /// Projection already on disk — no write, seq unchanged.
        Unchanged(u64),
        /// Alternate block written with the bumped seq.
        Written(u64),
        Io,
    }

    pub(crate) fn attach() -> Option<RemoteBlockDevice> {
        RemoteBlockDevice::open_with_deadline(
            blockproto::CLIENT_REQ_SLOT,
            blockproto::CLIENT_REPLY_SEND_SLOT,
            blockproto::CLIENT_REPLY_RECV_SLOT,
            blockproto::PART_BSB,
            ATTACH_BUDGET_NS,
        )
    }

    /// Reads the current pair (for startup reconciliation).
    pub(crate) fn read_current(dev: &RemoteBlockDevice) -> Option<(Bsb, usize)> {
        let mut block0 = [0u8; 512];
        let mut block1 = [0u8; 512];
        dev.read_blocks(0, &mut block0).ok()?;
        dev.read_blocks(1, &mut block1).ok()?;
        bsb::pick(&block0, &block1)
    }

    /// Idempotent projection write (double-block rule).
    pub(crate) fn sync(dev: &mut RemoteBlockDevice, desired: &Bsb) -> Outcome {
        let mut block0 = [0u8; 512];
        let mut block1 = [0u8; 512];
        if dev.read_blocks(0, &mut block0).is_err() || dev.read_blocks(1, &mut block1).is_err() {
            return Outcome::Io;
        }
        match bsb::pick(&block0, &block1) {
            Some((cur, _)) if super::fields_equal(&cur, desired) => Outcome::Unchanged(cur.seq),
            Some((cur, picked_idx)) => {
                let next = Bsb { seq: cur.seq.saturating_add(1), ..*desired };
                let other_lba = if picked_idx == 0 { 1 } else { 0 };
                if dev.write_blocks(other_lba, &bsb::encode(&next)).is_err() {
                    return Outcome::Io;
                }
                let _ = dev.sync();
                Outcome::Written(next.seq)
            }
            None => {
                // Both blocks invalid (virgin or corrupt bsb partition):
                // re-seed block 0 — the projection is fully derivable.
                let next = Bsb { seq: 1, ..*desired };
                if dev.write_blocks(0, &bsb::encode(&next)).is_err() {
                    return Outcome::Io;
                }
                let _ = dev.sync();
                Outcome::Written(next.seq)
            }
        }
    }
}
