#![forbid(unsafe_code)]
#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]
//! CONTEXT: Userspace loader library providing ELF64/RISC-V load plan and mapping interface
//! OWNERS: @runtime
//! PUBLIC API: parse_elf64_riscv(), load_with(), Mapper, LoadPlan
//! DEPENDS_ON: goblin, thiserror, nexus-abi (os mapper only)
//! FEATURES: `os_mapper` only when `cfg(nexus_env = "os")`
//! INVARIANTS: No W+X segments; page-aligned segments; sorted non-overlapping vaddrs
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::vec::Vec;
use bitflags::bitflags;
use goblin::elf::{
    header::{self, header64::SIZEOF_EHDR, ELFCLASS64, ELFDATA2LSB, ELFMAG, EM_RISCV},
    program_header::PT_LOAD,
    Elf,
};
use thiserror::Error;

pub const PAGE_SIZE: u64 = 4096;

#[cfg(nexus_env = "os")]
pub mod os_mapper;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Prot: u32 {
        const R = 0x1;
        const W = 0x2;
        const X = 0x4;
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid ELF")]
    InvalidElf,
    #[error("unsupported feature")]
    Unsupported,
    #[error("segment has write and execute permissions")]
    ProtWx,
    #[error("segment alignment violation")]
    Align,
    #[error("segment goes out of bounds")]
    Oob,
    #[error("segment data truncated")]
    Truncated,
    #[error("internal loader error")]
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentPlan {
    pub vaddr: u64,
    pub memsz: u64,
    pub filesz: u64,
    pub off: u64,
    pub prot: Prot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadPlan {
    pub entry: u64,
    pub segments: Vec<SegmentPlan>,
}

pub trait Mapper {
    fn map_segment(&mut self, seg: &SegmentPlan, src: &[u8]) -> Result<(), Error>;
}

pub fn parse_elf64_riscv(bytes: &[u8]) -> Result<LoadPlan, Error> {
    if bytes.len() < SIZEOF_EHDR {
        return Err(Error::Truncated);
    }
    if &bytes[..ELFMAG.len()] != ELFMAG {
        return Err(Error::InvalidElf);
    }

    let elf = Elf::parse(bytes).map_err(map_goblin_error)?;

    if elf.header.e_ident[header::EI_CLASS] != ELFCLASS64
        || elf.header.e_ident[header::EI_DATA] != ELFDATA2LSB
    {
        return Err(Error::Unsupported);
    }

    if elf.header.e_machine != EM_RISCV {
        return Err(Error::Unsupported);
    }

    if elf.program_headers.is_empty() {
        return Err(Error::Unsupported);
    }

    let mut segments = Vec::new();
    for ph in &elf.program_headers {
        if ph.p_type == goblin::elf::program_header::PT_NULL {
            continue;
        }
        if ph.p_type != PT_LOAD {
            return Err(Error::Unsupported);
        }

        let prot = map_prot(ph.p_flags)?;
        if ph.p_align < PAGE_SIZE || ph.p_vaddr % PAGE_SIZE != 0 {
            return Err(Error::Align);
        }
        if ph.p_filesz > ph.p_memsz {
            return Err(Error::Oob);
        }
        let end = ph.p_offset.checked_add(ph.p_filesz).ok_or(Error::Internal)?;
        if end > bytes.len() as u64 {
            return Err(Error::Truncated);
        }

        segments.push(SegmentPlan {
            vaddr: ph.p_vaddr,
            memsz: ph.p_memsz,
            filesz: ph.p_filesz,
            off: ph.p_offset,
            prot,
        });
    }

    segments.sort_by_key(|s| s.vaddr);
    if segments.is_empty() {
        return Err(Error::Unsupported);
    }
    if segments.windows(2).any(|window| window[0].vaddr >= window[1].vaddr) {
        return Err(Error::Unsupported);
    }

    Ok(LoadPlan { entry: elf.header.e_entry, segments })
}

fn map_prot(flags: u32) -> Result<Prot, Error> {
    let mut prot = Prot::empty();
    if flags & goblin::elf::program_header::PF_R != 0 {
        prot |= Prot::R;
    }
    if flags & goblin::elf::program_header::PF_W != 0 {
        prot |= Prot::W;
    }
    if flags & goblin::elf::program_header::PF_X != 0 {
        prot |= Prot::X;
    }
    if prot.contains(Prot::W) && prot.contains(Prot::X) {
        return Err(Error::ProtWx);
    }
    Ok(prot)
}

pub fn load_with<M: Mapper>(bytes: &[u8], mapper: &mut M) -> Result<LoadPlan, Error> {
    let plan = parse_elf64_riscv(bytes)?;
    for seg in &plan.segments {
        let data = if seg.filesz == 0 {
            &[]
        } else {
            let start = seg.off as usize;
            let end = start.checked_add(seg.filesz as usize).ok_or(Error::Internal)?;
            bytes.get(start..end).ok_or(Error::Truncated)?
        };
        mapper.map_segment(seg, data)?;
    }
    Ok(plan)
}

fn map_goblin_error(err: goblin::error::Error) -> Error {
    match err {
        goblin::error::Error::Scroll(_) => Error::Truncated,
        goblin::error::Error::Malformed(_) | goblin::error::Error::BadMagic(_) => Error::InvalidElf,
        #[allow(unreachable_patterns)]
        _ => Error::InvalidElf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct VecMapper(Vec<(SegmentPlan, Vec<u8>)>);

    impl Mapper for VecMapper {
        fn map_segment(&mut self, seg: &SegmentPlan, src: &[u8]) -> Result<(), Error> {
            self.0.push((seg.clone(), src.to_vec()));
            Ok(())
        }
    }

    fn build_minimal_elf(flags: u32, second_segment: bool) -> Vec<u8> {
        use goblin::elf::header::{ELFCLASS64, ELFDATA2LSB};
        use goblin::elf::program_header::{PF_R, PF_W};

        let mut buf = Vec::new();
        let mut e_ident = [0u8; 16];
        e_ident[0] = 0x7f;
        e_ident[1] = b'E';
        e_ident[2] = b'L';
        e_ident[3] = b'F';
        e_ident[4] = ELFCLASS64;
        e_ident[5] = ELFDATA2LSB;
        e_ident[6] = 1;
        buf.extend_from_slice(&e_ident);
        buf.extend_from_slice(&[0u8; 48]);
        use byteorder::{LittleEndian, WriteBytesExt};
        let mut cursor = std::io::Cursor::new(buf);
        cursor.set_position(16);
        cursor.write_u16::<LittleEndian>(2).unwrap(); // e_type
        cursor.write_u16::<LittleEndian>(EM_RISCV).unwrap();
        cursor.write_u32::<LittleEndian>(1).unwrap();
        cursor.write_u64::<LittleEndian>(0x10000).unwrap();
        cursor.write_u64::<LittleEndian>(0x40).unwrap();
        cursor.write_u64::<LittleEndian>(0).unwrap();
        cursor.write_u32::<LittleEndian>(0).unwrap();
        cursor.write_u16::<LittleEndian>(64).unwrap();
        cursor.write_u16::<LittleEndian>(56).unwrap();
        cursor.write_u16::<LittleEndian>(if second_segment { 2 } else { 1 }).unwrap();
        cursor.write_u16::<LittleEndian>(0).unwrap();
        cursor.write_u16::<LittleEndian>(0).unwrap();
        cursor.write_u16::<LittleEndian>(0).unwrap();
        cursor.set_position(64);
        cursor.write_u32::<LittleEndian>(PT_LOAD).unwrap();
        cursor.write_u32::<LittleEndian>(flags).unwrap();
        cursor.write_u64::<LittleEndian>(0x100).unwrap();
        cursor.write_u64::<LittleEndian>(0x10000).unwrap();
        cursor.write_u64::<LittleEndian>(0x10000).unwrap();
        cursor.write_u64::<LittleEndian>(0x20).unwrap();
        cursor.write_u64::<LittleEndian>(0x20).unwrap();
        cursor.write_u64::<LittleEndian>(PAGE_SIZE).unwrap();
        if second_segment {
            cursor.write_u32::<LittleEndian>(PT_LOAD).unwrap();
            cursor.write_u32::<LittleEndian>(PF_R | PF_W).unwrap();
            cursor.write_u64::<LittleEndian>(0x200).unwrap();
            cursor.write_u64::<LittleEndian>(0x20000).unwrap();
            cursor.write_u64::<LittleEndian>(0x20000).unwrap();
            cursor.write_u64::<LittleEndian>(0x10).unwrap();
            cursor.write_u64::<LittleEndian>(0x20).unwrap();
            cursor.write_u64::<LittleEndian>(PAGE_SIZE).unwrap();
        }
        let mut data = cursor.into_inner();
        let pad_len = 0x100 - data.len();
        data.extend(std::iter::repeat(0).take(pad_len));
        data.extend_from_slice(&[0x13, 0x05, 0x00, 0x00]);
        data.resize(0x120, 0);
        if second_segment {
            let pad2 = 0x200 - data.len();
            data.extend(std::iter::repeat(0).take(pad2));
            data.extend_from_slice(&[1u8; 0x10]);
        }
        data
    }

    #[test]
    fn parses_minimal_exec() {
        let bytes = build_minimal_elf(
            goblin::elf::program_header::PF_R | goblin::elf::program_header::PF_X,
            true,
        );
        let plan = parse_elf64_riscv(&bytes).unwrap();
        assert_eq!(plan.entry, 0x10000);
        assert_eq!(plan.segments.len(), 2);
        assert!(plan.segments[0].prot.contains(Prot::R));
        assert!(plan.segments[0].prot.contains(Prot::X));
        assert!(!plan.segments[0].prot.contains(Prot::W));
        assert_eq!(plan.segments[0].vaddr, 0x10000);
        assert_eq!(plan.segments[1].vaddr, 0x20000);
    }

    #[test]
    fn rejects_wx() {
        let bytes = build_minimal_elf(
            goblin::elf::program_header::PF_R
                | goblin::elf::program_header::PF_W
                | goblin::elf::program_header::PF_X,
            false,
        );
        let err = parse_elf64_riscv(&bytes).unwrap_err();
        assert_eq!(err, Error::ProtWx);
    }

    #[test]
    fn load_invokes_mapper() {
        let bytes = build_minimal_elf(
            goblin::elf::program_header::PF_R | goblin::elf::program_header::PF_X,
            false,
        );
        let mut mapper = VecMapper(Vec::new());
        let plan = load_with(&bytes, &mut mapper).unwrap();
        assert_eq!(mapper.0.len(), plan.segments.len());
        assert_eq!(mapper.0[0].1.len(), plan.segments[0].filesz as usize);
    }

    #[test]
    fn rejects_wrong_machine() {
        let mut bytes = build_minimal_elf(goblin::elf::program_header::PF_R, false);
        bytes[18] = 0;
        bytes[19] = 0;
        assert_eq!(parse_elf64_riscv(&bytes).unwrap_err(), Error::Unsupported);
    }

    #[test]
    fn rejects_alignment_violations() {
        let mut bytes = build_minimal_elf(goblin::elf::program_header::PF_R, false);
        let offset = 64 + 16;
        bytes[offset] = 0x01;
        assert_eq!(parse_elf64_riscv(&bytes).unwrap_err(), Error::Align);
    }

    #[test]
    fn rejects_filesz_exceeding_memsz() {
        let mut bytes = build_minimal_elf(goblin::elf::program_header::PF_R, false);
        // Decrease p_memsz below p_filesz to violate the constraint filesz <= memsz.
        // For ELF64 program header, p_memsz is at offset 64 + 40.
        let p_memsz_off = 64 + 40;
        bytes[p_memsz_off..p_memsz_off + 8].fill(0);
        assert_eq!(parse_elf64_riscv(&bytes).unwrap_err(), Error::Oob);
    }

    #[test]
    fn rejects_truncated_file() {
        let bytes = build_minimal_elf(goblin::elf::program_header::PF_R, false);
        let truncated = bytes[..80].to_vec();
        assert_eq!(parse_elf64_riscv(&truncated).unwrap_err(), Error::Truncated);
    }
}
