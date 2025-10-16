#![forbid(unsafe_code)]

use core::cmp::Ordering;

use bitflags::bitflags;
use goblin::elf::{
    header::{self, Header},
    program_header::{ProgramHeader, PT_LOAD, PT_NULL},
    Elf,
};
use thiserror::Error;

#[cfg(nexus_env = "os")]
pub mod os_mapper;

const PAGE_SIZE: u64 = 4096;

bitflags! {
    /// Memory protection bits for a mapped segment.
    pub struct Prot: u32 {
        const R = 0x1;
        const W = 0x2;
        const X = 0x4;
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid ELF: {0}")]
    InvalidElf(&'static str),
    #[error("unsupported feature: {0}")]
    Unsupported(&'static str),
    #[error("w^x violation for segment")]
    ProtWx,
    #[error("segment alignment error")]
    Align,
    #[error("value out of bounds")]
    Oob,
    #[error("ELF truncated")]
    Truncated,
    #[error("internal error: {0}")]
    Internal(&'static str),
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
    if bytes.len() < Header::size() {
        return Err(Error::Truncated);
    }
    if &bytes[..header::SELFMAG.len()] != header::ELFMAG {
        return Err(Error::InvalidElf("bad magic"));
    }

    let elf = Elf::parse(bytes).map_err(|_| Error::InvalidElf("parse error"))?;

    if elf.header.e_ident[header::EI_CLASS] != header::ELFCLASS64 {
        return Err(Error::Unsupported("not ELF64"));
    }
    if elf.header.e_ident[header::EI_DATA] != header::ELFDATA2LSB {
        return Err(Error::Unsupported("not little endian"));
    }
    if elf.header.e_machine != header::EM_RISCV {
        return Err(Error::Unsupported("unexpected machine"));
    }

    let mut segments = Vec::new();
    for ph in &elf.program_headers {
        match ph.p_type {
            PT_LOAD => segments.push(segment_from_program(ph, bytes)?),
            PT_NULL => {}
            _ => return Err(Error::Unsupported("non-PT_LOAD program headers")),
        }
    }

    if segments.is_empty() {
        return Err(Error::Unsupported("no PT_LOAD segments"));
    }

    segments.sort_by(|a, b| match a.vaddr.cmp(&b.vaddr) {
        Ordering::Equal => a.off.cmp(&b.off),
        other => other,
    });

    Ok(LoadPlan {
        entry: elf.header.e_entry,
        segments,
    })
}

fn segment_from_program(ph: &ProgramHeader, bytes: &[u8]) -> Result<SegmentPlan, Error> {
    if ph.p_align < PAGE_SIZE {
        return Err(Error::Align);
    }
    if ph.p_vaddr & (PAGE_SIZE - 1) != 0 {
        return Err(Error::Align);
    }
    if ph.p_filesz > ph.p_memsz {
        return Err(Error::InvalidElf("filesz larger than memsz"));
    }

    let prot = prot_from_flags(ph.p_flags)?;

    let end = ph
        .p_offset
        .checked_add(ph.p_filesz)
        .ok_or(Error::Oob)?;
    if end > bytes.len() as u64 {
        return Err(Error::Truncated);
    }

    Ok(SegmentPlan {
        vaddr: ph.p_vaddr,
        memsz: ph.p_memsz,
        filesz: ph.p_filesz,
        off: ph.p_offset,
        prot,
    })
}

fn prot_from_flags(flags: u32) -> Result<Prot, Error> {
    let mut prot = Prot::empty();
    if flags & header::PF_R != 0 {
        prot |= Prot::R;
    }
    if flags & header::PF_W != 0 {
        prot |= Prot::W;
    }
    if flags & header::PF_X != 0 {
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
        let off = usize::try_from(seg.off).map_err(|_| Error::Oob)?;
        let filesz = usize::try_from(seg.filesz).map_err(|_| Error::Oob)?;
        let end = off
            .checked_add(filesz)
            .ok_or(Error::Oob)?;
        if end > bytes.len() {
            return Err(Error::Truncated);
        }
        let src = &bytes[off..end];
        mapper.map_segment(seg, src)?;
    }

    Ok(plan)
}
