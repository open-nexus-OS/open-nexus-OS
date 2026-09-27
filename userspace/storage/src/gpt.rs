// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded, read-only GPT parsing + `PartitionView` (ADR-0044): the
//! shared partition seam under statefsd/nxfsd. Fail-closed — a bad header or
//! entry CRC, a header that is not the primary, or a partition outside the
//! usable range yields an error, never a guessed layout. The writer (`nx image`
//! and host tests; services only READ) builds a complete GPT for the disk it
//! writes (TASK-0260 P1, RFC-0089 §2): the protective MBR in sector 0 — U-Boot's
//! GPT driver, and so the board's SPL, sees no GPT without one — the primary
//! header and entries, and the backup entries and header in the disk's last
//! sectors. The boot code area of sector 0 stays zero here (the board's boot-ROM
//! header is `nx image`'s).
//! OWNERS: @runtime
//! STATUS: Functional (TASK-0293; the complete writer since TASK-0260 P1)
//! TEST_COVERAGE: parse/reject + view-bounds + writer tests below

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::{BlockDevice, BlockError};

/// GPT partition-type GUID for the nexus `state` partition (statefs journal).
pub const GUID_NEXUS_STATE: [u8; 16] = *b"NEXUS-STATE-v1\0\0";
/// GPT partition-type GUID for the nexus `data` partition (nxfs container).
pub const GUID_NEXUS_DATA: [u8; 16] = *b"NEXUS-DATA-v1\0\0\0";
/// GPT partition-type GUID for the boot-selection-block partition
/// (RFC-0089 §6; write matrix ADR-0058).
pub const GUID_NEXUS_BSB: [u8; 16] = *b"NEXUS-BSB-v1\0\0\0\0";
/// GPT partition-type GUID for a boot-image slot (`boot-a`/`boot-b`,
/// RFC-0089 §2/§5 — NXBD at sector 0, image from sector 8; the NAME
/// distinguishes the slots, the type is shared).
pub const GUID_NEXUS_BOOT: [u8; 16] = *b"NEXUS-BOOT-v1\0\0\0";
/// GPT partition-type GUID for a reserved system volume (`system-a`/`-b`,
/// RFC-0089 §12 Phase B — empty until the bundle-set phase).
pub const GUID_NEXUS_SYS: [u8; 16] = *b"NEXUS-SYS-v1\0\0\0\0";
/// GPT partition-type GUID for the board's boot-ROM head (`fsbl`, `env`, `opensbi`,
/// `uboot` — RFC-0089 §2 amendment 2026-09-26): firmware no stage of ours reads or writes
/// at run time.
pub const GUID_NEXUS_FW: [u8; 16] = *b"NEXUS-FW-v1\0\0\0\0\0";
/// GPT partition-type GUID for the boot trace (RFC-0107): each boot's console text.
pub const GUID_NEXUS_TRACE: [u8; 16] = *b"NEXUS-TRACE-v1\0\0";
/// The disk GUID every image carries (deterministic builds); the partitions' unique GUIDs
/// derive from it and their position.
pub const NEXUS_DISK_GUID: [u8; 16] = *b"NEXUS-DISK-v1\0\0\0";

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const HEADER_LBA: u64 = 1;
/// Bounded entry scan (standard GPT default is 128).
const MAX_ENTRIES: u32 = 128;
const ENTRY_SIZE: usize = 128;
/// A partition name holds at most this many UTF-16 code units.
const NAME_UNITS: usize = 36;
/// The protective MBR's one entry and its signature (UEFI: type `0xee` from LBA 1).
const PMBR_ENTRY: usize = 446;
const PMBR_TYPE_GPT: u8 = 0xEE;

/// One parsed partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    /// Partition-type GUID (16 bytes, verbatim).
    pub type_guid: [u8; 16],
    /// First LBA (inclusive), in DEVICE blocks.
    pub first_lba: u64,
    /// Last LBA (inclusive), in DEVICE blocks.
    pub last_lba: u64,
    /// UTF-16LE name decoded lossily to ASCII (diagnostics only).
    pub name: String,
}

/// GPT parse errors (fail-closed; the caller reports and stays down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GptError {
    /// Device IO failed.
    Io,
    /// No GPT signature at LBA 1.
    NoGpt,
    /// Header or entry-array CRC mismatch, or structurally invalid fields.
    Invalid,
}

/// Parses the primary GPT (LBA 1 + entry array). Bounded: at most
/// [`MAX_ENTRIES`] entries of the standard 128-byte size are examined.
pub fn parse_gpt<D: BlockDevice>(device: &D) -> Result<Vec<Partition>, GptError> {
    let sector = device.block_size();
    if sector < 92 {
        return Err(GptError::Invalid);
    }
    let mut header = vec![0u8; sector];
    device.read_block(HEADER_LBA, &mut header).map_err(|_| GptError::Io)?;
    if &header[0..8] != GPT_SIGNATURE {
        return Err(GptError::NoGpt);
    }
    // Header CRC: field zeroed during computation, over header_size bytes.
    let header_size = u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
    if !(92..=sector).contains(&header_size) {
        return Err(GptError::Invalid);
    }
    let stored_crc = u32::from_le_bytes([header[16], header[17], header[18], header[19]]);
    let mut scratch = header[..header_size].to_vec();
    scratch[16..20].fill(0);
    if crc32_ieee(&scratch) != stored_crc {
        return Err(GptError::Invalid);
    }

    // The primary header points at itself; anything else (a backup copied to LBA 1, a
    // foreign structure) is refused.
    let my_lba = u64::from_le_bytes(header[24..32].try_into().map_err(|_| GptError::Invalid)?);
    let first_usable =
        u64::from_le_bytes(header[40..48].try_into().map_err(|_| GptError::Invalid)?);
    let last_usable = u64::from_le_bytes(header[48..56].try_into().map_err(|_| GptError::Invalid)?);
    if my_lba != HEADER_LBA || first_usable > last_usable {
        return Err(GptError::Invalid);
    }
    let entries_lba = u64::from_le_bytes(header[72..80].try_into().map_err(|_| GptError::Invalid)?);
    let entry_count = u32::from_le_bytes(header[80..84].try_into().map_err(|_| GptError::Invalid)?);
    let entry_size =
        u32::from_le_bytes(header[84..88].try_into().map_err(|_| GptError::Invalid)?) as usize;
    let entries_crc = u32::from_le_bytes(header[88..92].try_into().map_err(|_| GptError::Invalid)?);
    if entry_size != ENTRY_SIZE || entry_count == 0 || entry_count > MAX_ENTRIES {
        return Err(GptError::Invalid);
    }

    let table_bytes = entry_count as usize * entry_size;
    let table_blocks = table_bytes.div_ceil(sector);
    let mut table = vec![0u8; table_blocks * sector];
    for i in 0..table_blocks as u64 {
        let offset = (i as usize) * sector;
        device
            .read_block(entries_lba + i, &mut table[offset..offset + sector])
            .map_err(|_| GptError::Io)?;
    }
    if crc32_ieee(&table[..table_bytes]) != entries_crc {
        return Err(GptError::Invalid);
    }

    let device_blocks = device.block_count();
    let mut partitions = Vec::new();
    for idx in 0..entry_count as usize {
        let entry = &table[idx * entry_size..(idx + 1) * entry_size];
        let mut type_guid = [0u8; 16];
        type_guid.copy_from_slice(&entry[0..16]);
        if type_guid == [0u8; 16] {
            continue; // unused slot
        }
        let first_lba =
            u64::from_le_bytes(entry[32..40].try_into().map_err(|_| GptError::Invalid)?);
        let last_lba = u64::from_le_bytes(entry[40..48].try_into().map_err(|_| GptError::Invalid)?);
        if first_lba < first_usable
            || last_lba > last_usable
            || last_lba < first_lba
            || last_lba >= device_blocks
        {
            return Err(GptError::Invalid);
        }
        let mut name = String::new();
        for pair in entry[56..128].chunks_exact(2) {
            let code = u16::from_le_bytes([pair[0], pair[1]]);
            if code == 0 {
                break;
            }
            name.push(if (0x20..0x7F).contains(&code) { code as u8 as char } else { '?' });
        }
        partitions.push(Partition { type_guid, first_lba, last_lba, name });
    }
    Ok(partitions)
}

/// Finds the partition with `type_guid`.
/// Finds a partition by type GUID AND name — required where one type is
/// shared by several partitions (`boot-a`/`boot-b`, `system-a`/`-b`).
pub fn find_partition_named(
    partitions: &[Partition],
    type_guid: &[u8; 16],
    name: &str,
) -> Option<Partition> {
    partitions.iter().find(|p| &p.type_guid == type_guid && p.name == name).cloned()
}

pub fn find_partition(partitions: &[Partition], type_guid: &[u8; 16]) -> Option<Partition> {
    partitions.iter().find(|p| &p.type_guid == type_guid).cloned()
}

/// A bounds-checked window over a [`BlockDevice`] — the partition seam both
/// storage services consume (ADR-0044). Never reads or writes outside
/// `[first_lba, last_lba]`.
pub struct PartitionView<D: BlockDevice> {
    inner: D,
    first_lba: u64,
    blocks: u64,
}

impl<D: BlockDevice> PartitionView<D> {
    /// Creates a view; fails closed if the range exceeds the device.
    pub fn new(inner: D, partition: &Partition) -> Result<Self, GptError> {
        if partition.last_lba >= inner.block_count() || partition.last_lba < partition.first_lba {
            return Err(GptError::Invalid);
        }
        Ok(Self {
            inner,
            first_lba: partition.first_lba,
            blocks: partition.last_lba - partition.first_lba + 1,
        })
    }

    /// Consumes the view, returning the underlying device.
    pub fn into_inner(self) -> D {
        self.inner
    }
}

impl<D: BlockDevice> BlockDevice for PartitionView<D> {
    fn block_size(&self) -> usize {
        self.inner.block_size()
    }

    fn block_count(&self) -> u64 {
        self.blocks
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        if block_idx >= self.blocks {
            return Err(BlockError::OutOfRange);
        }
        self.inner.read_block(self.first_lba + block_idx, buf)
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        if block_idx >= self.blocks {
            return Err(BlockError::OutOfRange);
        }
        self.inner.write_block(self.first_lba + block_idx, buf)
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        self.inner.sync()
    }
}

/// crc32 (IEEE, as GPT mandates) — bitwise, table-free, no_std.
#[must_use]
pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = !0;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Sectors the 128-entry array spans.
fn entry_sectors(sector: usize) -> u64 {
    (MAX_ENTRIES as usize * ENTRY_SIZE).div_ceil(sector) as u64
}

/// The first sector a partition may use behind the primary GPT (34 at 512 bytes).
pub fn first_usable_lba(sector: usize) -> u64 {
    HEADER_LBA + 1 + entry_sectors(sector)
}

/// The last sector a partition may use in front of the backup GPT of a disk of
/// `disk_sectors`; `None` when the disk cannot hold both copies.
pub fn last_usable_lba(disk_sectors: u64, sector: usize) -> Option<u64> {
    disk_sectors.checked_sub(2 + entry_sectors(sector)).filter(|l| *l >= first_usable_lba(sector))
}

/// Writes a complete GPT for the disk `device` is (`block_count` sectors): the protective MBR
/// in sector 0, the primary header and entries, the backup entries and header in the last
/// sectors. Refuses partitions outside the usable range, overlapping or badly named. The
/// partitions' unique GUIDs are [`NEXUS_DISK_GUID`] with the entry's 1-based index in its last
/// two bytes — deterministic and distinct.
pub fn write_gpt<D: BlockDevice>(device: &mut D, partitions: &[Partition]) -> Result<(), GptError> {
    let sector = device.block_size();
    let disk = device.block_count();
    if sector < 512 || partitions.len() > MAX_ENTRIES as usize {
        return Err(GptError::Invalid);
    }
    let first_usable = first_usable_lba(sector);
    let last_usable = last_usable_lba(disk, sector).ok_or(GptError::Invalid)?;
    let mut spans: Vec<(u64, u64)> = partitions.iter().map(|p| (p.first_lba, p.last_lba)).collect();
    spans.sort_unstable();
    for (i, &(first, last)) in spans.iter().enumerate() {
        let overlaps = i > 0 && first <= spans[i - 1].1;
        if first < first_usable || last > last_usable || last < first || overlaps {
            return Err(GptError::Invalid);
        }
    }

    let mut table = vec![0u8; MAX_ENTRIES as usize * ENTRY_SIZE];
    for (idx, partition) in partitions.iter().enumerate() {
        let units: Vec<u16> = partition.name.encode_utf16().collect();
        if units.is_empty() || units.len() > NAME_UNITS {
            return Err(GptError::Invalid);
        }
        let entry = &mut table[idx * ENTRY_SIZE..(idx + 1) * ENTRY_SIZE];
        entry[0..16].copy_from_slice(&partition.type_guid);
        entry[16..32].copy_from_slice(&unique_guid(idx));
        entry[32..40].copy_from_slice(&partition.first_lba.to_le_bytes());
        entry[40..48].copy_from_slice(&partition.last_lba.to_le_bytes());
        for (i, unit) in units.iter().enumerate() {
            entry[56 + i * 2..58 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    let entries_crc = crc32_ieee(&table);
    let last = disk - 1;
    let backup_entries = last - entry_sectors(sector);
    let header = |my_lba: u64, alternate: u64, entries_lba: u64| {
        let mut h = vec![0u8; sector];
        h[0..8].copy_from_slice(GPT_SIGNATURE);
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // revision 1.0
        h[12..16].copy_from_slice(&92u32.to_le_bytes()); // header size
        h[24..32].copy_from_slice(&my_lba.to_le_bytes());
        h[32..40].copy_from_slice(&alternate.to_le_bytes());
        h[40..48].copy_from_slice(&first_usable.to_le_bytes());
        h[48..56].copy_from_slice(&last_usable.to_le_bytes());
        h[56..72].copy_from_slice(&NEXUS_DISK_GUID);
        h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        h[80..84].copy_from_slice(&MAX_ENTRIES.to_le_bytes());
        h[84..88].copy_from_slice(&(ENTRY_SIZE as u32).to_le_bytes());
        h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let crc = crc32_ieee(&h[..92]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    };

    let io = |_| GptError::Io;
    device.write_block(0, &protective_mbr(disk, sector)).map_err(io)?;
    device.write_block(HEADER_LBA, &header(HEADER_LBA, last, 2)).map_err(io)?;
    let mut padded = table;
    padded.resize(entry_sectors(sector) as usize * sector, 0);
    device.write_blocks(2, &padded).map_err(io)?;
    device.write_blocks(backup_entries, &padded).map_err(io)?;
    device.write_block(last, &header(last, HEADER_LBA, backup_entries)).map_err(io)?;
    device.sync().map_err(io)
}

/// Sector 0 of a GPT disk: one entry of type `0xee` from LBA 1 over the disk (capped at the
/// field's 32 bits), CHS as UEFI sets them, the `0x55aa` signature; the boot code area zero.
pub fn protective_mbr(disk_sectors: u64, sector: usize) -> Vec<u8> {
    let mut mbr = vec![0u8; sector.max(512)];
    let size = u32::try_from(disk_sectors.saturating_sub(1)).unwrap_or(u32::MAX);
    let entry = &mut mbr[PMBR_ENTRY..PMBR_ENTRY + 16];
    entry[1..4].copy_from_slice(&[0x00, 0x02, 0x00]); // start CHS
    entry[4] = PMBR_TYPE_GPT;
    entry[5..8].copy_from_slice(&[0xFF, 0xFF, 0xFF]); // end CHS: not representable
    entry[8..12].copy_from_slice(&1u32.to_le_bytes());
    entry[12..16].copy_from_slice(&size.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    mbr
}

fn unique_guid(idx: usize) -> [u8; 16] {
    let mut guid = NEXUS_DISK_GUID;
    guid[14..16].copy_from_slice(&(idx as u16 + 1).to_le_bytes());
    guid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemBlockDevice;

    const DISK: u64 = 4096;

    fn part(name: &str, type_guid: [u8; 16], first_lba: u64, last_lba: u64) -> Partition {
        Partition { type_guid, first_lba, last_lba, name: name.into() }
    }

    fn image() -> MemBlockDevice {
        let mut device = MemBlockDevice::new(512, DISK);
        write_gpt(
            &mut device,
            &[part("state", GUID_NEXUS_STATE, 64, 1063), part("data", GUID_NEXUS_DATA, 1064, 4000)],
        )
        .expect("write gpt");
        device
    }

    fn sector(device: &MemBlockDevice, lba: u64) -> Vec<u8> {
        let mut buf = vec![0u8; 512];
        device.read_block(lba, &mut buf).expect("read");
        buf
    }

    fn u64_at(buf: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(buf[at..at + 8].try_into().expect("8 bytes"))
    }

    /// Re-seals the primary header after a test edited its entries or fields.
    fn reseal(device: &mut MemBlockDevice, edit: impl FnOnce(&mut Vec<u8>, &mut Vec<u8>)) {
        let mut header = sector(device, 1);
        let mut entries = vec![0u8; 128 * 128];
        device.read_blocks(2, &mut entries).expect("entries");
        edit(&mut header, &mut entries);
        header[88..92].copy_from_slice(&crc32_ieee(&entries).to_le_bytes());
        header[16..20].fill(0);
        let crc = crc32_ieee(&header[..92]);
        header[16..20].copy_from_slice(&crc.to_le_bytes());
        device.write_blocks(2, &entries).expect("entries");
        device.write_block(1, &header).expect("header");
    }

    #[test]
    fn gpt_roundtrip_and_view_bounds() {
        let device = image();
        let partitions = parse_gpt(&device).expect("parse");
        assert_eq!(partitions.len(), 2);
        let data = find_partition(&partitions, &GUID_NEXUS_DATA).expect("data");
        assert_eq!(data.name, "data");
        let mut view = PartitionView::new(device, &data).expect("view");
        assert_eq!(view.block_count(), 4000 - 1064 + 1);
        let payload = [0xAB; 512];
        view.write_block(0, &payload).expect("write");
        let mut back = [0u8; 512];
        view.read_block(0, &mut back).expect("read");
        assert_eq!(back, payload);
        // Bounds: outside the window fails closed.
        assert_eq!(view.write_block(view.block_count(), &payload), Err(BlockError::OutOfRange));
        // The write landed at the partition base on the raw device.
        let device = view.into_inner();
        device.read_block(1064, &mut back).expect("raw read");
        assert_eq!(back, payload);
    }

    /// TASK-0260 P1: the protective MBR, both headers and both entry arrays, as the board's
    /// boot medium carries them (docs/board/measurements/2026-09-26-boot-medium).
    #[test]
    fn the_writer_lays_down_a_complete_gpt_for_its_disk() {
        let device = image();
        let mbr = sector(&device, 0);
        assert!(mbr[..446].iter().all(|b| *b == 0), "the boot code area stays free");
        assert_eq!(mbr[450], 0xEE, "one protective entry");
        assert_eq!(&mbr[454..458], &1u32.to_le_bytes(), "from LBA 1");
        assert_eq!(&mbr[458..462], &((DISK - 1) as u32).to_le_bytes(), "over the disk");
        assert_eq!(&mbr[510..512], &[0x55, 0xAA]);
        let primary = sector(&device, 1);
        assert_eq!(u64_at(&primary, 24), 1);
        assert_eq!(u64_at(&primary, 32), DISK - 1, "the backup at the disk's last sector");
        assert_eq!((u64_at(&primary, 40), u64_at(&primary, 48)), (34, DISK - 34));
        assert_eq!(&primary[56..72], &NEXUS_DISK_GUID);
        let backup = sector(&device, DISK - 1);
        assert_eq!(&backup[0..8], GPT_SIGNATURE);
        assert_eq!((u64_at(&backup, 24), u64_at(&backup, 32)), (DISK - 1, 1));
        assert_eq!(u64_at(&backup, 72), DISK - 33, "its entries right in front of it");
        let mut sealed = backup[..92].to_vec();
        sealed[16..20].fill(0);
        assert_eq!(&backup[16..20], &crc32_ieee(&sealed).to_le_bytes());
        for i in 0..32 {
            assert_eq!(sector(&device, 2 + i), sector(&device, DISK - 33 + i), "entries {i}");
        }
        let entries = sector(&device, 2);
        assert_ne!(&entries[16..32], &entries[128 + 16..128 + 32], "distinct unique GUIDs");
    }

    #[test]
    fn test_reject_a_layout_the_disk_cannot_hold() {
        let refuse = |parts: &[Partition]| {
            let mut device = MemBlockDevice::new(512, DISK);
            write_gpt(&mut device, parts)
        };
        let beyond = [part("data", GUID_NEXUS_DATA, 64, DISK - 1)];
        assert_eq!(refuse(&beyond), Err(GptError::Invalid), "over the backup GPT");
        let early = [part("data", GUID_NEXUS_DATA, 33, 100)];
        assert_eq!(refuse(&early), Err(GptError::Invalid), "over the primary entries");
        let overlap =
            [part("state", GUID_NEXUS_STATE, 64, 200), part("data", GUID_NEXUS_DATA, 200, 300)];
        assert_eq!(refuse(&overlap), Err(GptError::Invalid));
        let long = [part(&"n".repeat(37), GUID_NEXUS_DATA, 64, 100)];
        assert_eq!(refuse(&long), Err(GptError::Invalid), "37 UTF-16 units");
        assert_eq!(refuse(&[part("", GUID_NEXUS_DATA, 64, 100)]), Err(GptError::Invalid));
    }

    #[test]
    fn test_reject_corrupt_gpt() {
        // No signature.
        let device = MemBlockDevice::new(512, DISK);
        assert_eq!(parse_gpt(&device), Err(GptError::NoGpt));
        // Corrupt header CRC.
        let mut device = image();
        let mut header = sector(&device, 1);
        header[40] ^= 0xFF;
        device.write_block(1, &header).expect("write");
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // Corrupt entry array.
        let mut device = image();
        let mut entries = sector(&device, 2);
        entries[33] ^= 0x01;
        device.write_block(2, &entries).expect("write");
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // The backup header copied to LBA 1: sealed, but not the primary.
        let mut device = image();
        let backup = sector(&device, DISK - 1);
        device.write_block(1, &backup).expect("write");
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // A partition over the backup GPT, correctly sealed.
        let mut device = image();
        reseal(&mut device, |_, entries| {
            entries[128 + 40..128 + 48].copy_from_slice(&(DISK - 2).to_le_bytes());
        });
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // A partition over the primary entries, correctly sealed.
        let mut device = image();
        reseal(&mut device, |_, entries| entries[32..40].copy_from_slice(&2u64.to_le_bytes()));
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // A usable range that is empty.
        let mut device = image();
        reseal(&mut device, |header, _| header[48..56].copy_from_slice(&10u64.to_le_bytes()));
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
    }
}
