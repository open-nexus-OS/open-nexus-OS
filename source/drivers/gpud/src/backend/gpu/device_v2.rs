// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! G2 on the device (TASK-0169B P2, RFC-0101 §1–§6, §8c; TASK-0215 5c-3 P2): `GfxDevice` over
//! the G1 [`Gpu`]. Several contexts, each its client's and its own GPU address space
//! ([`super::contexts`]); buffers as DMA VMOs with their cache mode — CPU-mapped, or the GPU's
//! alone — or a plane of the ring adopted by its bus address ([`super::bos`]), bound in ranges
//! through the context's page tables with the waited MMU-cache flush; a transfer submit is one
//! transfer command plus the timeline `UPDATE` in the context's transfer queue, a render submit
//! one pass on the context's render objects; one kick, one interrupt, one point. The device
//! numbers the points (the firmware writes the sequence); `reached` reads the word. Every
//! refusal is by name: a queue kind the device does not execute, the tree's command vocabulary,
//! a cache mode M2 has not brought, a stream the decoder refused, a reference outside the
//! context's space, a buffer still bound, a context that is not the client's.

use alloc::boxed::Box;

use nexus_abi::DmaVmo;
use nexus_driverkit::gpu::{
    Binding, CacheMode, Capabilities, CodeTarget, GpuVa, Priority, QueueKind, QueueKinds,
    TimelinePoint, VmError,
};
use nexus_gfx::backend::gpu::fwif::{self, TransferCmd};
use nexus_gfx::backend::gpu::mmu::PAGE;
use nexus_gfx::backend::gpu::pipelines::{PipelineTable, Placed};
use nexus_gfx::backend::gpu::scene::RenderSetup;
use nexus_gfx::backend::v2::{
    Batch, BoId, ClientId, ContextId, DeviceError, GfxDevice, Mapping, Stream,
};
use nexus_service_topology::DEVICE_MMIO_SLOT;

use super::bos::{Backing, Bo, Bos};
use super::contexts::{Contexts, Holdings, Slot, MAX_CONTEXTS, RENDER, TRANSFER};
use super::device::{Gpu, Tables};
use super::gpu_vmo::GpuVmo;
use super::job::Queue;
use super::memory::HeapObj;
use super::render::Render;

/// The device: the G1 state plus G2's bookkeeping.
pub(crate) struct Device {
    pub(crate) gpu: Box<Gpu>,
    bos: Bos,
    contexts: Contexts,
}

impl Device {
    pub(crate) fn new(gpu: Box<Gpu>) -> Self {
        Self { gpu, bos: Bos::new(), contexts: Contexts::new() }
    }

    /// The open driver's flags for `bo`; they apply when it is bound, so a bound buffer's are
    /// refused rather than silently left as they were mapped.
    pub(super) fn set_bo_flags(
        &mut self,
        client: ClientId,
        bo: BoId,
        device_cached: bool,
        pm_fw_protect: bool,
    ) -> Result<(), DeviceError> {
        let b = self.bos.get_mut(client, bo)?;
        if b.bindings > 0 {
            return Err(DeviceError::BufferBound);
        }
        b.device_cached = device_cached;
        b.pm_fw_protect = pm_fw_protect;
        Ok(())
    }

    /// A plane of the ring as a buffer of `client`: the display's memory at `bus`, `len` bytes.
    pub(super) fn adopt_plane(
        &mut self,
        client: ClientId,
        bus: u64,
        len: usize,
    ) -> Result<BoId, DeviceError> {
        if len == 0 || len % PAGE as usize != 0 || bus % PAGE != 0 {
            return Err(DeviceError::Vm(VmError::Unaligned));
        }
        self.bos.insert(Bo {
            client,
            len,
            backing: Backing::Plane { bus },
            bindings: 0,
            device_cached: false,
            pm_fw_protect: false,
        })
    }

    /// The render pass's kernel-side objects for `client`'s context (§8d): the render context,
    /// the free lists (their page numbers written into the client's bound list buffers) and the
    /// dataset. Once per context.
    pub(super) fn render_setup(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        setup: &RenderSetup<'_>,
    ) -> Result<(), DeviceError> {
        let slot = self.contexts.live(client, ctx)?;
        if slot.render.is_some() {
            return Err(DeviceError::Exhausted);
        }
        let tiles = self.gpu.fw.tiles.ok_or(DeviceError::Lost)?;
        // The list buffers must be bound before the lists are built (§1).
        for fl in &setup.freelists {
            slot.space.resolve(GpuVa(fl.gpu_addr), fl.list_bytes() as u64)?;
        }
        let fw_mem_ctx = slot.mmu.as_ref().ok_or(DeviceError::Lost)?.fw_mem_ctx.fw_addr();
        let render =
            Render::create(&mut self.gpu, setup, tiles, fw_mem_ctx, ctx.0, slot.fw_priority())?;
        let block = self.gpu.mem.block();
        let bos = &mut self.bos;
        let space = &slot.space;
        let written = (0..2).all(|list| {
            render.entries(list, |args, index, value| {
                let va = GpuVa(args.gpu_addr + u64::from(index) * 4);
                let Ok(b) = space.resolve(va, 4) else { return false };
                let at = b.offset_of(va) as usize;
                match bos.get_mut(client, BoId(b.bo)).ok().and_then(|bo| bo.bytes_mut(at, 4)) {
                    Some(dst) => {
                        dst.copy_from_slice(&value.to_le_bytes());
                        if block != 0 {
                            nexus_abi::cache_clean(dst, block);
                        }
                        true
                    }
                    None => false,
                }
            })
        });
        if !written {
            // The firmware never ran on them: their cleanup answers at once.
            let _ = render.destroy(&mut self.gpu);
            return Err(DeviceError::Vm(VmError::Unbound));
        }
        slot.render = Some(render);
        Ok(())
    }

    /// The CPU view of `len` bytes bound at `va` in `client`'s context (a CPU-mapped buffer).
    pub(super) fn bound_bytes_mut(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        va: u64,
        len: usize,
    ) -> Option<&mut [u8]> {
        let slot = self.contexts.get(client, ctx).ok()?;
        let b = slot.space.resolve(GpuVa(va), len as u64).ok()?;
        let at = b.offset_of(GpuVa(va)) as usize;
        self.bos.get_mut(client, BoId(b.bo)).ok()?.bytes_mut(at, len)
    }

    /// Ownership CPU → device for `len` bytes bound at `va` (the non-coherent machine's rule).
    pub(super) fn clean_bound(&self, client: ClientId, ctx: ContextId, va: u64, len: usize) {
        let block = self.gpu.mem.block();
        let Ok(slot) = self.contexts.get(client, ctx) else { return };
        let Ok(b) = slot.space.resolve(GpuVa(va), len as u64) else { return };
        let at = b.offset_of(GpuVa(va)) as usize;
        if let Some(bytes) = self.bos.get(client, BoId(b.bo)).ok().and_then(|bo| bo.bytes(at, len))
        {
            if block != 0 {
                nexus_abi::cache_clean(bytes, block);
            }
        }
    }

    /// The CPU view of a whole CPU-mapped buffer of `client`'s (a buffer it fills before
    /// binding it).
    pub(super) fn bo_bytes_mut(&mut self, client: ClientId, bo: BoId) -> Option<&mut [u8]> {
        let b = self.bos.get_mut(client, bo).ok()?;
        let len = b.len;
        b.bytes_mut(0, len)
    }

    /// Ownership CPU → device for a whole CPU-mapped buffer.
    pub(super) fn clean(&self, client: ClientId, bo: BoId) {
        let block = self.gpu.mem.block();
        if let Ok(Bo { backing: Backing::Vmo(v), .. }) = self.bos.get(client, bo) {
            if block != 0 {
                nexus_abi::cache_clean(v.bytes(), block);
            }
        }
    }

    /// The render queues' timeline objects and their last points `(geometry, fragment)` of
    /// `client`'s context, for a wait that stamps each stage.
    pub(super) fn render_progress(
        &self,
        client: ClientId,
        ctx: ContextId,
    ) -> Option<((HeapObj, u32), (HeapObj, u32))> {
        self.contexts.get(client, ctx).ok()?.render.as_ref().map(|r| r.progress())
    }

    /// The render queues' timeline words `(geometry, fragment)` of `client`'s context.
    pub(super) fn render_words(&self, client: ClientId, ctx: ContextId) -> Option<(u32, u32)> {
        let r = self.contexts.get(client, ctx).ok()?.render.as_ref()?;
        Some((r.geom_word(&self.gpu), r.frag_word(&self.gpu)))
    }

    /// The page catalogue of `client`'s context — what the firmware names its faults by.
    pub(super) fn catalogue(&self, client: ClientId, ctx: ContextId) -> Option<u64> {
        Some(self.contexts.get(client, ctx).ok()?.mmu.as_ref()?.root_phys())
    }

    /// Waits for `point` with the device's interrupt-ended slices.
    pub(super) fn wait_point(&mut self, point: TimelinePoint, deadline_ns: u64) -> bool {
        let contexts = &self.contexts;
        self.gpu.wait_until(deadline_ns, |g| contexts.reached(g, point))
    }

    /// The pipeline table of `client`'s context, taken out to be filled (the library's heaps
    /// write through the device meanwhile); [`Self::set_pipelines`] puts it back.
    pub(super) fn take_pipelines(
        &mut self,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<PipelineTable, DeviceError> {
        Ok(core::mem::take(&mut self.contexts.live(client, ctx)?.pipelines))
    }

    pub(super) fn set_pipelines(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        table: PipelineTable,
    ) -> Result<(), DeviceError> {
        self.contexts.live(client, ctx)?.pipelines = table;
        Ok(())
    }

    /// The pipeline the context's library named `name`.
    pub(super) fn pipeline(&self, client: ClientId, ctx: ContextId, name: &str) -> Option<Placed> {
        let table = &self.contexts.get(client, ctx).ok()?.pipelines;
        table.id_of(name).and_then(|id| table.get(id).ok()).cloned()
    }

    /// What the device holds: contexts live and quarantined, buffers, free backing pages.
    pub(super) fn holdings(&self) -> Holdings {
        self.contexts.holdings(&self.gpu, &self.bos)
    }

    /// Unmaps `pages` pages at `va` from `slot`'s tables and flushes; whether the firmware
    /// answered.
    fn unmap(gpu: &mut Gpu, slot: &mut Slot, va: u64, pages: u64) -> bool {
        let Some(mmu) = slot.mmu.as_mut() else { return false };
        for i in 0..pages {
            let _ = mmu.tables.unmap(&mut Tables(&mut gpu.mem), va + i * PAGE);
        }
        let flush = mmu.tables.take_flush_flags();
        gpu.mmu_flush(flush)
    }
}

impl GfxDevice for Device {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Declared; compute is refused by name until its layer (§2).
            queue_kinds: QueueKinds::ALL,
            max_contexts: MAX_CONTEXTS as u32,
            // One job per batch in this layer.
            max_batch_commands: 1,
            va_bits: 40,
            page_size: PAGE as u32,
            executes_gpu_code: true,
            gpu_code_target: CodeTarget(self.gpu.win.core_id()),
        }
    }

    fn create_context(
        &mut self,
        client: ClientId,
        priority: Priority,
    ) -> Result<ContextId, DeviceError> {
        self.contexts.create(&mut self.gpu, client, priority)
    }

    fn destroy_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        self.contexts.destroy(&mut self.gpu, &mut self.bos, client, ctx)
    }

    fn create_bo(
        &mut self,
        client: ClientId,
        bytes: usize,
        mode: CacheMode,
    ) -> Result<BoId, DeviceError> {
        if bytes == 0 {
            return Err(DeviceError::Vm(VmError::Unaligned));
        }
        let len = bytes.div_ceil(PAGE as usize) * PAGE as usize;
        let backing = match mode {
            CacheMode::CpuCached => Backing::Vmo(
                DmaVmo::anonymous(DEVICE_MMIO_SLOT, len).map_err(|_| DeviceError::Exhausted)?,
            ),
            CacheMode::GpuPrivate => {
                Backing::Gpu(GpuVmo::new(DEVICE_MMIO_SLOT, len).ok_or(DeviceError::Exhausted)?)
            }
            CacheMode::Uncached => return Err(DeviceError::CacheModeUnsupported(mode)),
        };
        self.bos.insert(Bo {
            client,
            len,
            backing,
            bindings: 0,
            device_cached: false,
            pm_fw_protect: false,
        })
    }

    fn destroy_bo(&mut self, client: ClientId, bo: BoId) -> Result<(), DeviceError> {
        self.bos.remove(client, bo)
    }

    fn vm_bind(&mut self, client: ClientId, ctx: ContextId, m: Mapping) -> Result<(), DeviceError> {
        let slot = self.contexts.live(client, ctx)?;
        let b = self.bos.get(client, m.bo)?;
        let end = m.offset.checked_add(m.len).ok_or(VmError::OutOfRange)?;
        if end > b.len as u64 {
            return Err(DeviceError::Vm(VmError::OutOfRange));
        }
        let flags = b.page_flags(m.access);
        slot.space.bind(Binding {
            va: m.va,
            len: m.len,
            bo: m.bo.0,
            offset: m.offset,
            access: m.access,
        })?;
        let gpu = &mut *self.gpu;
        let Some(mmu) = slot.mmu.as_mut() else {
            let _ = slot.space.unbind(m.va);
            return Err(DeviceError::Lost);
        };
        // The buffer's pages walked while the tables are written, nothing collected.
        let mut mapped = 0u64;
        let walked = b.walk(m.offset, m.len, |phys| {
            let va = m.va.0 + mapped * PAGE;
            if mmu.tables.map(&mut Tables(&mut gpu.mem), va, phys, flags).is_err() {
                return false;
            }
            mapped += 1;
            true
        });
        if !walked {
            for i in 0..mapped {
                let _ = mmu.tables.unmap(&mut Tables(&mut gpu.mem), m.va.0 + i * PAGE);
            }
        }
        let flush = mmu.tables.take_flush_flags();
        let flushed = gpu.mmu_flush(flush);
        if !walked || !flushed {
            let _ = slot.space.unbind(m.va);
            return Err(if walked { DeviceError::Lost } else { DeviceError::Exhausted });
        }
        if let Some(b) = self.bos.any_mut(m.bo) {
            b.bindings += 1;
        }
        Ok(())
    }

    fn vm_unbind(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        va: GpuVa,
    ) -> Result<(), DeviceError> {
        let slot = self.contexts.live(client, ctx)?;
        let binding = slot.space.unbind(va)?;
        let answered = Self::unmap(&mut self.gpu, slot, binding.va.0, binding.len / PAGE);
        if let Some(b) = self.bos.any_mut(BoId(binding.bo)) {
            b.bindings = b.bindings.saturating_sub(1);
        }
        if answered {
            Ok(())
        } else {
            Err(DeviceError::Lost)
        }
    }

    fn submit(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        kind: QueueKind,
        batch: &Batch<'_>,
    ) -> Result<TimelinePoint, DeviceError> {
        // Transfer and render execute; compute is declared and refused by name (§2). A stream
        // on the wrong queue is refused as that queue's.
        let fits = matches!(
            (kind, &batch.stream),
            (QueueKind::Transfer, Stream::Transfer { .. })
                | (QueueKind::Render, Stream::Render(_))
                | (_, Stream::Commands(_))
                | (_, Stream::Code { .. })
        );
        if kind == QueueKind::Compute || !fits {
            return Err(DeviceError::QueueUnsupported(kind));
        }
        self.contexts.get(client, ctx)?;
        if let Some(&pending) = batch.waits.iter().find(|&&p| !self.reached(p)) {
            return Err(DeviceError::WouldBlock(pending));
        }
        let slot = self.contexts.live(client, ctx)?;
        // The target must be bound in this context (§1: nothing names memory outside it).
        slot.space.resolve(batch.target.va, batch.target.bytes())?;
        let gpu = &mut *self.gpu;
        match batch.stream {
            Stream::Commands(_) => Err(DeviceError::CommandsUnsupported),
            Stream::Code { .. } => Err(DeviceError::CodeUnsupported),
            Stream::Render(rs) => {
                let feats = gpu.fw.render;
                let render = slot.render.as_mut().ok_or(DeviceError::NoRenderTarget)?;
                let f2 = render.submit(gpu, &rs, feats)?;
                Ok(Slot::point(ctx, RENDER, f2))
            }
            Stream::Transfer { stream, single_core } => {
                let cmd = TransferCmd::from_stream(stream, gpu.fw.stream, single_core)
                    .map_err(|_| DeviceError::MalformedStream)?;
                if slot.transfer.is_none() {
                    let fw_mem_ctx =
                        slot.mmu.as_ref().ok_or(DeviceError::Lost)?.fw_mem_ctx.fw_addr();
                    let queue = Queue::create(gpu, fw_mem_ctx, ctx.0, slot.fw_priority())
                        .ok_or(DeviceError::Exhausted)?;
                    slot.transfer = Some(queue);
                }
                let queue = slot.transfer.as_mut().ok_or(DeviceError::Lost)?;
                let seq = queue
                    .submit_task(gpu, fwif::ccb::TQ_3D, &cmd.to_bytes())
                    .map_err(|_| DeviceError::Lost)?;
                Ok(Slot::point(ctx, TRANSFER, seq))
            }
        }
    }

    fn reached(&self, point: TimelinePoint) -> bool {
        self.contexts.reached(&self.gpu, point)
    }

    fn reset_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        let slot = self.contexts.get(client, ctx)?;
        // Nothing in flight is cancelled by force in this layer: a job still running is named.
        let pending = [
            slot.transfer.as_ref().map(|q| Slot::point(ctx, TRANSFER, q.seq)),
            slot.render.as_ref().map(|r| Slot::point(ctx, RENDER, r.progress().1 .1)),
        ];
        match pending.into_iter().flatten().find(|&p| !self.reached(p)) {
            Some(p) => Err(DeviceError::WouldBlock(p)),
            None => Ok(()),
        }
    }
}
