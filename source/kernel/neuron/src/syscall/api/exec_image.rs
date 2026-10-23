// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the frame side of `exec` (TASK-0286 P3a): a process image is
//! pool blocks, largest first — allocated and recorded on the task's
//! `ImageAllocs`, zeroed and filled by the staged copy plan (phase B, BKL
//! dropped), mapped back to back into the child. Split out of `exec.rs`
//! (module-size ratchet).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every service image on the QEMU ladder; `image_allocs` host tests

use super::exec::{CopyOp, CopyPlan};
use super::*;
use core::ptr;

/// `len` bytes of image memory as pool blocks (largest first), recorded on
/// `image` and planned for zeroing in phase B. Exhaustion is the pool's event
/// line plus the same errno the arena returned.
pub(super) fn alloc_image(
    len: usize,
    image: &mut crate::task::ImageAllocs,
    plan: &mut CopyPlan,
) -> SysResult<alloc::vec::Vec<crate::frames::Block>> {
    let blocks = crate::mm::frame_pool::alloc_bytes(len)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    for block in &blocks {
        image.push(*block);
        plan.push(CopyOp {
            src: usize::MAX,
            dst: block.base as usize,
            len: block.size() as usize,
        })?;
    }
    Ok(blocks)
}

/// Plan the copy of `src_len` bytes at `src` to image offset `dst_off`,
/// split wherever a block boundary falls inside it.
pub(super) fn plan_payload(
    blocks: &[crate::frames::Block],
    dst_off: usize,
    src: usize,
    src_len: usize,
    plan: &mut CopyPlan,
) -> SysResult<()> {
    let end = dst_off.checked_add(src_len).ok_or(AddressSpaceError::InvalidArgs)?;
    let mut at = 0usize;
    for block in blocks {
        let block_end = at + block.size() as usize;
        let (lo, hi) = (dst_off.max(at), end.min(block_end));
        if lo < hi {
            plan.push(CopyOp {
                src: src + (lo - dst_off),
                dst: block.base as usize + (lo - at),
                len: hi - lo,
            })?;
        }
        at = block_end;
    }
    Ok(())
}

/// Map the blocks back to back from `va`, page by page (tracked as Fixed).
pub(super) fn map_blocks(
    address_spaces: &mut crate::mm::AddressSpaceManager,
    as_handle: crate::mm::AsHandle,
    va: usize,
    blocks: &[crate::frames::Block],
    flags: PageFlags,
) -> SysResult<()> {
    let mut cursor = va;
    for block in blocks {
        for page in 0..block.frames() {
            let pa = block.base as usize + page * PAGE_SIZE;
            address_spaces.map_page_tracked(as_handle, cursor, pa, flags)?;
            cursor = cursor.checked_add(PAGE_SIZE).ok_or(AddressSpaceError::InvalidArgs)?;
        }
    }
    Ok(())
}

/// One PT_LOAD segment as exec found it: `len` bytes from the page-aligned `va`, the first byte
/// `page_off` into its page, the file `payload` `(src, filesz)` to lay there, the PTE `flags`.
pub(super) struct Segment {
    pub va: usize,
    pub page_off: usize,
    pub len: usize,
    pub payload: (usize, usize),
    pub flags: PageFlags,
}

/// Load one segment (ADR-0077): the pages that hold file bytes are pool blocks — zeroed and
/// filled by the copy plan in phase B, mapped now; a writable segment's pure `.bss` tail beyond
/// them is a lazy object the child owns, committed page by page on first touch. A read-only
/// segment is eager whole.
pub(super) fn load_segment(
    address_spaces: &mut crate::mm::AddressSpaceManager,
    as_handle: crate::mm::AsHandle,
    image: &mut crate::task::ImageAllocs,
    plan: &mut CopyPlan,
    seg: Segment,
) -> SysResult<()> {
    let (src, filesz) = seg.payload;
    let file_pages = if filesz == 0 {
        0
    } else {
        align_len(seg.page_off + filesz).ok_or(AddressSpaceError::InvalidArgs)?
    };
    let eager = if seg.flags.contains(PageFlags::WRITE) { file_pages } else { seg.len };
    if eager != 0 {
        let blocks = alloc_image(eager, image, plan)?;
        if filesz != 0 {
            plan_payload(&blocks, seg.page_off, src, filesz, plan)?;
            plan.fence_i = true;
        }
        map_blocks(address_spaces, as_handle, seg.va, &blocks, seg.flags)?;
    }
    if eager < seg.len {
        map_lazy_fixed(address_spaces, as_handle, seg.va + eager, seg.len - eager, seg.flags)?;
    }
    Ok(())
}

/// `len` bytes of kernel-placed memory at `va` (a `.bss` tail, a stack) backed by a new lazy
/// object the child owns: recorded, no frame taken (ADR-0077). Nothing names the object but the
/// child's region; it dies with the space at reap (RFC-0100 §1).
pub(super) fn map_lazy_fixed(
    address_spaces: &mut crate::mm::AddressSpaceManager,
    as_handle: crate::mm::AsHandle,
    va: usize,
    len: usize,
    flags: PageFlags,
) -> SysResult<()> {
    let reach = crate::dma_reach::DmaReach::ALL;
    let vmo = crate::mm::vmo::create(len, crate::mm::vmo::VmoKind::Lazy, &reach)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    let space = address_spaces.get_mut(as_handle)?;
    if let Err(err) = space.va_space_mut().record_lazy(va, len, flags.bits(), vmo) {
        let _ = crate::mm::vmo::destroy(vmo);
        return Err(Error::Va(err));
    }
    Ok(())
}

/// One zeroed page for the bootstrap meta/info records, recorded on `image`.
pub(super) fn alloc_zeroed_page(image: &mut crate::task::ImageAllocs) -> SysResult<usize> {
    let block = crate::mm::frame_pool::alloc(0)
        .map_err(|_| Error::Capability(CapError::PermissionDenied))?;
    image.push(block);
    let pa = block.base as usize;
    // SAFETY: a frame nobody else owns, through the direct map.
    unsafe { ptr::write_bytes(crate::phys::phys_to_virt(pa) as *mut u8, 0, PAGE_SIZE) };
    Ok(pa)
}
