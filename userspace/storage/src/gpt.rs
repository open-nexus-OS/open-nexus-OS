// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded, read-only GPT parsing + `PartitionView` (ADR-0044): the
//! shared partition seam under statefsd/nxfsd. Fail-closed — a bad header or
//! entry CRC yields an error, never a guessed layout. Write/format tooling
//! stays host-side (`scripts/mk-gpt-image.py`); services only READ the table.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0293)
//! TEST_COVERAGE: parse/reject + view-bounds tests below

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::{BlockDevice, BlockError};

/// GPT partition-type GUID for the nexus `state` partition (statefs journal).
pub const GUID_NEXUS_STATE: [u8; 16] = *b"NEXUS-STATE-v1\0\0";
/// GPT partition-type GUID for the nexus `data` partition (nxfs container).
pub const GUID_NEXUS_DATA: [u8; 16] = *b"NEXUS-DATA-v1\0\0\0";

const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
const HEADER_LBA: u64 = 1;
/// Bounded entry scan (standard GPT default is 128).
const MAX_ENTRIES: u32 = 128;
const ENTRY_SIZE: usize = 128;

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
        let first_lba = u64::from_le_bytes(entry[32..40].try_into().map_err(|_| GptError::Invalid)?);
        let last_lba = u64::from_le_bytes(entry[40..48].try_into().map_err(|_| GptError::Invalid)?);
        if first_lba == 0 || last_lba < first_lba || last_lba >= device_blocks {
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

/// Builds a minimal valid GPT (host tooling + tests): protective MBR is
/// omitted (we never boot from these images), primary header at LBA 1,
/// entries at LBA 2. Used by tests and by the launcher-side image formatter.
pub fn write_gpt<D: BlockDevice>(
    device: &mut D,
    partitions: &[Partition],
) -> Result<(), GptError> {
    let sector = device.block_size();
    if sector < 128 || partitions.len() > MAX_ENTRIES as usize {
        return Err(GptError::Invalid);
    }
    let entry_count: u32 = 128;
    let table_bytes = entry_count as usize * ENTRY_SIZE;
    let mut table = vec![0u8; table_bytes];
    for (idx, partition) in partitions.iter().enumerate() {
        let entry = &mut table[idx * ENTRY_SIZE..(idx + 1) * ENTRY_SIZE];
        entry[0..16].copy_from_slice(&partition.type_guid);
        entry[16..32].copy_from_slice(&partition.type_guid); // unique GUID: reuse type (dev images)
        entry[32..40].copy_from_slice(&partition.first_lba.to_le_bytes());
        entry[40..48].copy_from_slice(&partition.last_lba.to_le_bytes());
        for (i, ch) in partition.name.bytes().take(35).enumerate() {
            entry[56 + i * 2] = ch;
        }
    }
    let entries_crc = crc32_ieee(&table);

    let mut header = vec![0u8; sector];
    header[0..8].copy_from_slice(GPT_SIGNATURE);
    header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // revision 1.0
    header[12..16].copy_from_slice(&92u32.to_le_bytes()); // header size
    header[24..32].copy_from_slice(&HEADER_LBA.to_le_bytes()); // my LBA
    header[72..80].copy_from_slice(&2u64.to_le_bytes()); // entries LBA
    header[80..84].copy_from_slice(&entry_count.to_le_bytes());
    header[84..88].copy_from_slice(&(ENTRY_SIZE as u32).to_le_bytes());
    header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    let crc = crc32_ieee(&header[..92]);
    header[16..20].copy_from_slice(&crc.to_le_bytes());

    device.write_block(HEADER_LBA, &header).map_err(|_| GptError::Io)?;
    let table_blocks = table_bytes.div_ceil(sector);
    let mut padded = table;
    padded.resize(table_blocks * sector, 0);
    for i in 0..table_blocks as u64 {
        let offset = (i as usize) * sector;
        device
            .write_block(2 + i, &padded[offset..offset + sector])
            .map_err(|_| GptError::Io)?;
    }
    device.sync().map_err(|_| GptError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemBlockDevice;

    fn image() -> MemBlockDevice {
        let mut device = MemBlockDevice::new(512, 4096);
        write_gpt(
            &mut device,
            &[
                Partition {
                    type_guid: GUID_NEXUS_STATE,
                    first_lba: 64,
                    last_lba: 1063,
                    name: "state".into(),
                },
                Partition {
                    type_guid: GUID_NEXUS_DATA,
                    first_lba: 1064,
                    last_lba: 4000,
                    name: "data".into(),
                },
            ],
        )
        .expect("write gpt");
        device
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

    #[test]
    fn test_reject_corrupt_gpt() {
        // No signature.
        let device = MemBlockDevice::new(512, 4096);
        assert_eq!(parse_gpt(&device), Err(GptError::NoGpt));
        // Corrupt header CRC.
        let mut device = image();
        let mut header = vec![0u8; 512];
        device.read_block(1, &mut header).expect("read");
        header[40] ^= 0xFF;
        device.write_block(1, &header).expect("write");
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // Corrupt entry array.
        let mut device = image();
        let mut entries = vec![0u8; 512];
        device.read_block(2, &mut entries).expect("read");
        entries[33] ^= 0x01;
        device.write_block(2, &entries).expect("write");
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
        // Partition beyond the device.
        let mut device = MemBlockDevice::new(512, 4096);
        write_gpt(
            &mut device,
            &[Partition {
                type_guid: GUID_NEXUS_DATA,
                first_lba: 64,
                last_lba: 9999,
                name: "data".into(),
            }],
        )
        .expect("write gpt");
        assert_eq!(parse_gpt(&device), Err(GptError::Invalid));
    }
}
