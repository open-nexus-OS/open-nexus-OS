// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A context's GPU address space on the device (TASK-0215 5c-3 P2b, RFC-0101 §1): a page
//! catalogue with the directories and tables under it, and the firmware memory context naming
//! the catalogue — the firmware picks the catalogue register set (`page_cat_base_reg_set` left
//! invalid, as the open kernel driver leaves it), so contexts switch spaces in the firmware.
//! Made per context; destroyed once the firmware let go of every common context on it: the
//! tables' pages back to the pool, the memory context unmapped and freed, one waited flush —
//! the slot storage kept for the next space (gpud's heap never frees).

use nexus_gfx::backend::gpu::fwif::FwMemContext;
use nexus_gfx::backend::gpu::mmu::{PageTables, TableStorage};

use super::device::{Gpu, Tables};
use super::memory::HeapObj;

pub(super) struct Space {
    pub tables: PageTables,
    pub fw_mem_ctx: HeapObj,
}

impl Space {
    /// A new space over `storage`; the storage comes back with a refusal.
    pub(super) fn create(
        gpu: &mut Gpu,
        storage: TableStorage,
    ) -> Result<Self, (&'static str, TableStorage)> {
        let tables = PageTables::new_in(storage, &mut Tables(&mut gpu.mem))
            .map_err(|(_, s)| ("no catalogue page", s))?;
        let Some(ctx) = gpu.mem.alloc(FwMemContext::SIZE as u32) else {
            let storage = tables.destroy(&mut Tables(&mut gpu.mem));
            return Err(("no memory for the firmware memory context", storage));
        };
        gpu.fw.map_object(&mut gpu.mem, &ctx);
        gpu.mem.write(&ctx, 0, &FwMemContext::new(tables.root_phys()).to_bytes());
        Ok(Self { tables, fw_mem_ctx: ctx })
    }

    /// The catalogue's physical address — what the firmware names a context's fault by.
    pub(super) fn root_phys(&self) -> u64 {
        self.tables.root_phys()
    }

    /// Gives the space back (no common context on it is left in the firmware): its table pages
    /// and its memory context, the firmware's MMU cache flushed and waited before anything is
    /// allocated again (nothing allocates between the two); the slot storage comes back. `false`
    /// when the flush got no return.
    pub(super) fn destroy(self, gpu: &mut Gpu) -> (TableStorage, bool) {
        let storage = self.tables.destroy(&mut Tables(&mut gpu.mem));
        let flushed = gpu.release(&[self.fw_mem_ctx]);
        (storage, flushed)
    }
}
