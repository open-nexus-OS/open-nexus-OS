// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The complete per-boot loader pipeline (RFC-0089 §7), generic
//! over `storage::BlockDevice` so the WHOLE decision path — BSB double
//! block → slot selection (actuator write BEFORE load) → GPT walk → NXBD
//! verify against the baked anchor → bounds → rollback floor → streamed
//! digest → fallback chain — runs identically against a host fixture disk
//! and the bare-metal virtio reader. Events are emitted through a callback;
//! the target maps them to the normative uart markers, host tests assert
//! them structurally.
//! OWNERS: @security @runtime
//! STATUS: Experimental (TASK-0289 Phase A)
//! TEST_COVERAGE: tests/loader_flow.rs (fixture disk, adversarial matrix)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

use bootfmt::bsb::{self, Bsb, Slot};
use bootfmt::nxbd::Nxbd;
use bootfmt::FmtError;
use storage::gpt::{find_partition_named, parse_gpt, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB};
use storage::BlockDevice;

use crate::select::{plan, PlanKind};
use crate::trust;

pub const SECTOR: usize = 512;
/// Image payload begins at slot sector 8 (RFC-0089 §5; sector 0 = NXBD).
pub const IMAGE_START_SECTOR: u64 = 8;
/// The only load address this boot chain supports (kernel entry contract).
pub const EXPECTED_LOAD_ADDR: u64 = 0x8020_0000;

/// Stable verify-FAIL reason vocabulary (RFC-0089 §7):
/// `nxbd | sig | digest | rollback <n> < min <m> | io`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Malformed/zeroed descriptor, impossible bounds or wrong load_addr.
    Nxbd,
    /// Signature did not verify against the baked anchor.
    Sig,
    /// Streamed image sha256 mismatch.
    Digest,
    /// Anti-downgrade floor violation (boot-time backstop, §10).
    Rollback { have: u32, min: u32 },
    /// Block reads failed.
    Io,
}

/// Terminal flow failures (all end in a loud panic + reset upstream).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowError {
    /// Both BSB blocks invalid — never guess a slot.
    BsbInvalid,
    /// GPT unreadable or the fixed partitions are missing.
    Disk,
    /// Selected slot AND its fallback failed verification.
    BothSlotsBad { first: (Slot, Reason), second: (Slot, Reason) },
}

/// Progress events in marker order (target renders uart lines from these).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// `nxboot: bsb ok (slot=<s> seq=<n>)` — selection decided.
    BsbOk { slot: Slot, seq: u64 },
    /// `nxboot: tries <from>-><to> (slot=<s> trial)` — persisted pre-load.
    Tries { slot: Slot, from: u8, to: u8 },
    /// `nxboot: fallback (slot=<s> exhausted) -> slot=<s'>`.
    Exhausted { attempted: Slot, to: Slot },
    /// `nxboot: verify ok (slot=<s> build=<id8> rbidx=<n>)`.
    VerifyOk { slot: Slot, desc: Nxbd },
    /// `nxboot: verify FAIL (slot=<s> <reason>)`.
    VerifyFail { slot: Slot, reason: Reason },
    /// `nxboot: fallback -> slot=<s>` (after a verify failure).
    VerifyFallback { to: Slot },
}

/// Result of a successful run: the verified image sits in `dest[..image_len]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub slot: Slot,
    pub desc: Nxbd,
    pub image_len: usize,
    /// Whether this boot consumed a trial try (feeds the handoff record).
    pub tries_decremented: bool,
    /// `seq` of the BSB state this boot runs under (post-actuation).
    pub bsb_seq: u64,
}

/// Runs one complete boot decision. On success the verified image bytes
/// are in `dest[..image_len]` and the caller may jump; on error the caller
/// panics loudly and resets (wait-loop doctrine).
pub fn run<D: BlockDevice>(
    dev: &mut D,
    dest: &mut [u8],
    emit: &mut dyn FnMut(Event),
) -> Result<Loaded, FlowError> {
    let parts = parse_gpt(dev).map_err(|_| FlowError::Disk)?;
    let bsb_part = find_partition_named(&parts, &GUID_NEXUS_BSB, "bsb").ok_or(FlowError::Disk)?;

    // BSB double-block rule (§6): valid block with the higher seq wins.
    let mut block0 = [0u8; SECTOR];
    let mut block1 = [0u8; SECTOR];
    dev.read_blocks(bsb_part.first_lba, &mut block0).map_err(|_| FlowError::Disk)?;
    dev.read_blocks(bsb_part.first_lba + 1, &mut block1).map_err(|_| FlowError::Disk)?;
    let (cur, cur_idx) = bsb::pick(&block0, &block1).ok_or(FlowError::BsbInvalid)?;

    let p = plan(&cur);
    emit(Event::BsbOk { slot: p.boot, seq: cur.seq });

    // Actuate BEFORE load (§7 step 2): a boot loop into a broken image
    // converges because the decrement is already on disk.
    let mut tries_decremented = false;
    let mut effective = cur;
    if let Some(w) = p.write {
        match p.kind {
            PlanKind::Trial { tries_after } => {
                emit(Event::Tries { slot: p.boot, from: cur.tries_left, to: tries_after });
                tries_decremented = true;
            }
            PlanKind::ExhaustedFallback { attempted } => {
                emit(Event::Exhausted { attempted, to: p.boot });
            }
            PlanKind::Active => {}
        }
        write_alternate(dev, &bsb_part, cur_idx, &w)?;
        effective = w;
    }

    // Verify chain: the selected slot first, then the other one (§7 step 3).
    match load_slot(dev, &parts, p.boot, cur.rollback_min_index, dest) {
        Ok(desc) => {
            emit(Event::VerifyOk { slot: p.boot, desc });
            Ok(Loaded {
                slot: p.boot,
                desc,
                image_len: desc.image_size as usize,
                tries_decremented,
                bsb_seq: effective.seq,
            })
        }
        Err(first) => {
            emit(Event::VerifyFail { slot: p.boot, reason: first });
            let other = p.boot.other();
            emit(Event::VerifyFallback { to: other });
            match load_slot(dev, &parts, other, cur.rollback_min_index, dest) {
                Ok(desc) => {
                    emit(Event::VerifyOk { slot: other, desc });
                    Ok(Loaded {
                        slot: other,
                        desc,
                        image_len: desc.image_size as usize,
                        tries_decremented,
                        bsb_seq: effective.seq,
                    })
                }
                Err(second) => {
                    emit(Event::VerifyFail { slot: other, reason: second });
                    Err(FlowError::BothSlotsBad { first: (p.boot, first), second: (other, second) })
                }
            }
        }
    }
}

/// Writer rule (§6): always overwrite the block the current state was NOT
/// read from — a torn single-sector write leaves the old block authoritative.
fn write_alternate<D: BlockDevice>(
    dev: &mut D,
    bsb_part: &Partition,
    picked_idx: usize,
    next: &Bsb,
) -> Result<(), FlowError> {
    let other_lba = bsb_part.first_lba + if picked_idx == 0 { 1 } else { 0 };
    dev.write_blocks(other_lba, &bsb::encode(next)).map_err(|_| FlowError::Disk)?;
    dev.sync().map_err(|_| FlowError::Disk)
}

fn slot_partition(parts: &[Partition], slot: Slot) -> Option<Partition> {
    let name = match slot {
        Slot::A => "boot-a",
        Slot::B => "boot-b",
    };
    find_partition_named(parts, &GUID_NEXUS_BOOT, name)
}

/// Verifies and loads one slot: NXBD signature (baked anchor) → bounds →
/// rollback floor → image read into `dest` → streamed sha256. Cheap checks
/// run before the multi-MB read.
fn load_slot<D: BlockDevice>(
    dev: &mut D,
    parts: &[Partition],
    slot: Slot,
    floor: u32,
    dest: &mut [u8],
) -> Result<Nxbd, Reason> {
    let part = slot_partition(parts, slot).ok_or(Reason::Io)?;
    let mut sector0 = [0u8; SECTOR];
    dev.read_blocks(part.first_lba, &mut sector0).map_err(|_| Reason::Io)?;
    let desc = trust::verify_nxbd(&sector0).map_err(|err| match err {
        FmtError::Signature => Reason::Sig,
        _ => Reason::Nxbd,
    })?;

    let budget_sectors = (part.last_lba - part.first_lba + 1).saturating_sub(IMAGE_START_SECTOR);
    let image_len = desc.image_size as usize;
    let padded_len = image_len.div_ceil(SECTOR) * SECTOR;
    if desc.image_size == 0
        || padded_len as u64 > budget_sectors * SECTOR as u64
        || padded_len > dest.len()
        || desc.load_addr != EXPECTED_LOAD_ADDR
    {
        return Err(Reason::Nxbd);
    }
    if desc.rollback_index < floor {
        return Err(Reason::Rollback { have: desc.rollback_index, min: floor });
    }

    dev.read_blocks(part.first_lba + IMAGE_START_SECTOR, &mut dest[..padded_len])
        .map_err(|_| Reason::Io)?;
    let digest = sha256(&dest[..image_len]);
    if digest != desc.image_sha256 {
        return Err(Reason::Digest);
    }
    Ok(desc)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}
