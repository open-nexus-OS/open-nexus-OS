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
