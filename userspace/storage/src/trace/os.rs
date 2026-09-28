// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot trace's OS writer (RFC-0107 Phase 2, TASK-0327B P2): the block owner
//! appends the kernel console's text to the OS region of this boot's slot — the slot the loader
//! took and handed over in `/chosen/nexus,trace` — text first, header last. It never takes a
//! slot over: the slot's header must already be this boot's record (magic, CRC, the handed-over
//! sequence number, the slot the ring rule gives that number), and the loader's region and flags
//! stay as the loader left them. Appending allocates nothing (the OS services' heaps never free):
//! whole sectors go straight from the caller's text, a partial last sector through one sector
//! kept here. A region that fills, or text the kernel ring dropped before it got here, sets
//! `OS_OVERFLOW`; bytes that do not fit are not written.
//! OWNERS: @runtime @devx
//! STATUS: Functional
//! API_STABILITY: Internal (the record is RFC-0107's contract)
//! TEST_COVERAGE: unit tests below

use super::{
    slot_lba, Header, TraceError, OS_AT, OS_OVERFLOW, OS_REGION, OS_RESCUED, SECTOR, SLOTS,
};
use crate::gpt::Partition;
use crate::BlockDevice;

/// The OS writer: one slot, its OS region, the header last.
pub struct OsTrace {
    lba: u64,
    header: Header,
    /// The OS region's last sector as it is on the disk, while it is partial.
    tail: [u8; SECTOR],
}

impl OsTrace {
    /// This boot's slot in the `trace` partition `part` of `dev`, as the loader handed it over:
    /// boot `seq` at `lba`. The slot is the one the ring rule gives `seq`, at that slot's place in
    /// the partition — a handed-over address anywhere else means the loader and the OS disagree
    /// about the disk, and nothing is written — and its header is already that boot's record.
    pub fn open<D: BlockDevice>(
        dev: &D,
        part: &Partition,
        lba: u64,
        seq: u64,
    ) -> Result<Self, TraceError> {
        if seq == 0 {
            return Err(TraceError::NotThisBoot);
        }
        let slot = ((seq - 1) % u64::from(SLOTS)) as u32;
        if slot_lba(part, slot)? != lba {
            return Err(TraceError::NotThisBoot);
        }
        let mut sector = [0u8; SECTOR];
        dev.read_block(lba, &mut sector).map_err(|_| TraceError::Io)?;
        let header = Header::decode(&sector).ok_or(TraceError::NotThisBoot)?;
        if header.seq != seq || header.slot != slot {
            return Err(TraceError::NotThisBoot);
        }
        let mut tail = [0u8; SECTOR];
        let used = header.os_len as usize;
        if used % SECTOR != 0 {
            let at = lba + OS_AT + (used / SECTOR) as u64;
            dev.read_block(at, &mut tail).map_err(|_| TraceError::Io)?;
        }
        Ok(Self { lba, header, tail })
    }

    /// Bytes kept in the OS region so far.
    pub fn len(&self) -> usize {
        self.header.os_len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Appends `text` — as much as the region still holds — then writes the header. `lost` says
    /// text was dropped before it reached here. Returns the bytes kept.
    pub fn append<D: BlockDevice>(
        &mut self,
        dev: &mut D,
        text: &[u8],
        lost: bool,
    ) -> Result<usize, TraceError> {
        self.append_flagged(dev, text, lost, 0)
    }

    /// The next boot's loader appends what it found of this boot's ring in RAM — the tail the
    /// block owner never wrote, or all of it (RFC-0107 Phase 3): the record says so.
    pub fn rescue<D: BlockDevice>(
        &mut self,
        dev: &mut D,
        text: &[u8],
        lost: bool,
    ) -> Result<usize, TraceError> {
        self.append_flagged(dev, text, lost, OS_RESCUED)
    }

    fn append_flagged<D: BlockDevice>(
        &mut self,
        dev: &mut D,
        text: &[u8],
        lost: bool,
        extra: u32,
    ) -> Result<usize, TraceError> {
        let io = |_| TraceError::Io;
        let used = self.len();
        let n = text.len().min(OS_REGION - used);
        let mut header = self.header;
        header.flags |= extra;
        if lost || n < text.len() {
            header.flags |= OS_OVERFLOW;
        }
        if n == 0 && header == self.header {
            return Ok(0);
        }
        let mut pos = used;
        let mut rest = &text[..n];
        let sector_of = |pos: usize| self.lba + OS_AT + (pos / SECTOR) as u64;
        // The partial sector first: what is on the disk, then the new bytes.
        if pos % SECTOR != 0 && !rest.is_empty() {
            let off = pos % SECTOR;
            let take = rest.len().min(SECTOR - off);
            self.tail[off..off + take].copy_from_slice(&rest[..take]);
            dev.write_block(sector_of(pos), &self.tail).map_err(io)?;
            pos += take;
            rest = &rest[take..];
        }
        // Whole sectors straight from the text.
        let whole = rest.len() / SECTOR * SECTOR;
        if whole > 0 {
            dev.write_blocks(sector_of(pos), &rest[..whole]).map_err(io)?;
            pos += whole;
            rest = &rest[whole..];
        }
        // A new partial sector: kept here for the next append.
        if !rest.is_empty() {
            self.tail = [0u8; SECTOR];
            self.tail[..rest.len()].copy_from_slice(rest);
            dev.write_block(sector_of(pos), &self.tail).map_err(io)?;
            pos += rest.len();
        }
        header.os_len = pos as u32;
        dev.write_block(self.lba, &header.encode()).map_err(io)?;
        dev.sync().map_err(io)?;
        self.header = header;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdisk::{disk, part_of, Torn};
    use super::super::{records, LoaderTrace, LOADER_COMPLETE, OS_COMPLETE};
    use super::*;
    use alloc::vec::Vec;

    /// A boot as the loader leaves it: its slot, its text, complete.
    fn booted(dev: &mut crate::MemBlockDevice, text: &[u8]) -> (u64, u64) {
        let mut t = LoaderTrace::open(dev).expect("loader");
        t.write(dev, text, false, true).expect("loader text");
        (t.slot_lba(), t.seq())
    }

    fn stream(len: usize) -> Vec<u8> {
        (0..len).map(|i| b'a' + (i % 26) as u8).collect()
    }

    #[test]
    fn the_os_text_follows_the_loaders_across_sectors_and_a_reopen() {
        let mut dev = disk();
        let (lba, seq) = booted(&mut dev, b"nxboot: jump slot=a\n");
        let text = stream(3000);
        let mut os = OsTrace::open(&dev, &part_of(&dev), lba, seq).expect("this boot");
        assert_eq!(os.append(&mut dev, &text[..3], false), Ok(3));
        assert_eq!(os.append(&mut dev, &text[3..700], false), Ok(697));
        // The block owner restarted: a new writer continues where the region ends.
        let mut os = OsTrace::open(&dev, &part_of(&dev), lba, seq).expect("reopen");
        assert_eq!(os.len(), 700);
        assert_eq!(os.append(&mut dev, &text[700..], false), Ok(2300));
        let got = records(&dev, &part_of(&dev)).expect("records").pop().expect("record");
        assert_eq!(got.loader, b"nxboot: jump slot=a\n", "the loader's region as it was");
        assert_eq!(got.os, text);
        assert_eq!(got.header.flags, LOADER_COMPLETE, "the loader's flags kept, no OS flag");
    }

    #[test]
    fn the_next_loader_rescues_the_tail_the_block_owner_never_wrote() {
        let mut dev = disk();
        let (lba, seq) = booted(&mut dev, b"nxboot: jump slot=a\n");
        let text = stream(2000);
        let part = part_of(&dev);
        let mut os = OsTrace::open(&dev, &part, lba, seq).expect("this boot");
        os.append(&mut dev, &text[..700], false).expect("what the owner kept");
        // The next boot: its loader names the previous slot and finds the ring's rest.
        let next = LoaderTrace::open(&dev).expect("next boot");
        assert_eq!(next.previous(), Some((lba, seq)));
        let mut prev = OsTrace::open(&dev, next.partition(), lba, seq).expect("previous boot");
        assert_eq!(prev.len(), 700, "the loader continues where the owner stopped");
        assert_eq!(prev.rescue(&mut dev, &text[700..], false), Ok(1300));
        let got = records(&dev, &part).expect("records");
        let rec = got.iter().find(|r| r.header.seq == seq).expect("the previous boot");
        assert_eq!(rec.os, text);
        assert_eq!(rec.header.flags & (OS_RESCUED | OS_OVERFLOW), OS_RESCUED);
        // A gap the ring had already overwritten is flagged too.
        let mut prev = OsTrace::open(&dev, &part, lba, seq).expect("again");
        prev.rescue(&mut dev, b"late\n", true).expect("gap");
        let got = records(&dev, &part).expect("records");
        let rec = got.iter().find(|r| r.header.seq == seq).expect("the previous boot");
        assert_eq!(rec.header.flags & OS_OVERFLOW, OS_OVERFLOW);
        // The first boot has no previous one.
        let mut fresh = disk();
        assert_eq!(LoaderTrace::open(&fresh).expect("first").previous(), None);
        let _ = booted(&mut fresh, b"x\n");
    }

    #[test]
    fn test_reject_a_slot_that_does_not_hold_this_boot() {
        let mut dev = disk();
        let (lba, seq) = booted(&mut dev, b"one\n");
        let part = part_of(&dev);
        let reject =
            |dev: &crate::MemBlockDevice, lba, seq| OsTrace::open(dev, &part, lba, seq).err();
        assert_eq!(reject(&dev, lba, seq + 1), Some(TraceError::NotThisBoot), "another boot's seq");
        assert_eq!(reject(&dev, lba, 0), Some(TraceError::NotThisBoot), "no boot");
        // Slot 1 is empty: no record, nothing to append to.
        let next = lba + (crate::trace::SLOT_BYTES / SECTOR) as u64;
        assert_eq!(reject(&dev, next, seq + 1), Some(TraceError::NotThisBoot), "an empty slot");
        // The loader's record copied to slot 1's place: not where boot `seq` lives, and not the
        // record of the boot slot 1 belongs to.
        let mut sector = [0u8; SECTOR];
        dev.read_block(lba, &mut sector).expect("header");
        let mut dev = dev;
        dev.write_block(next, &sector).expect("copy");
        assert_eq!(reject(&dev, next, seq), Some(TraceError::NotThisBoot), "another slot's place");
        assert_eq!(reject(&dev, next, seq + 1), Some(TraceError::NotThisBoot), "a stray record");
        // An address outside the partition.
        assert_eq!(reject(&dev, 7, seq), Some(TraceError::NotThisBoot), "not a slot at all");
    }

    #[test]
    fn test_reject_text_past_the_region_and_text_the_ring_lost() {
        let mut dev = disk();
        let (lba, seq) = booted(&mut dev, b"b\n");
        let mut os = OsTrace::open(&dev, &part_of(&dev), lba, seq).expect("open");
        let big = stream(OS_REGION + 100);
        assert_eq!(os.append(&mut dev, &big, false), Ok(OS_REGION));
        assert_eq!(os.append(&mut dev, b"more", false), Ok(0));
        let got = records(&dev, &part_of(&dev)).expect("records").pop().expect("record");
        assert_eq!(got.os.len(), OS_REGION);
        assert_eq!(got.header.flags & (OS_OVERFLOW | OS_COMPLETE), OS_OVERFLOW);
        let mut dev = disk();
        let (lba, seq) = booted(&mut dev, b"b\n");
        let mut os = OsTrace::open(&dev, &part_of(&dev), lba, seq).expect("open");
        assert_eq!(os.append(&mut dev, b"after a gap\n", true), Ok(12));
        let got = records(&dev, &part_of(&dev)).expect("records").pop().expect("record");
        assert_eq!(
            (got.os.as_slice(), got.header.flags & OS_OVERFLOW),
            (&b"after a gap\n"[..], OS_OVERFLOW)
        );
    }

    #[test]
    fn test_reject_an_append_torn_before_its_header_it_stays_invisible() {
        let mut dev = disk();
        let (lba, seq) = booted(&mut dev, b"l\n");
        let mut os = OsTrace::open(&dev, &part_of(&dev), lba, seq).expect("open");
        os.append(&mut dev, b"kept\n", false).expect("first");
        // The power goes at the header write: the text is on the disk, the header is not.
        let mut torn = Torn { dev, fails: |at, _: &[u8]| at == lba };
        assert_eq!(os.append(&mut torn, b"lost in the cut\n", false), Err(TraceError::Io));
        let got = records(&torn.dev, &part_of(&torn.dev)).expect("records").pop().expect("record");
        assert_eq!(got.os, b"kept\n", "only what a header claims is shown");
    }
}
