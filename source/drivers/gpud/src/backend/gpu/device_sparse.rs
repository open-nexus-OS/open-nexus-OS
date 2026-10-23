// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The surface object on the device (ADR-0078, TASK-0333 6b-2b): the display's lazy object
//! adopted as a buffer of windowd's, held against the GPU's ceiling whole, recorded in the
//! context's address space WITHOUT a page entered — binding it whole would name, and so commit,
//! every page — and entered a range at a time as frames first sample it (`vm_enter`): the kernel
//! commits what it names, the tables get the pages, the MMU's caches are flushed once a range. A
//! page never entered stays invalid: a draw that read it would be a named GPU fault, never black
//! texels (`crate::surface_pages` proves the pages entered hold every texel a frame reads).

use nexus_driverkit::gpu::{Binding, GpuVa, VmError};
use nexus_gfx::backend::gpu::mmu::PAGE;
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, DeviceError, Mapping};

use super::bos::{Backing, Bo};
use super::device::Tables;
use super::device_v2::Device;

impl Device {
    /// The display's lazy surface object `handle` as `client`'s buffer of `len` bytes — held
    /// against the ceiling whole: all of it may be drawn.
    pub(super) fn adopt_lazy(
        &mut self,
        client: ClientId,
        handle: u32,
        len: usize,
    ) -> Result<BoId, DeviceError> {
        if len == 0 || len % PAGE as usize != 0 {
            return Err(DeviceError::Vm(VmError::Unaligned));
        }
        self.hold(len)?;
        let made = self.bos.insert(Bo {
            client,
            len,
            backing: Backing::Lazy { handle },
            bindings: 0,
            device_cached: false,
            pm_fw_protect: false,
        });
        if made.is_err() {
            self.ledger.give_back(len as u64);
        }
        made
    }

    /// Records `m` in the context's space with no page entered.
    pub(super) fn vm_bind_sparse(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        m: Mapping,
    ) -> Result<(), DeviceError> {
        let slot = self.contexts.running(client, ctx)?;
        let b = self.bos.get(client, m.bo)?;
        let end = m.offset.checked_add(m.len).ok_or(VmError::OutOfRange)?;
        if end > b.len as u64 {
            return Err(DeviceError::Vm(VmError::OutOfRange));
        }
        let binding =
            Binding { va: m.va, len: m.len, bo: m.bo.0, offset: m.offset, access: m.access };
        slot.space.bind(binding)?;
        if let Some(b) = self.bos.any_mut(m.bo) {
            b.bindings += 1;
        }
        Ok(())
    }

    /// Enters `len` bytes from `va`, inside one binding, with the buffer's pages there: named (a
    /// lazy buffer's committed as they are), written into the tables, the MMU's caches flushed
    /// once. The caller enters only what it has not (`crate::surface_pages::Entered`); a refused
    /// range leaves none of its pages entered.
    pub(super) fn vm_enter(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        va: GpuVa,
        len: u64,
    ) -> Result<(), DeviceError> {
        let slot = self.contexts.running(client, ctx)?;
        let binding = *slot.space.resolve(va, len)?;
        let b = self.bos.get(client, BoId(binding.bo))?;
        let flags = b.page_flags(binding.access);
        let gpu = &mut *self.gpu;
        let Some(mmu) = slot.mmu.as_mut() else {
            return Err(DeviceError::Lost);
        };
        let mut mapped = 0u64;
        let walked = b.walk(binding.offset_of(va), len, |phys| {
            let at = va.0 + mapped * PAGE;
            if mmu.tables.map(&mut Tables(&mut gpu.mem), at, phys, flags).is_err() {
                return false;
            }
            mapped += 1;
            true
        });
        if !walked {
            for i in 0..mapped {
                let _ = mmu.tables.unmap(&mut Tables(&mut gpu.mem), va.0 + i * PAGE);
            }
        }
        let flush = mmu.tables.take_flush_flags();
        match (walked, gpu.mmu_flush(flush)) {
            (true, true) => Ok(()),
            (true, false) => Err(DeviceError::Lost),
            (false, _) => Err(DeviceError::Exhausted),
        }
    }
}
