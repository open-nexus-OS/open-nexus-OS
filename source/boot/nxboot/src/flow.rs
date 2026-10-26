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
/// The only `load_addr` this boot chain boots: a position-independent image
/// (RFC-0098 C6, TASK-0245 P2) — the loader chooses the window, the image
/// fixes itself up there. A descriptor demanding a fixed address is refused.
pub const EXPECTED_LOAD_ADDR: u64 = bootfmt::nxbd::LOAD_ADDR_RELOCATABLE;

/// Where the bootfs goes (ADR-0081): the first boundary of this alignment past the kernel's
/// memory footprint, inside the window — the kernel's own image never reaches it, and the
/// frames between stay the pool's.
pub const BOOTFS_ALIGN: usize = 2 * 1024 * 1024;

/// Stable verify-FAIL reason vocabulary (RFC-0089 §7):
/// `nxbd | sig | digest | rollback <n> < min <m> | io | payload`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Malformed/zeroed descriptor, impossible bounds or wrong load_addr — with the check
    /// that refused it, the sector's first bytes and whether a second read of the same
    /// sector agreed (TASK-0327B P4 H0a: the board's loader loop after the stock kernel).
    Nxbd { why: NxbdWhy, head: [u8; 8], stable: bool },
    /// Signature did not verify against the baked anchor.
    Sig,
    /// Streamed image sha256 mismatch.
    Digest,
    /// Anti-downgrade floor violation (boot-time backstop, §10).
    Rollback { have: u32, min: u32 },
    /// Block reads failed.
    Io,
    /// The image is not a container this loader can place (ADR-0081) — checked after the
    /// descriptor and before the multi-MB read, under the descriptor's signature (a forged
    /// header never gets this far: the header sector is the first bytes of the digest).
    Payload(PayloadWhy),
}

/// Which check refused the container's header sector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadWhy {
    /// `nexus-bootfs` refused the header (magic, reserved bytes, item layout).
    Header(nexus_bootfs::Error),
    /// The header's items do not add up to the descriptor's `image_size`.
    Size,
    /// The bootfs, placed past the kernel's footprint, would leave the window.
    Window,
}

/// Where the two items of a container go inside the window (ADR-0081).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The kernel's bytes: `dest[..kernel_len]`.
    pub kernel_len: usize,
    /// The kernel item's sectors (its bytes padded to the sector): `dest[..kernel_padded]`.
    pub kernel_padded: usize,
    /// The bootfs's first byte inside the window.
    pub bootfs_at: usize,
    /// The bootfs's length (whole pages).
    pub bootfs_len: usize,
}

/// Computes the placement of `header`'s items in a window of `window` bytes: the kernel at the
/// window's base, the bootfs at the first `BOOTFS_ALIGN` boundary past the kernel's footprint.
pub fn placement(header: &nexus_bootfs::payload::Header, window: usize) -> Option<Placement> {
    let kernel_len = usize::try_from(header.kernel_len).ok()?;
    let kernel_padded =
        usize::try_from(header.bootfs_offset.checked_sub(header.kernel_offset)?).ok()?;
    let footprint = usize::try_from(header.kernel_footprint).ok()?;
    let bootfs_len = usize::try_from(header.bootfs_len).ok()?;
    let bootfs_at = footprint.checked_add(BOOTFS_ALIGN - 1)? & !(BOOTFS_ALIGN - 1);
    let end = bootfs_at.checked_add(bootfs_len)?;
    (kernel_padded <= bootfs_at && end <= window).then_some(Placement {
        kernel_len,
        kernel_padded,
        bootfs_at,
        bootfs_len,
    })
}

/// Which check refused a slot's descriptor sector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NxbdWhy {
    /// `bootfmt` refused the sector's structure (magic, version, reserved, CRC).
    Fmt(FmtError),
    /// A decoded image size of 0, past the partition's budget or past the window.
    Bounds,
    /// A decoded load address other than the relocatable one.
    Load,
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

/// Result of a successful run: the verified kernel sits in `dest[..kernel_len]`, the verified
/// bootfs at `dest[bootfs_at..bootfs_at + bootfs_len]` (ADR-0081).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loaded {
    pub slot: Slot,
    pub desc: Nxbd,
    pub kernel_len: usize,
    /// The bootfs's offset inside `dest` and its length.
    pub bootfs: (usize, usize),
    /// Whether this boot consumed a trial try (feeds the handoff record).
    pub tries_decremented: bool,
    /// `seq` of the BSB state this boot runs under (post-actuation).
    pub bsb_seq: u64,
}

/// A disk's boot state: its partitions, its `bsb` partition, and the BSB block that counts
/// with its index — the double-block rule (§6): the valid block with the higher seq wins.
pub struct BootState {
    pub parts: alloc::vec::Vec<Partition>,
    pub bsb_part: Partition,
    pub current: Bsb,
    pub index: usize,
}

/// Reads `dev`'s boot state. The same read decides which candidate is the boot disk
/// (`disk::carries_bsb`) and starts every boot decision (`run`).
pub fn read_boot_state<D: BlockDevice>(dev: &D) -> Result<BootState, FlowError> {
    let parts = parse_gpt(dev).map_err(|_| FlowError::Disk)?;
    let bsb_part = find_partition_named(&parts, &GUID_NEXUS_BSB, "bsb").ok_or(FlowError::Disk)?;
    let mut block0 = [0u8; SECTOR];
    let mut block1 = [0u8; SECTOR];
    dev.read_blocks(bsb_part.first_lba, &mut block0).map_err(|_| FlowError::Disk)?;
    dev.read_blocks(bsb_part.first_lba + 1, &mut block1).map_err(|_| FlowError::Disk)?;
    let (current, index) = bsb::pick(&block0, &block1).ok_or(FlowError::BsbInvalid)?;
    Ok(BootState { parts, bsb_part, current, index })
}

/// Runs one complete boot decision. On success the verified image bytes
/// are in `dest[..image_len]` and the caller may jump; on error the caller
/// panics loudly and resets (wait-loop doctrine).
pub fn run<D: BlockDevice>(
    dev: &mut D,
    dest: &mut [u8],
    emit: &mut dyn FnMut(Event),
) -> Result<Loaded, FlowError> {
    let BootState { parts, bsb_part, current: cur, index: cur_idx } = read_boot_state(dev)?;

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
        Ok((desc, place)) => {
            emit(Event::VerifyOk { slot: p.boot, desc });
            Ok(Loaded {
                slot: p.boot,
                desc,
                kernel_len: place.kernel_len,
                bootfs: (place.bootfs_at, place.bootfs_len),
                tries_decremented,
                bsb_seq: effective.seq,
            })
        }
        Err(first) => {
            emit(Event::VerifyFail { slot: p.boot, reason: first });
            let other = p.boot.other();
            emit(Event::VerifyFallback { to: other });
            match load_slot(dev, &parts, other, cur.rollback_min_index, dest) {
                Ok((desc, place)) => {
                    emit(Event::VerifyOk { slot: other, desc });
                    Ok(Loaded {
                        slot: other,
                        desc,
                        kernel_len: place.kernel_len,
                        bootfs: (place.bootfs_at, place.bootfs_len),
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
/// rollback floor → the container's header sector and the placement of its two items
/// (ADR-0081) → the kernel read to the window's base and the bootfs to its place → the
/// streamed sha256 over the three, in disk order. Cheap checks run before the multi-MB read.
fn load_slot<D: BlockDevice>(
    dev: &mut D,
    parts: &[Partition],
    slot: Slot,
    floor: u32,
    dest: &mut [u8],
) -> Result<(Nxbd, Placement), Reason> {
    let part = slot_partition(parts, slot).ok_or(Reason::Io)?;
    let mut sector0 = [0u8; SECTOR];
    dev.read_blocks(part.first_lba, &mut sector0).map_err(|_| Reason::Io)?;
    let mut head = [0u8; 8];
    head.copy_from_slice(&sector0[..8]);
    let desc = match trust::verify_nxbd(&sector0) {
        Ok(desc) => desc,
        Err(FmtError::Signature) => return Err(Reason::Sig),
        Err(err) => {
            // A second read of the same sector: the same bytes name a data path that reads
            // wrong but steadily (timing, bus width); different bytes name a marginal one.
            let mut again = [0u8; SECTOR];
            let stable = dev.read_blocks(part.first_lba, &mut again).is_ok() && again == sector0;
            return Err(Reason::Nxbd { why: NxbdWhy::Fmt(err), head, stable });
        }
    };

    let budget_sectors = (part.last_lba - part.first_lba + 1).saturating_sub(IMAGE_START_SECTOR);
    let image_len = desc.image_size as usize;
    let padded_len = image_len.div_ceil(SECTOR) * SECTOR;
    if desc.image_size == 0 || padded_len as u64 > budget_sectors * SECTOR as u64 {
        return Err(Reason::Nxbd { why: NxbdWhy::Bounds, head, stable: true });
    }
    if desc.load_addr != EXPECTED_LOAD_ADDR {
        return Err(Reason::Nxbd { why: NxbdWhy::Load, head, stable: true });
    }
    if desc.rollback_index < floor {
        return Err(Reason::Rollback { have: desc.rollback_index, min: floor });
    }

    // ADR-0081: the image is one container — its first sector names the kernel item and the
    // bootfs item; the descriptor's digest covers the sector and both items as they lie on disk.
    let image_lba = part.first_lba + IMAGE_START_SECTOR;
    let mut header = [0u8; SECTOR];
    dev.read_blocks(image_lba, &mut header).map_err(|_| Reason::Io)?;
    let container = nexus_bootfs::payload::decode(&header)
        .map_err(|e| Reason::Payload(PayloadWhy::Header(e)))?;
    if container.total_len() != Some(desc.image_size) {
        return Err(Reason::Payload(PayloadWhy::Size));
    }
    let place = placement(&container, dest.len()).ok_or(Reason::Payload(PayloadWhy::Window))?;

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(header);
    let kernel_lba = image_lba + container.kernel_offset / SECTOR as u64;
    dev.read_blocks(kernel_lba, &mut dest[..place.kernel_padded]).map_err(|_| Reason::Io)?;
    hasher.update(&dest[..place.kernel_padded]);
    let bootfs_lba = image_lba + container.bootfs_offset / SECTOR as u64;
    let bootfs = &mut dest[place.bootfs_at..place.bootfs_at + place.bootfs_len];
    dev.read_blocks(bootfs_lba, bootfs).map_err(|_| Reason::Io)?;
    hasher.update(&*bootfs);
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&hasher.finalize());
    if digest != desc.image_sha256 {
        return Err(Reason::Digest);
    }
    Ok((desc, place))
}
