// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The firmware image's documented layout (TASK-0329 P1): an ELF32 padded to 4 KiB
//! blocks, an info header in the last block (version 3, 48 bytes), the layout table after it
//! (up to eight 24-byte entries: section id, kind, firmware virtual base, max, alloc size,
//! alloc offset), and a device-info block before the header (four `u64` sizes, then the quirk,
//! enhancement and feature masks and the valued features' parameters). The parser refuses the
//! wrong blob before a byte is placed (`test_reject_firmware_*`); the ELF's `PT_LOAD` segments
//! become placements into the code and data regions at `alloc_offset + (vaddr − base)`.
//! Every access is bounds-checked: the blob is input, never trusted.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (synthetic images) + `tests/gpu_goldens.rs` (the published blob)

use super::CoreId;

pub use super::fw_features::{DeviceInfoBlock, Enhancement, Feature, Quirk, FEATURE_COUNT};

/// The image is padded to this.
pub const BLOCK: usize = 4096;
/// The info header's version this parser reads.
pub const INFO_VERSION: u32 = 3;
pub const HEADER_LEN: u32 = 48;
pub const LAYOUT_ENTRY_SIZE: u32 = 24;
pub const MAX_LAYOUT_ENTRIES: u32 = 8;
/// Info header flag bit 0: the open-source build (the one whose interface is documented).
pub const FLAG_OPEN_SOURCE: u32 = 1;
/// The newest firmware major this interface knows.
pub const MAX_SUPPORTED_MAJOR: u16 = 1;

/// Why an image was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FwImageError {
    /// Shorter than one block, or not a whole number of blocks.
    NotBlockAligned,
    /// `info_version` is not 3.
    InfoVersion(u32),
    /// Header length, entry size or entry count disagree with the format.
    HeaderFormat,
    /// The open-source flag is clear — another build with another interface.
    NotOpenBuild,
    /// Major version 0 or above the supported one.
    Version { major: u16, minor: u16 },
    /// The header names another core.
    WrongId { expected: CoreId, found: CoreId },
    /// A layout entry with zero size, or a table that leaves the image.
    Layout,
    /// The device-info block leaves the image.
    DeviceInfo,
    /// Not an ELF32 little-endian image, or its program headers leave the image.
    Elf,
    /// A loadable segment lies in no layout section.
    SegmentOutsideLayout { vaddr: u32, size: u32 },
}

/// A layout section's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SectionId {
    MetaCode = 0,
    MetaPrivateData,
    MetaCorememCode,
    MetaCorememData,
    MipsCode,
    MipsExceptionsCode,
    MipsBootCode,
    MipsPrivateData,
    MipsBootData,
    MipsStack,
    RiscvUncachedCode,
    RiscvCachedCode,
    RiscvPrivateData,
    RiscvCorememCode,
    RiscvCorememData,
}

impl SectionId {
    fn from_raw(raw: u32) -> Option<Self> {
        Some(match raw {
            0 => Self::MetaCode,
            1 => Self::MetaPrivateData,
            2 => Self::MetaCorememCode,
            3 => Self::MetaCorememData,
            4 => Self::MipsCode,
            5 => Self::MipsExceptionsCode,
            6 => Self::MipsBootCode,
            7 => Self::MipsPrivateData,
            8 => Self::MipsBootData,
            9 => Self::MipsStack,
            10 => Self::RiscvUncachedCode,
            11 => Self::RiscvCachedCode,
            12 => Self::RiscvPrivateData,
            13 => Self::RiscvCorememCode,
            14 => Self::RiscvCorememData,
            _ => return None,
        })
    }
}

/// Which region a section is placed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    Code,
    Data,
    CorememCode,
    CorememData,
}

impl SectionKind {
    fn from_raw(raw: u32) -> Option<Option<Self>> {
        Some(match raw {
            0 => None,
            1 => Some(Self::Code),
            2 => Some(Self::Data),
            3 => Some(Self::CorememCode),
            4 => Some(Self::CorememData),
            _ => return None,
        })
    }
}

/// One layout entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutEntry {
    pub id: SectionId,
    /// `None` for an entry of kind NONE (reserved, never placed).
    pub kind: Option<SectionKind>,
    /// The section's base in the firmware processor's own address space.
    pub base_addr: u32,
    pub max_size: u32,
    pub alloc_size: u32,
    /// The section's offset inside its region (code or data).
    pub alloc_offset: u32,
}

impl LayoutEntry {
    fn holds(&self, addr: u32, end: u32) -> bool {
        let start = self.base_addr;
        let Some(stop) = start.checked_add(self.alloc_size) else { return false };
        addr >= start && addr < stop && end > start && end <= stop
    }
}

/// The info header as read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InfoHeader {
    pub core: CoreId,
    pub fw_page_size: u32,
    pub flags: u32,
    pub version_major: u16,
    pub version_minor: u16,
    pub version_build: u32,
    pub device_info_size: u32,
    pub layout_entry_num: u32,
}

/// One loadable segment placed into a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement<'a> {
    pub region: SectionKind,
    /// Offset inside the region.
    pub offset: u32,
    /// The bytes to copy (`p_filesz`).
    pub bytes: &'a [u8],
    /// Bytes to zero after them (`p_memsz − p_filesz`).
    pub zero_tail: u32,
}

/// The parsed image: header, layout, device info and the ELF's placements, all borrowed.
#[derive(Clone, Copy, Debug)]
pub struct FirmwareImage<'a> {
    bytes: &'a [u8],
    header: InfoHeader,
    layout: [Option<LayoutEntry>; MAX_LAYOUT_ENTRIES as usize],
    dev_info: DeviceInfoBlock<'a>,
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

impl<'a> FirmwareImage<'a> {
    /// Parses and validates `bytes` for `expected` — the order of refusals is the open
    /// interface's: blocks, info version, format, open build + version, id, layout, device info.
    pub fn parse(bytes: &'a [u8], expected: CoreId) -> Result<Self, FwImageError> {
        if bytes.len() < BLOCK || bytes.len() % BLOCK != 0 {
            return Err(FwImageError::NotBlockAligned);
        }
        let h = bytes.len() - BLOCK;
        let info_version = u32_at(bytes, h).ok_or(FwImageError::HeaderFormat)?;
        if info_version != INFO_VERSION {
            return Err(FwImageError::InfoVersion(info_version));
        }
        let header_len = u32_at(bytes, h + 4).ok_or(FwImageError::HeaderFormat)?;
        let layout_entry_num = u32_at(bytes, h + 8).ok_or(FwImageError::HeaderFormat)?;
        let layout_entry_size = u32_at(bytes, h + 12).ok_or(FwImageError::HeaderFormat)?;
        if header_len != HEADER_LEN
            || layout_entry_size != LAYOUT_ENTRY_SIZE
            || layout_entry_num > MAX_LAYOUT_ENTRIES
        {
            return Err(FwImageError::HeaderFormat);
        }
        let core = CoreId(u64_at(bytes, h + 16).ok_or(FwImageError::HeaderFormat)?);
        let fw_page_size = u32_at(bytes, h + 24).ok_or(FwImageError::HeaderFormat)?;
        let flags = u32_at(bytes, h + 28).ok_or(FwImageError::HeaderFormat)?;
        let version_major = u16_at(bytes, h + 32).ok_or(FwImageError::HeaderFormat)?;
        let version_minor = u16_at(bytes, h + 34).ok_or(FwImageError::HeaderFormat)?;
        let version_build = u32_at(bytes, h + 36).ok_or(FwImageError::HeaderFormat)?;
        let device_info_size = u32_at(bytes, h + 40).ok_or(FwImageError::HeaderFormat)?;
        if flags & FLAG_OPEN_SOURCE == 0 {
            return Err(FwImageError::NotOpenBuild);
        }
        if version_major == 0 || version_major > MAX_SUPPORTED_MAJOR {
            return Err(FwImageError::Version { major: version_major, minor: version_minor });
        }
        if core != expected {
            return Err(FwImageError::WrongId { expected, found: core });
        }
        let table = h + HEADER_LEN as usize;
        let table_size = (LAYOUT_ENTRY_SIZE * layout_entry_num) as usize;
        if table + table_size > bytes.len() {
            return Err(FwImageError::Layout);
        }
        let mut layout = [None; MAX_LAYOUT_ENTRIES as usize];
        for (i, slot) in layout.iter_mut().enumerate().take(layout_entry_num as usize) {
            let e = table + i * LAYOUT_ENTRY_SIZE as usize;
            let id = SectionId::from_raw(u32_at(bytes, e).ok_or(FwImageError::Layout)?)
                .ok_or(FwImageError::Layout)?;
            let kind = SectionKind::from_raw(u32_at(bytes, e + 4).ok_or(FwImageError::Layout)?)
                .ok_or(FwImageError::Layout)?;
            let entry = LayoutEntry {
                id,
                kind,
                base_addr: u32_at(bytes, e + 8).ok_or(FwImageError::Layout)?,
                max_size: u32_at(bytes, e + 12).ok_or(FwImageError::Layout)?,
                alloc_size: u32_at(bytes, e + 16).ok_or(FwImageError::Layout)?,
                alloc_offset: u32_at(bytes, e + 20).ok_or(FwImageError::Layout)?,
            };
            // A section whose end does not lie beyond its start is empty: refused.
            if entry
                .base_addr
                .checked_add(entry.alloc_size)
                .is_none_or(|end| end <= entry.base_addr)
            {
                return Err(FwImageError::Layout);
            }
            *slot = Some(entry);
        }
        let dev_info = DeviceInfoBlock::parse(bytes, h, device_info_size)?;
        let header = InfoHeader {
            core,
            fw_page_size,
            flags,
            version_major,
            version_minor,
            version_build,
            device_info_size,
            layout_entry_num,
        };
        Ok(Self { bytes, header, layout, dev_info })
    }

    pub const fn header(&self) -> &InfoHeader {
        &self.header
    }

    /// The layout entries, in table order.
    pub fn layout(&self) -> impl Iterator<Item = &LayoutEntry> {
        self.layout.iter().flatten()
    }

    /// The entry with `id`.
    pub fn section(&self, id: SectionId) -> Option<&LayoutEntry> {
        self.layout().find(|e| e.id == id)
    }

    /// The private-data section of whichever processor the image is for.
    pub fn private_data(&self) -> Option<&LayoutEntry> {
        self.layout().find(|e| {
            matches!(
                e.id,
                SectionId::MetaPrivateData
                    | SectionId::MipsPrivateData
                    | SectionId::RiscvPrivateData
            )
        })
    }

    /// Bytes a region needs: the sum of its sections' alloc sizes.
    pub fn region_size(&self, kind: SectionKind) -> u32 {
        self.layout().filter(|e| e.kind == Some(kind)).map(|e| e.alloc_size).sum()
    }

    pub const fn device_info(&self) -> &DeviceInfoBlock<'a> {
        &self.dev_info
    }

    /// Walks the ELF's program headers; every `PT_LOAD` becomes a placement in the region of
    /// the layout section that holds its virtual range. Refuses a segment in no section.
    pub fn placements(&self) -> Result<Placements<'a>, FwImageError> {
        let b = self.bytes;
        if b.len() < 0x34 || &b[0..4] != b"\x7fELF" || b[4] != 1 || b[5] != 1 {
            return Err(FwImageError::Elf);
        }
        let phoff = u32_at(b, 0x1C).ok_or(FwImageError::Elf)? as usize;
        let phentsize = u16_at(b, 0x2A).ok_or(FwImageError::Elf)? as usize;
        let phnum = u16_at(b, 0x2C).ok_or(FwImageError::Elf)? as usize;
        if phentsize < 32 || phoff.checked_add(phentsize * phnum).is_none_or(|end| end > b.len()) {
            return Err(FwImageError::Elf);
        }
        Ok(Placements { image: *self, phoff, phentsize, phnum, next: 0 })
    }

    fn placement_of(&self, phdr: usize) -> Result<Option<Placement<'a>>, FwImageError> {
        let b = self.bytes;
        const PT_LOAD: u32 = 1;
        let p_type = u32_at(b, phdr).ok_or(FwImageError::Elf)?;
        if p_type != PT_LOAD {
            return Ok(None);
        }
        let p_offset = u32_at(b, phdr + 4).ok_or(FwImageError::Elf)? as usize;
        let p_vaddr = u32_at(b, phdr + 8).ok_or(FwImageError::Elf)?;
        let p_filesz = u32_at(b, phdr + 16).ok_or(FwImageError::Elf)?;
        let p_memsz = u32_at(b, phdr + 20).ok_or(FwImageError::Elf)?;
        if p_memsz == 0 {
            return Ok(None);
        }
        if p_filesz > p_memsz || p_offset.checked_add(p_filesz as usize).is_none_or(|e| e > b.len())
        {
            return Err(FwImageError::Elf);
        }
        let end = p_vaddr.checked_add(p_memsz).ok_or(FwImageError::Elf)?;
        let entry = self
            .layout()
            .find(|e| e.kind.is_some() && e.holds(p_vaddr, end))
            .ok_or(FwImageError::SegmentOutsideLayout { vaddr: p_vaddr, size: p_memsz })?;
        let Some(region) = entry.kind else {
            return Err(FwImageError::SegmentOutsideLayout { vaddr: p_vaddr, size: p_memsz });
        };
        Ok(Some(Placement {
            region,
            offset: entry.alloc_offset + (p_vaddr - entry.base_addr),
            bytes: &b[p_offset..p_offset + p_filesz as usize],
            zero_tail: p_memsz - p_filesz,
        }))
    }
}

/// The ELF's loadable segments as placements.
pub struct Placements<'a> {
    image: FirmwareImage<'a>,
    phoff: usize,
    phentsize: usize,
    phnum: usize,
    next: usize,
}

impl<'a> Iterator for Placements<'a> {
    type Item = Result<Placement<'a>, FwImageError>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next < self.phnum {
            let phdr = self.phoff + self.next * self.phentsize;
            self.next += 1;
            match self.image.placement_of(phdr) {
                Ok(Some(p)) => return Some(Ok(p)),
                Ok(None) => continue,
                Err(e) => return Some(Err(e)),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// A synthetic image: one ELF header with one PT_LOAD into the MIPS code section, a
    /// device-info block with the given masks, and the info header + one layout entry.
    fn image(
        core: CoreId,
        flags: u32,
        major: u16,
        entries: &[(u32, u32, u32, u32, u32, u32)],
    ) -> Vec<u8> {
        let mut b = vec![0u8; 2 * BLOCK];
        // ELF32 LE header: magic, class 1, data 1; e_phoff = 0x34, e_phentsize = 32, e_phnum = 1.
        b[0..4].copy_from_slice(b"\x7fELF");
        b[4] = 1;
        b[5] = 1;
        b[0x1C..0x20].copy_from_slice(&0x34u32.to_le_bytes());
        b[0x2A..0x2C].copy_from_slice(&32u16.to_le_bytes());
        b[0x2C..0x2E].copy_from_slice(&1u16.to_le_bytes());
        // PT_LOAD: offset 0x100, vaddr 0xC0000000, filesz 16, memsz 32.
        let ph = 0x34;
        b[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        b[ph + 4..ph + 8].copy_from_slice(&0x100u32.to_le_bytes());
        b[ph + 8..ph + 12].copy_from_slice(&0xC000_0000u32.to_le_bytes());
        b[ph + 16..ph + 20].copy_from_slice(&16u32.to_le_bytes());
        b[ph + 20..ph + 24].copy_from_slice(&32u32.to_le_bytes());
        for (i, x) in b[0x100..0x110].iter_mut().enumerate() {
            *x = i as u8 + 1;
        }
        // Device info: 1 word of each mask + 2 params, 32 + 40 = 72 bytes before the header.
        let h = BLOCK;
        let dis = 32 + 8 * 5;
        let d = h - dis;
        for (i, words) in [1u64, 1, 1, 2].iter().enumerate() {
            b[d + i * 8..d + i * 8 + 8].copy_from_slice(&words.to_le_bytes());
        }
        // BRN 63553 (bit 9); ERN 35421 (bit 0); features: MIPS (27), PHYS_BUS_WIDTH (36),
        // VIRTUAL_ADDRESS_SPACE_BITS (60) → params [36, 40].
        b[d + 32..d + 40].copy_from_slice(&(1u64 << 9).to_le_bytes());
        b[d + 40..d + 48].copy_from_slice(&1u64.to_le_bytes());
        b[d + 48..d + 56].copy_from_slice(&((1u64 << 27) | (1 << 36) | (1 << 60)).to_le_bytes());
        b[d + 56..d + 64].copy_from_slice(&36u64.to_le_bytes());
        b[d + 64..d + 72].copy_from_slice(&40u64.to_le_bytes());
        // Info header.
        b[h..h + 4].copy_from_slice(&INFO_VERSION.to_le_bytes());
        b[h + 4..h + 8].copy_from_slice(&HEADER_LEN.to_le_bytes());
        b[h + 8..h + 12].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        b[h + 12..h + 16].copy_from_slice(&LAYOUT_ENTRY_SIZE.to_le_bytes());
        b[h + 16..h + 24].copy_from_slice(&core.0.to_le_bytes());
        b[h + 24..h + 28].copy_from_slice(&4096u32.to_le_bytes());
        b[h + 28..h + 32].copy_from_slice(&flags.to_le_bytes());
        b[h + 32..h + 34].copy_from_slice(&major.to_le_bytes());
        b[h + 34..h + 36].copy_from_slice(&1u16.to_le_bytes());
        b[h + 36..h + 40].copy_from_slice(&6976702u32.to_le_bytes());
        b[h + 40..h + 44].copy_from_slice(&(dis as u32).to_le_bytes());
        for (i, e) in entries.iter().enumerate() {
            let at = h + 48 + i * 24;
            for (k, v) in [e.0, e.1, e.2, e.3, e.4, e.5].iter().enumerate() {
                b[at + k * 4..at + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        b
    }

    const MIPS_CODE: (u32, u32, u32, u32, u32, u32) = (4, 1, 0xC000_0000, 0x2F000, 0x17000, 0);

    #[test]
    fn a_good_image_parses_places_and_reports_its_features() {
        let b = image(CoreId::BOARD, FLAG_OPEN_SOURCE, 1, &[MIPS_CODE]);
        let img = FirmwareImage::parse(&b, CoreId::BOARD).unwrap();
        assert_eq!(img.header().version_build, 6976702);
        assert_eq!(img.region_size(SectionKind::Code), 0x17000);
        assert_eq!(img.region_size(SectionKind::Data), 0);
        let p: Vec<_> = img.placements().unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].region, SectionKind::Code);
        assert_eq!(p[0].offset, 0);
        assert_eq!(p[0].bytes, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(p[0].zero_tail, 16);
        let d = img.device_info();
        assert!(d.has(Feature::Mips));
        assert!(!d.has(Feature::RiscvFwProcessor));
        assert_eq!(d.value(Feature::PhysBusWidth), Some(36));
        assert_eq!(d.value(Feature::VirtualAddressSpaceBits), Some(40));
        assert_eq!(d.value(Feature::Mips), None, "a flag has no value");
        assert!(d.has_quirk(Quirk::Brn63553));
        assert!(!d.has_quirk(Quirk::Brn71242));
        assert!(d.has_enhancement(Enhancement::Ern35421));
        assert!(!d.unknown_feature_bits());
    }

    #[test]
    fn test_reject_firmware_wrong_id() {
        let other = CoreId::new(36, 52, 104, 182);
        let b = image(other, FLAG_OPEN_SOURCE, 1, &[MIPS_CODE]);
        assert_eq!(
            FirmwareImage::parse(&b, CoreId::BOARD).err(),
            Some(FwImageError::WrongId { expected: CoreId::BOARD, found: other })
        );
    }

    #[test]
    fn test_reject_firmware_not_open_build() {
        let b = image(CoreId::BOARD, 0, 1, &[MIPS_CODE]);
        assert_eq!(FirmwareImage::parse(&b, CoreId::BOARD).err(), Some(FwImageError::NotOpenBuild));
    }

    #[test]
    fn test_reject_firmware_bad_trailer() {
        let mut b = image(CoreId::BOARD, FLAG_OPEN_SOURCE, 1, &[MIPS_CODE]);
        b[BLOCK..BLOCK + 4].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            FirmwareImage::parse(&b, CoreId::BOARD).err(),
            Some(FwImageError::InfoVersion(2))
        );
        let b = image(CoreId::BOARD, FLAG_OPEN_SOURCE, 2, &[MIPS_CODE]);
        assert_eq!(
            FirmwareImage::parse(&b, CoreId::BOARD).err(),
            Some(FwImageError::Version { major: 2, minor: 1 })
        );
        let b = image(CoreId::BOARD, FLAG_OPEN_SOURCE, 1, &[(4, 1, 0xC000_0000, 0, 0, 0)]);
        assert_eq!(FirmwareImage::parse(&b, CoreId::BOARD).err(), Some(FwImageError::Layout));
        let short = vec![0u8; 100];
        assert_eq!(
            FirmwareImage::parse(&short, CoreId::BOARD).err(),
            Some(FwImageError::NotBlockAligned)
        );
    }

    #[test]
    fn test_reject_segment_outside_every_layout_section() {
        // The only section starts above the segment's address.
        let b =
            image(CoreId::BOARD, FLAG_OPEN_SOURCE, 1, &[(4, 1, 0xC010_0000, 0x1000, 0x1000, 0)]);
        let img = FirmwareImage::parse(&b, CoreId::BOARD).unwrap();
        let first = img.placements().unwrap().next().unwrap();
        assert_eq!(
            first.err(),
            Some(FwImageError::SegmentOutsideLayout { vaddr: 0xC000_0000, size: 32 })
        );
    }
}
