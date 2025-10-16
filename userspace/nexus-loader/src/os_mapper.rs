#[cfg(not(nexus_env = "os"))]
compile_error!("os_mapper is only available when building for nexus_env=\"os\"");

use alloc::vec::{self, Vec};
use core::convert::TryFrom;

use crate::{Error, Mapper, Prot, SegmentPlan, PAGE_SIZE};
use nexus_abi::{self, AsHandle};

const MAP_FLAG_USER: u32 = 1 << 0;

pub struct OsMapper {
    as_handle: AsHandle,
    _bundle_vmo: nexus_abi::Handle,
    segment_vmos: Vec<nexus_abi::Handle>,
}

impl OsMapper {
    pub fn new(as_handle: AsHandle, bundle_vmo: nexus_abi::Handle) -> Self {
        Self { as_handle, _bundle_vmo: bundle_vmo, segment_vmos: Vec::new() }
    }
}

impl Mapper for OsMapper {
    fn map_segment(&mut self, seg: &SegmentPlan, src: &[u8]) -> Result<(), Error> {
        if seg.prot.contains(Prot::W) && seg.prot.contains(Prot::X) {
            return Err(Error::ProtWx);
        }

        let page_mask = PAGE_SIZE - 1;
        let map_base = seg.vaddr & !page_mask;
        let offset = seg.vaddr - map_base;
        let total = seg
            .memsz
            .checked_add(offset)
            .ok_or(Error::Internal)?;
        let map_len = align_up(total, PAGE_SIZE).ok_or(Error::Internal)?;
        if map_len == 0 {
            return Err(Error::Internal);
        }

        let map_len_usize = usize::try_from(map_len).map_err(|_| Error::Internal)?;
        let offset_usize = usize::try_from(offset).map_err(|_| Error::Internal)?;
        if offset_usize > map_len_usize {
            return Err(Error::Internal);
        }
        if offset_usize
            .checked_add(src.len())
            .map_or(true, |end| end > map_len_usize)
        {
            return Err(Error::Truncated);
        }

        let seg_vmo = nexus_abi::vmo_create(map_len_usize).map_err(|_| Error::Internal)?;
        let mut image = vec![0u8; map_len_usize];
        if !src.is_empty() {
            image[offset_usize..offset_usize + src.len()].copy_from_slice(src);
        }
        nexus_abi::vmo_write(seg_vmo, 0, &image).map_err(|_| Error::Internal)?;

        let prot = seg.prot.bits();
        nexus_abi::as_map(
            self.as_handle,
            seg_vmo,
            map_base,
            map_len,
            prot,
            MAP_FLAG_USER,
        )
        .map_err(|_| Error::Internal)?;

        self.segment_vmos.push(seg_vmo);
        Ok(())
    }
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    if align == 0 {
        return None;
    }
    let minus_one = align - 1;
    value
        .checked_add(minus_one)
        .map(|v| v & !minus_one)
}

pub struct StackBuilder {
    top_va: u64,
    size: u64,
    guard: u64,
}

impl StackBuilder {
    pub fn new(top_va: u64, stack_pages: u64) -> Result<Self, Error> {
        let size = stack_pages
            .checked_mul(PAGE_SIZE)
            .ok_or(Error::Internal)?;
        if size > usize::MAX as u64 {
            return Err(Error::Internal);
        }
        let guard = PAGE_SIZE;
        if top_va < size + guard {
            return Err(Error::Align);
        }
        if top_va % PAGE_SIZE != 0 {
            return Err(Error::Align);
        }
        Ok(Self { top_va, size, guard })
    }

    pub fn stack_base(&self) -> u64 {
        self.top_va - self.size
    }

    pub fn guard_base(&self) -> u64 {
        self.top_va - self.size - self.guard
    }

    pub fn map_stack(&self, as_handle: AsHandle) -> Result<nexus_abi::Handle, Error> {
        let stack_vmo = nexus_abi::vmo_create(self.size as usize).map_err(|_| Error::Internal)?;
        let base = self.stack_base();
        nexus_abi::as_map(
            as_handle,
            stack_vmo,
            base,
            self.size,
            (Prot::R | Prot::W).bits(),
            MAP_FLAG_USER,
        )
        .map_err(|_| Error::Internal)?;
        Ok(stack_vmo)
    }

    pub fn populate(
        &self,
        stack_vmo: nexus_abi::Handle,
        argv: &[&str],
        env: &[&str],
    ) -> Result<(u64, u64, u64), Error> {
        let base = self.stack_base();
        let mut image = vec![0u8; self.size as usize];
        let mut cursor = image.len();

        let mut env_addrs = Vec::new();
        for entry in env.iter().rev() {
            let addr = push_string(&mut image, &mut cursor, base, entry)?;
            env_addrs.push(addr);
        }
        env_addrs.reverse();

        let mut argv_addrs = Vec::new();
        for entry in argv.iter().rev() {
            let addr = push_string(&mut image, &mut cursor, base, entry)?;
            argv_addrs.push(addr);
        }
        argv_addrs.reverse();

        cursor &= !0xf;

        let env_ptr = push_pointer_array(&mut image, &mut cursor, base, &env_addrs)?;
        let argv_ptr = push_pointer_array(&mut image, &mut cursor, base, &argv_addrs)?;

        cursor &= !0xf;
        let sp = (base + cursor as u64) & !0xf;

        nexus_abi::vmo_write(stack_vmo, 0, &image).map_err(|_| Error::Internal)?;
        Ok((sp, argv_ptr, env_ptr))
    }
}

fn push_string(
    image: &mut [u8],
    cursor: &mut usize,
    base: u64,
    value: &str,
) -> Result<u64, Error> {
    let bytes = value.as_bytes();
    let len = bytes.len().checked_add(1).ok_or(Error::Internal)?;
    if *cursor < len {
        return Err(Error::Oob);
    }
    *cursor -= len;
    let start = *cursor;
    image[start..start + bytes.len()].copy_from_slice(bytes);
    image[start + bytes.len()] = 0;
    Ok(base + start as u64)
}

fn push_pointer_array(
    image: &mut [u8],
    cursor: &mut usize,
    base: u64,
    values: &[u64],
) -> Result<u64, Error> {
    *cursor &= !0x7;
    let mut entries: Vec<u64> = values.to_vec();
    entries.push(0);
    for value in entries.iter().rev() {
        if *cursor < core::mem::size_of::<u64>() {
            return Err(Error::Oob);
        }
        *cursor -= core::mem::size_of::<u64>();
        let start = *cursor;
        image[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(base + *cursor as u64)
}
