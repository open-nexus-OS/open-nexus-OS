#![cfg(nexus_env = "os")]

extern crate alloc;

use crate::{Error, Mapper, Prot, SegmentPlan};
use nexus_abi::{self, AsHandle, Handle};

const PAGE_SIZE: u64 = 4096;
const MAP_FLAG_USER: u32 = 1 << 0;
const PROT_READ: u32 = 1 << 0;
const PROT_WRITE: u32 = 1 << 1;
const PROT_EXEC: u32 = 1 << 2;

/// Maps PT_LOAD segments into a child address space using kernel syscalls.
pub struct OsMapper {
    pub as_handle: AsHandle,
    pub bundle_vmo: Handle,
}

impl Mapper for OsMapper {
    fn map_segment(&mut self, seg: &SegmentPlan, src: &[u8]) -> Result<(), Error> {
        if seg.memsz == 0 {
            return Ok(());
        }
        let base = align_down(seg.vaddr, PAGE_SIZE)?;
        let end = align_up(
            seg.vaddr
                .checked_add(seg.memsz)
                .ok_or(Error::Oob)?,
            PAGE_SIZE,
        )?;
        let len = end.checked_sub(base).ok_or(Error::Oob)?;
        if len == 0 {
            return Err(Error::Align);
        }

        let offset_within_segment = seg
            .vaddr
            .checked_sub(base)
            .ok_or(Error::Oob)?;
        let offset_bytes = usize::try_from(offset_within_segment).map_err(|_| Error::Oob)?;
        let len_usize = usize::try_from(len).map_err(|_| Error::Oob)?;
        if offset_bytes > len_usize {
            return Err(Error::Oob);
        }

        let mut backing = alloc::vec![0u8; len_usize];
        let dst_range_end = offset_bytes
            .checked_add(src.len())
            .ok_or(Error::Oob)?;
        if dst_range_end > backing.len() {
            return Err(Error::Oob);
        }
        if !src.is_empty() {
            backing[offset_bytes..dst_range_end].copy_from_slice(src);
        }

        let segment_vmo = nexus_abi::vmo_create(backing.len())
            .map_err(|_| Error::Internal("vmo_create"))?;
        if !backing.is_empty() {
            nexus_abi::vmo_write(segment_vmo, 0, &backing)
                .map_err(|_| Error::Internal("vmo_write"))?;
        }

        let prot_bits = prot_to_bits(seg.prot)?;
        nexus_abi::as_map(self.as_handle, segment_vmo, base, len, prot_bits, MAP_FLAG_USER)
            .map_err(|_| Error::Internal("as_map"))
    }
}

/// Stack allocation helper for child address spaces.
pub struct StackBuilder {
    pub top_va: u64,
    pub size: u64,
    pub guard: u64,
}

impl StackBuilder {
    /// Creates a new builder with the provided layout.
    pub fn new(top_va: u64, size: u64, guard: u64) -> Result<Self, Error> {
        if size == 0 {
            return Err(Error::Internal("stack size"));
        }
        if size % PAGE_SIZE != 0 || guard % PAGE_SIZE != 0 {
            return Err(Error::Align);
        }
        if top_va % PAGE_SIZE != 0 {
            return Err(Error::Align);
        }
        Ok(Self { top_va, size, guard })
    }

    fn stack_base(&self) -> Result<u64, Error> {
        self.top_va.checked_sub(self.size).ok_or(Error::Oob)
    }

    /// Creates a zeroed stack VMO, writes argv/env tables, maps it, and returns the stack layout.
    pub fn build(
        &self,
        mapper: &mut OsMapper,
        argv: &[&str],
        env: &[&str],
    ) -> Result<StackImage, Error> {
        let base = self.stack_base()?;
        let guard_base = base.checked_sub(self.guard).ok_or(Error::Oob)?;
        let len = self.size;
        let handle = nexus_abi::vmo_create(len as usize).map_err(|_| Error::Internal("vmo_create"))?;

        let mut image = vec![0u8; len as usize];
        let layout = layout_arguments(&mut image, base, argv, env)?;
        nexus_abi::vmo_write(handle, 0, &image).map_err(|_| Error::Internal("vmo_write"))?;

        let prot = Prot::R | Prot::W;
        let prot_bits = prot_to_bits(prot)?;
        nexus_abi::as_map(mapper.as_handle, handle, base, len, prot_bits, MAP_FLAG_USER)
            .map_err(|_| Error::Internal("as_map"))?;

        Ok(StackImage { handle, base, guard_base, len, sp: layout.sp, argv_ptr: layout.argv, env_ptr: layout.env })
    }
}

/// Result of provisioning the child's stack.
pub struct StackImage {
    pub handle: Handle,
    pub base: u64,
    pub guard_base: u64,
    pub len: u64,
    pub sp: u64,
    pub argv_ptr: u64,
    pub env_ptr: u64,
}

struct ArgLayout {
    sp: u64,
    argv: u64,
    env: u64,
}

fn layout_arguments(image: &mut [u8], base: u64, argv: &[&str], env: &[&str]) -> Result<ArgLayout, Error> {
    let mut cursor = image.len();
    let mut argv_ptrs = Vec::with_capacity(argv.len());
    let mut env_ptrs = Vec::with_capacity(env.len());

    for value in argv.iter().rev() {
        let ptr = push_string(image, &mut cursor, base, value)?;
        argv_ptrs.push(ptr);
    }
    argv_ptrs.reverse();
    for value in env.iter().rev() {
        let ptr = push_string(image, &mut cursor, base, value)?;
        env_ptrs.push(ptr);
    }
    env_ptrs.reverse();

    cursor = align_down_usize(cursor, 8)?;
    let env_block = push_pointer_block(image, &mut cursor, base, &env_ptrs)?;
    let argv_block = push_pointer_block(image, &mut cursor, base, &argv_ptrs)?;

    cursor = align_down_usize(cursor, 16)?;
    let header_size = 3 * core::mem::size_of::<u64>();
    if cursor < header_size {
        return Err(Error::Internal("stack overflow"));
    }
    cursor -= header_size;
    let argc = argv.len() as u64;
    write_u64(image, cursor, argc);
    write_u64(image, cursor + 8, argv_block);
    write_u64(image, cursor + 16, env_block);

    let sp = base + cursor as u64;
    debug_assert_eq!(sp & 0xf, 0);
    Ok(ArgLayout { sp, argv: argv_block, env: env_block })
}

fn push_string(
    image: &mut [u8],
    cursor: &mut usize,
    base: u64,
    value: &str,
) -> Result<u64, Error> {
    let bytes = value.as_bytes();
    let needed = bytes.len() + 1; // include NUL
    if *cursor < needed {
        return Err(Error::Internal("stack overflow"));
    }
    *cursor -= needed;
    let start = *cursor;
    image[start..start + bytes.len()].copy_from_slice(bytes);
    // trailing byte already zeroed
    Ok(base + start as u64)
}

fn push_pointer_block(
    image: &mut [u8],
    cursor: &mut usize,
    base: u64,
    values: &[u64],
) -> Result<u64, Error> {
    let width = core::mem::size_of::<u64>();
    let needed = (values.len() + 1) * width;
    if *cursor < needed {
        return Err(Error::Internal("stack overflow"));
    }
    *cursor -= needed;
    let start = *cursor;
    for (index, value) in values.iter().enumerate() {
        write_u64(image, start + index * width, *value);
    }
    write_u64(image, start + values.len() * width, 0);
    Ok(base + start as u64)
}

fn write_u64(buf: &mut [u8], offset: usize, value: u64) {
    buf[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn align_down(value: u64, align: u64) -> Result<u64, Error> {
    if align == 0 {
        return Err(Error::Align);
    }
    Ok(value & !(align - 1))
}

fn align_up(value: u64, align: u64) -> Result<u64, Error> {
    if align == 0 {
        return Err(Error::Align);
    }
    if value == 0 {
        return Ok(0);
    }
    let sum = value.checked_add(align - 1).ok_or(Error::Oob)?;
    Ok((sum / align) * align)
}

fn align_down_usize(value: usize, align: usize) -> Result<usize, Error> {
    if align == 0 {
        return Err(Error::Align);
    }
    Ok(value & !(align - 1))
}

fn prot_to_bits(prot: Prot) -> Result<u32, Error> {
    if prot.contains(Prot::W) && prot.contains(Prot::X) {
        return Err(Error::ProtWx);
    }
    let mut bits = 0;
    if prot.contains(Prot::R) {
        bits |= PROT_READ;
    }
    if prot.contains(Prot::W) {
        bits |= PROT_WRITE;
    }
    if prot.contains(Prot::X) {
        bits |= PROT_EXEC;
    }
    Ok(bits)
}
