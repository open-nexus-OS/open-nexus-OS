// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot trace (RFC-0107, TASK-0327B): the console text of each boot kept on the
//! boot disk, so a board boot can be read without a serial adapter. The `trace` partition holds
//! eight 1 MiB slots, one per boot: a header sector, the loader's region (64 KiB), the OS region
//! (the rest). A writer taking over a slot clears its header first; after that it writes its text
//! first and the header last, so a torn write leaves no header or the previous one — never a
//! record claiming bytes that are not there. Readers count a slot only with its magic, version,
//! CRC and in-bounds lengths. Pure over `BlockDevice`: the loader, the OS writer (Phase 2) and
//! `nx image trace` share this one codec, and the host proves it.
//! OWNERS: @runtime @devx
//! STATUS: Functional (Phase 1: the loader's side and the readers)
//! API_STABILITY: Internal (the record is RFC-0107's contract)
//! TEST_COVERAGE: unit tests below

use alloc::vec;
use alloc::vec::Vec;

use crate::gpt::{find_partition_named, parse_gpt, Partition, GUID_NEXUS_TRACE};
use crate::BlockDevice;

/// The sector the trace is laid out in.
pub const SECTOR: usize = 512;
/// Slots in the partition: the last eight boots.
pub const SLOTS: u32 = 8;
/// One slot.
pub const SLOT_BYTES: usize = 1024 * 1024;
const SLOT_SECTORS: u64 = (SLOT_BYTES / SECTOR) as u64;
/// The loader's region, right after the header.
pub const LOADER_REGION: usize = 64 * 1024;
/// The OS region: the rest of the slot.
pub const OS_REGION: usize = SLOT_BYTES - SECTOR - LOADER_REGION;
const LOADER_AT: u64 = 1;
const OS_AT: u64 = 1 + (LOADER_REGION / SECTOR) as u64;
/// The partition's name in the layout.
pub const PARTITION: &str = "trace";
/// The loader's handoff of its slot to the OS: `/chosen/nexus,trace` = `"<slot's first LBA> <seq>"`.
pub const CHOSEN_KEY: &str = "trace";

/// `NXTRACE1`, version 1.
pub const MAGIC: [u8; 8] = *b"NXTRACE1";
pub const VERSION: u32 = 1;
const CRC_SPAN: usize = 40;

/// Header flags.
pub const LOADER_COMPLETE: u32 = 1 << 0;
pub const LOADER_OVERFLOW: u32 = 1 << 1;
pub const OS_COMPLETE: u32 = 1 << 2;
pub const OS_OVERFLOW: u32 = 1 << 3;
/// The OS region's tail (or all of it) was rescued from RAM by the next boot's loader (Phase 3).
pub const OS_RESCUED: u32 = 1 << 4;

/// A slot's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub slot: u32,
    /// The boot's sequence number, from 1.
    pub seq: u64,
    pub loader_len: u32,
    pub os_len: u32,
    pub flags: u32,
}

impl Header {
    /// The header sector: the fields, then the CRC-32 (IEEE) of bytes 0..40 at 40, zeros after.
    pub fn encode(&self) -> [u8; SECTOR] {
        let mut s = [0u8; SECTOR];
        s[0..8].copy_from_slice(&MAGIC);
        s[8..12].copy_from_slice(&VERSION.to_le_bytes());
        s[12..16].copy_from_slice(&self.slot.to_le_bytes());
        s[16..24].copy_from_slice(&self.seq.to_le_bytes());
        s[24..28].copy_from_slice(&self.loader_len.to_le_bytes());
        s[28..32].copy_from_slice(&self.os_len.to_le_bytes());
        s[32..36].copy_from_slice(&self.flags.to_le_bytes());
        let crc = crate::gpt::crc32_ieee(&s[..CRC_SPAN]);
        s[CRC_SPAN..CRC_SPAN + 4].copy_from_slice(&crc.to_le_bytes());
        s
    }

    /// The header in `sector`, when it is one: the magic, the version, the CRC, a slot of the
    /// partition, a sequence number from 1, lengths inside their regions.
    pub fn decode(sector: &[u8]) -> Option<Header> {
        let s = sector.get(..SECTOR)?;
        let u32_at = |at: usize| u32::from_le_bytes([s[at], s[at + 1], s[at + 2], s[at + 3]]);
        if s[0..8] != MAGIC
            || u32_at(8) != VERSION
            || u32_at(CRC_SPAN) != crate::gpt::crc32_ieee(&s[..CRC_SPAN])
        {
            return None;
        }
        let mut seq = [0u8; 8];
        seq.copy_from_slice(&s[16..24]);
        let header = Header {
            slot: u32_at(12),
            seq: u64::from_le_bytes(seq),
            loader_len: u32_at(24),
            os_len: u32_at(28),
            flags: u32_at(32),
        };
        let fits = header.slot < SLOTS
            && header.seq >= 1
            && header.loader_len as usize <= LOADER_REGION
            && header.os_len as usize <= OS_REGION;
        fits.then_some(header)
    }
}

mod os;
#[cfg(test)]
mod testdisk;

pub use os::OsTrace;

/// Why the trace cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// The disk has no GPT, or no `trace` partition of the layout's type.
    NoPartition,
    /// The partition cannot hold the eight slots.
    TooSmall,
    /// A read or a write failed.
    Io,
    /// The slot the loader handed over does not hold this boot's record.
    NotThisBoot,
}

/// The `trace` partition among `parts`, by the layout's name and type.
pub fn partition(parts: &[Partition]) -> Option<Partition> {
    find_partition_named(parts, &GUID_NEXUS_TRACE, PARTITION)
}

fn slot_lba(part: &Partition, slot: u32) -> Result<u64, TraceError> {
    if slot >= SLOTS
        || part.last_lba < part.first_lba
        || part.last_lba - part.first_lba + 1 < u64::from(SLOTS) * SLOT_SECTORS
    {
        return Err(TraceError::TooSmall);
    }
    Ok(part.first_lba + u64::from(slot) * SLOT_SECTORS)
}

/// The eight slots' headers (`None` where a slot holds no valid record).
pub fn headers<D: BlockDevice>(
    dev: &D,
    part: &Partition,
) -> Result<Vec<Option<Header>>, TraceError> {
    let mut sector = [0u8; SECTOR];
    let mut out = Vec::with_capacity(SLOTS as usize);
    for slot in 0..SLOTS {
        dev.read_block(slot_lba(part, slot)?, &mut sector).map_err(|_| TraceError::Io)?;
        out.push(Header::decode(&sector).filter(|h| h.slot == slot));
    }
    Ok(out)
}

/// The next boot's slot and sequence number: after the highest valid one, round the ring.
pub fn next(headers: &[Option<Header>]) -> (u32, u64) {
    let seq = headers.iter().flatten().map(|h| h.seq).max().unwrap_or(0) + 1;
    (((seq - 1) % u64::from(SLOTS)) as u32, seq)
}

/// The loader's writer: one slot, its region, the header last.
pub struct LoaderTrace {
    part: Partition,
    lba: u64,
    header: Header,
    taken: bool,
}

impl LoaderTrace {
    /// The next slot of the `trace` partition on `dev` (nothing is written yet).
    pub fn open<D: BlockDevice>(dev: &D) -> Result<Self, TraceError> {
        let parts = parse_gpt(dev).map_err(|_| TraceError::NoPartition)?;
        let part = partition(&parts).ok_or(TraceError::NoPartition)?;
        let (slot, seq) = next(&headers(dev, &part)?);
        let header = Header { slot, seq, loader_len: 0, os_len: 0, flags: 0 };
        Ok(Self { lba: slot_lba(&part, slot)?, part, header, taken: false })
    }

    /// The `trace` partition this slot lies in.
    pub fn partition(&self) -> &Partition {
        &self.part
    }

    /// The previous boot's slot and sequence number — where a ring the loader finds in RAM
    /// belongs (Phase 3). `None` on the first boot.
    pub fn previous(&self) -> Option<(u64, u64)> {
        let seq = self.header.seq.checked_sub(1).filter(|&s| s >= 1)?;
        let slot = ((seq - 1) % u64::from(SLOTS)) as u32;
        Some((slot_lba(&self.part, slot).ok()?, seq))
    }

    /// The slot's first sector on the disk.
    pub fn slot_lba(&self) -> u64 {
        self.lba
    }

    pub fn slot(&self) -> u32 {
        self.header.slot
    }

    pub fn seq(&self) -> u64 {
        self.header.seq
    }

    /// Writes the loader's text so far — everything it captured, cut at the region's end — and
    /// then the header. `lost` says the loader printed more than it could keep; `complete` marks
    /// its last write.
    pub fn write<D: BlockDevice>(
        &mut self,
        dev: &mut D,
        text: &[u8],
        lost: bool,
        complete: bool,
    ) -> Result<(), TraceError> {
        let io = |_| TraceError::Io;
        if !self.taken {
            // The slot may hold a boot from eight boots ago: its header goes first.
            dev.write_block(self.lba, &[0u8; SECTOR]).map_err(io)?;
            self.taken = true;
        }
        let len = text.len().min(LOADER_REGION);
        if len > 0 {
            let mut padded = vec![0u8; len.div_ceil(SECTOR) * SECTOR];
            padded[..len].copy_from_slice(&text[..len]);
            dev.write_blocks(self.lba + LOADER_AT, &padded).map_err(io)?;
        }
        self.header.loader_len = len as u32;
        if lost || text.len() > LOADER_REGION {
            self.header.flags |= LOADER_OVERFLOW;
        }
        if complete {
            self.header.flags |= LOADER_COMPLETE;
        }
        dev.write_block(self.lba, &self.header.encode()).map_err(io)?;
        dev.sync().map_err(io)
    }
}

/// A record as a reader sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub header: Header,
    pub loader: Vec<u8>,
    pub os: Vec<u8>,
}

/// Every valid record in `part`, by sequence number.
pub fn records<D: BlockDevice>(dev: &D, part: &Partition) -> Result<Vec<Record>, TraceError> {
    let mut out = Vec::new();
    for header in headers(dev, part)?.into_iter().flatten() {
        let lba = slot_lba(part, header.slot)?;
        let region = |at: u64, len: u32| -> Result<Vec<u8>, TraceError> {
            let mut buf = vec![0u8; (len as usize).div_ceil(SECTOR) * SECTOR];
            if !buf.is_empty() {
                dev.read_blocks(lba + at, &mut buf).map_err(|_| TraceError::Io)?;
            }
            buf.truncate(len as usize);
            Ok(buf)
        };
        let loader = region(LOADER_AT, header.loader_len)?;
        let os = region(OS_AT, header.os_len)?;
        out.push(Record { header, loader, os });
    }
    out.sort_unstable_by_key(|r| r.header.seq);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::testdisk::{disk, part_of, Torn};
    use super::*;
    use crate::MemBlockDevice;

    #[test]
    fn a_boot_writes_its_slot_text_first_header_last_and_reads_back() {
        let mut dev = disk();
        let mut trace = LoaderTrace::open(&dev).expect("open");
        assert_eq!((trace.slot(), trace.seq()), (0, 1));
        trace.write(&mut dev, b"nxboot: fdt ok\n", false, false).expect("first");
        trace.write(&mut dev, b"nxboot: fdt ok\nnxboot: jump slot=a\n", false, true).expect("last");
        let records = records(&dev, &part_of(&dev)).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].loader, b"nxboot: fdt ok\nnxboot: jump slot=a\n");
        assert_eq!(records[0].header.flags, LOADER_COMPLETE);
        assert!(records[0].os.is_empty());
    }

    #[test]
    fn the_ring_takes_the_next_slot_and_wraps_after_eight_boots() {
        let mut dev = disk();
        for boot in 1..=10u64 {
            let mut trace = LoaderTrace::open(&dev).expect("open");
            assert_eq!(trace.seq(), boot);
            assert_eq!(u64::from(trace.slot()), (boot - 1) % 8);
            trace.write(&mut dev, format!("boot {boot}\n").as_bytes(), false, true).expect("write");
        }
        let records = records(&dev, &part_of(&dev)).expect("records");
        let seqs: Vec<u64> = records.iter().map(|r| r.header.seq).collect();
        assert_eq!(seqs, (3..=10).collect::<Vec<_>>(), "the last eight boots, by seq");
        assert_eq!(records[7].loader, b"boot 10\n");
    }

    #[test]
    fn test_reject_a_damaged_header_and_fields_out_of_bounds() {
        let mut dev = disk();
        let part = part_of(&dev);
        let mut trace = LoaderTrace::open(&dev).expect("open");
        trace.write(&mut dev, b"boot one\n", false, true).expect("write");
        // A flipped bit: the slot no longer counts, and the next boot reuses its number.
        let mut sector = [0u8; SECTOR];
        dev.read_block(part.first_lba, &mut sector).expect("read");
        sector[20] ^= 1;
        dev.write_block(part.first_lba, &sector).expect("write");
        assert!(records(&dev, &part).expect("records").is_empty());
        assert_eq!(next(&headers(&dev, &part).expect("headers")), (0, 1));
        // Out-of-bounds lengths are refused even with a matching CRC.
        let bad =
            Header { slot: 0, seq: 1, loader_len: LOADER_REGION as u32 + 1, os_len: 0, flags: 0 };
        assert_eq!(Header::decode(&bad.encode()), None);
        let wrong_slot = Header { slot: SLOTS, seq: 1, loader_len: 0, os_len: 0, flags: 0 };
        assert_eq!(Header::decode(&wrong_slot.encode()), None);
        // A valid header in another slot's place (a copied sector) does not count there.
        let mut dev = disk();
        let part = part_of(&dev);
        let stray = Header { slot: 5, seq: 1, loader_len: 0, os_len: 0, flags: 0 };
        dev.write_block(part.first_lba + 2 * SLOT_SECTORS, &stray.encode()).expect("write");
        assert!(records(&dev, &part).expect("records").is_empty());
    }

    #[test]
    fn test_reject_a_takeover_torn_before_its_header_leaves_no_mixed_record() {
        let mut dev = disk();
        for _ in 0..8 {
            LoaderTrace::open(&dev)
                .expect("open")
                .write(&mut dev, b"old boot\n", false, true)
                .expect("w");
        }
        let slot0 = part_of(&dev).first_lba;
        // The ninth boot takes slot 0 over; the power goes at its header, after its text.
        let mut torn =
            Torn { dev, fails: |lba, b: &[u8]| lba == slot0 && b.iter().any(|&x| x != 0) };
        let mut ninth = LoaderTrace::open(&torn).expect("ninth");
        assert_eq!(ninth.slot_lba(), slot0);
        assert_eq!(ninth.write(&mut torn, b"new!\n", false, true), Err(TraceError::Io));
        let got = records(&torn.dev, &part_of(&torn.dev)).expect("records");
        let seqs: Vec<u64> = got.iter().map(|r| r.header.seq).collect();
        assert_eq!(seqs, (2..=8).collect::<Vec<_>>(), "boot 1 is gone, never shown with new text");
        assert!(got.iter().all(|r| r.loader == b"old boot\n"));
    }

    #[test]
    fn test_reject_a_milestone_torn_in_its_text_keeps_the_last_complete_one() {
        let dev = disk();
        let slot0 = part_of(&dev).first_lba;
        // The second text sector of the loader's region never reaches the disk.
        let mut torn = Torn { dev, fails: |lba, _: &[u8]| lba == slot0 + LOADER_AT + 1 };
        let mut trace = LoaderTrace::open(&torn).expect("open");
        trace.write(&mut torn, b"nxboot: fdt ok\n", false, false).expect("first milestone");
        let mut longer = b"nxboot: fdt ok\n".to_vec();
        longer.resize(SECTOR + 40, b'.');
        assert_eq!(trace.write(&mut torn, &longer, false, true), Err(TraceError::Io));
        let got = records(&torn.dev, &part_of(&torn.dev)).expect("records");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].loader, b"nxboot: fdt ok\n", "the header still says the first milestone");
        assert_eq!(got[0].header.flags, 0, "and never complete");
    }

    #[test]
    fn test_reject_a_disk_without_the_partition_and_text_beyond_the_region() {
        let dev = MemBlockDevice::new(SECTOR, 4096);
        assert!(matches!(LoaderTrace::open(&dev), Err(TraceError::NoPartition)));
        let mut dev = disk();
        let mut trace = LoaderTrace::open(&dev).expect("open");
        let long = vec![b'x'; LOADER_REGION + 10];
        trace.write(&mut dev, &long, false, true).expect("write");
        let first = records(&dev, &part_of(&dev)).expect("records");
        assert_eq!(first[0].loader.len(), LOADER_REGION);
        assert_eq!(first[0].header.flags, LOADER_COMPLETE | LOADER_OVERFLOW);
        // Text the loader could not keep is flagged too, though what it kept fits.
        let mut trace = LoaderTrace::open(&dev).expect("open");
        trace.write(&mut dev, b"kept\n", true, false).expect("write");
        let last = records(&dev, &part_of(&dev)).expect("records").pop().expect("record");
        assert_eq!((last.loader.as_slice(), last.header.flags), (&b"kept\n"[..], LOADER_OVERFLOW));
    }
}
