// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A buffer for the GPU alone (RFC-0101 §3 `GpuPrivate`, TASK-0215 5c-3 P2a): a VMO made for the
//! device, its pages named by `vmo_runs` a chunk at a time, never mapped for the CPU — so no
//! region of gpud's address space, which holds 64 (every CPU-mapped buffer takes one: the render
//! bundle alone took 36). The free lists' parameter pages, the partial render's scratch, the
//! dataset's memory are such buffers: the CPU never reads or writes them. The kernel zeroes the
//! object at create; drop destroys it.

use nexus_abi::{Cap, DmaRun, Handle};

const PAGE: usize = 4096;
/// Runs one query holds; a chunk of as many pages never needs more.
const RUNS: usize = 16;
const CHUNK_MIN: usize = RUNS * PAGE;
/// The first chunk a walk asks for; halved while the kernel answers that it needs more runs.
const CHUNK_MAX: usize = 2 << 20;

pub(super) struct GpuVmo {
    handle: Handle,
    len: usize,
}

impl GpuVmo {
    /// `len` bytes (page-rounded) for the device behind `device`.
    pub(super) fn new(device: Cap, len: usize) -> Option<Self> {
        let len = len.checked_next_multiple_of(PAGE)?;
        if len == 0 {
            return None;
        }
        let handle = nexus_abi::vmo_create_for(device, len).ok()?;
        Some(Self { handle, len })
    }

    pub(super) const fn len(&self) -> usize {
        self.len
    }

    /// Calls `page` with the bus address of every page of `offset..offset + len`, in order —
    /// walked, nothing collected (gpud's heap never frees); `false` when the kernel refused a
    /// query or `page` stopped the walk.
    pub(super) fn walk(
        &self,
        device: Cap,
        offset: usize,
        len: usize,
        mut page: impl FnMut(u64) -> bool,
    ) -> bool {
        let Some(end) = offset.checked_add(len) else { return false };
        if offset % PAGE != 0 || len % PAGE != 0 || end > self.len {
            return false;
        }
        let (mut at, mut chunk) = (offset, CHUNK_MAX);
        while at < end {
            let take = chunk.min(end - at);
            let mut runs = [DmaRun::default(); RUNS];
            match nexus_abi::vmo_runs(self.handle, device, at, take, &mut runs) {
                Ok(n) => {
                    for run in &runs[..n] {
                        for i in 0..run.len / PAGE as u64 {
                            if !page(run.bus + i * PAGE as u64) {
                                return false;
                            }
                        }
                    }
                    at += take;
                }
                // More runs than one query holds: a smaller chunk.
                Err(_) if chunk > CHUNK_MIN => chunk /= 2,
                Err(_) => return false,
            }
        }
        true
    }
}

impl Drop for GpuVmo {
    fn drop(&mut self) {
        let _ = nexus_abi::vmo_destroy(self.handle);
    }
}
