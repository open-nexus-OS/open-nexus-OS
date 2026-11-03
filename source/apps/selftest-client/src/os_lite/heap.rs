// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the runtime heap proof (TASK-0352 P8, ADR-0093). Every service's heap is the one
//! 64 MiB region of its image the kernel maps as a lazy object (ADR-0077): reserving it costs
//! nothing, a page costs the pool once it is touched. This client declared no heap size and
//! allocates 32 MiB — more than any service's old ceiling — touches every page and gives it back;
//! the pool's free count says what it cost: the pages touched, no more.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — mmio phase, `SELFTEST: heap 32m ok (`.

extern crate alloc;

/// What the allocation proved.
pub(crate) struct HeapProof {
    /// Bytes allocated and touched.
    pub bytes: u64,
    /// Pool frames the touch committed (the pool's free count before minus during).
    pub committed_pages: u64,
}

/// 32 MiB from the heap, every page touched: the frames it cost are the pages touched, within a
/// tenth (the allocator's own words and the frames other services take meanwhile).
pub(crate) fn heap_32m_probe() -> core::result::Result<HeapProof, ()> {
    const BYTES: usize = 32 * 1024 * 1024;
    const PAGE: usize = 4096;
    let before = nexus_abi::mm_stats().map_err(|_| ())?;
    let mut block: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    block.try_reserve_exact(BYTES).map_err(|_| ())?;
    block.resize(BYTES, 0);
    for page in (0..BYTES).step_by(PAGE) {
        // SAFETY: `page` is inside the vector's `BYTES` initialised bytes.
        unsafe { core::ptr::write_volatile(block.as_mut_ptr().add(page), 0xA5) };
    }
    let during = nexus_abi::mm_stats().map_err(|_| ())?;
    drop(block);
    let committed_pages = before.free.saturating_sub(during.free);
    let pages = (BYTES / PAGE) as u64;
    if committed_pages < pages * 9 / 10 || committed_pages > pages * 11 / 10 {
        return Err(());
    }
    Ok(HeapProof { bytes: BYTES as u64, committed_pages })
}
