// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! G2 on the device (TASK-0169B P2, RFC-0101 §1–§5, §8c): `GfxDevice` over the G1 [`Gpu`].
//! One context (the device's single address space of layer 3, owned by the first client that
//! asks), buffers as DMA VMOs with their cache mode — or a plane of the ring adopted by its bus
//! address — bound through the G1 page tables with the waited MMU-cache flush, and a submit
//! that is one transfer command plus the timeline `UPDATE` in the transfer context's client
//! buffer, one kick, one interrupt, one point. The device numbers the points (the firmware
//! writes the sequence); `reached` reads the word. Every refusal is by name: a queue kind the
//! device does not execute, the tree's command vocabulary, a cache mode M2 has not brought, a
//! stream the decoder refused, a reference outside the context's space.

use alloc::boxed::Box;
use alloc::vec::Vec;

use nexus_abi::DmaVmo;
use nexus_driverkit::gpu::{
    Access, Binding, CacheMode, Capabilities, CodeTarget, GpuVa, Priority, QueueKind, QueueKinds,
    TimelinePoint, VmError, VmSpace,
};
use nexus_gfx::backend::gpu::fwif::{self, TransferCmd};
use nexus_gfx::backend::gpu::mmu::{PageFlags, PAGE};
use nexus_gfx::backend::v2::{Batch, BoId, ClientId, ContextId, DeviceError, GfxDevice, Stream};
use nexus_service_topology::DEVICE_MMIO_SLOT;

use nexus_gfx::backend::gpu::scene::RenderSetup;

use super::device::{Gpu, Tables};
use super::job;
use super::render::{Render, QUEUE_SHIFT, RENDER_QUEUE};

/// Bindings one context holds — the first quad binds 29.
const MAX_BINDINGS: usize = 64;
const MAX_BOS: usize = 64;
/// The one context of this layer.
const CONTEXT: ContextId = ContextId(1);

enum Backing {
    /// The device's own pages, CPU-mapped (`CpuCached`).
    Vmo(DmaVmo),
    /// A plane of the ring: the display's memory at a bus address, maintained by the display.
    Plane { bus: u64 },
}

struct Bo {
    client: ClientId,
    /// Bytes, a multiple of the page.
    len: usize,
    backing: Backing,
    bound: bool,
}

/// The device: the G1 state plus G2's bookkeeping.
pub(crate) struct Device {
    pub(crate) gpu: Box<Gpu>,
    bos: Vec<Option<Bo>>,
    space: VmSpace<MAX_BINDINGS>,
    owner: Option<ClientId>,
    /// The render context, free lists and dataset, once a client set them up (§8d).
    render: Option<Render>,
}

impl Device {
    pub(crate) fn new(gpu: Box<Gpu>) -> Self {
        Self {
            gpu,
            bos: Vec::new(),
            space: VmSpace::new(40, PAGE as u32),
            owner: None,
            render: None,
        }
    }

    /// The render pass's kernel-side objects for `client`'s context (§8d): the render context,
    /// the free lists (their page numbers written into the client's bound list buffers) and
    /// the dataset. Once per device in this layer.
    pub(super) fn render_setup(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        setup: &RenderSetup<'_>,
    ) -> Result<(), DeviceError> {
        self.check_ctx(client, ctx)?;
        if self.render.is_some() {
            return Err(DeviceError::Exhausted);
        }
        let tiles = self.gpu.fw.tiles.ok_or(DeviceError::Lost)?;
        // The list buffers must be bound before the lists are built (§1).
        for fl in &setup.freelists {
            self.space.resolve(GpuVa(fl.gpu_addr), fl.list_bytes() as u64)?;
        }
        let (render, lists) = Render::create(&mut self.gpu, setup, tiles)?;
        for list in &lists {
            let binding =
                *self.space.resolve(GpuVa(list.args.gpu_addr), list.args.list_bytes() as u64)?;
            let at =
                (list.args.gpu_addr - binding.va.0) as usize + list.args.first_entry() as usize * 4;
            let bo = BoId(binding.bo);
            let bytes = self.bo_bytes_mut(client, bo).ok_or(DeviceError::NoSuchBuffer)?;
            for (i, e) in list.entries.iter().enumerate() {
                if let Some(dst) = bytes.get_mut(at + i * 4..at + i * 4 + 4) {
                    dst.copy_from_slice(&e.to_le_bytes());
                }
            }
            self.clean(client, bo);
        }
        self.render = Some(render);
        Ok(())
    }

    /// The buffer bound over `va..va + len`, if one is.
    pub(super) fn bo_at(&self, va: u64, len: u64) -> Option<BoId> {
        self.space.resolve(GpuVa(va), len).ok().map(|b| BoId(b.bo))
    }

    /// The render queues' timeline words `(geometry, fragment)`, for the rung's lines.
    pub(super) fn render_words(&self) -> Option<(u32, u32)> {
        self.render.as_ref().map(|r| (r.geom_word(&self.gpu), r.frag_word(&self.gpu)))
    }

    /// Whether `point` (its queue in the high word) is reached.
    fn point_reached(&self, point: TimelinePoint) -> bool {
        let seq = point.0 as u32;
        match point.0 >> QUEUE_SHIFT {
            RENDER_QUEUE => self
                .render
                .as_ref()
                .is_some_and(|r| r.frag_word(&self.gpu).wrapping_sub(seq) < 0x8000_0000),
            _ => self
                .gpu
                .context
                .as_ref()
                .is_some_and(|c| job::timeline(&self.gpu, c).wrapping_sub(seq) < 0x8000_0000),
        }
    }

    fn check_ctx(&self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        match self.owner {
            None => Err(DeviceError::NoSuchContext),
            Some(o) if o != client => Err(DeviceError::NotOwner),
            Some(_) if ctx != CONTEXT => Err(DeviceError::NoSuchContext),
            Some(_) => Ok(()),
        }
    }

    fn bo(&self, client: ClientId, bo: BoId) -> Result<&Bo, DeviceError> {
        let b = self
            .bos
            .get(bo.0 as usize)
            .and_then(|b| b.as_ref())
            .ok_or(DeviceError::NoSuchBuffer)?;
        if b.client != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(b)
    }

    fn insert(&mut self, bo: Bo) -> Result<BoId, DeviceError> {
        if let Some(i) = self.bos.iter().position(|b| b.is_none()) {
            self.bos[i] = Some(bo);
            return Ok(BoId(i as u32));
        }
        if self.bos.len() >= MAX_BOS {
            return Err(DeviceError::Exhausted);
        }
        self.bos.push(Some(bo));
        Ok(BoId(self.bos.len() as u32 - 1))
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
        self.insert(Bo { client, len, backing: Backing::Plane { bus }, bound: false })
    }

    /// The CPU view of a VMO-backed buffer (a plane's bytes are the display's).
    pub(super) fn bo_bytes_mut(&mut self, client: ClientId, bo: BoId) -> Option<&mut [u8]> {
        self.bo(client, bo).ok()?;
        match &mut self.bos[bo.0 as usize].as_mut()?.backing {
            Backing::Vmo(v) => Some(v.bytes_mut()),
            Backing::Plane { .. } => None,
        }
    }

    /// Ownership CPU → device for a VMO-backed buffer (the non-coherent machine's rule).
    pub(super) fn clean(&self, client: ClientId, bo: BoId) {
        if let Ok(Bo { backing: Backing::Vmo(v), .. }) = self.bo(client, bo) {
            let block = self.gpu.mem.block();
            if block != 0 {
                nexus_abi::cache_clean(v.bytes(), block);
            }
        }
    }

    /// The physical pages behind a buffer, in order.
    fn pages(bo: &Bo) -> Vec<u64> {
        let mut pages = Vec::with_capacity(bo.len / PAGE as usize);
        match &bo.backing {
            Backing::Vmo(v) => {
                for run in v.runs() {
                    let n = run.len.div_ceil(PAGE);
                    pages.extend((0..n).map(|i| run.bus + i * PAGE));
                }
            }
            Backing::Plane { bus } => {
                pages.extend((0..bo.len as u64 / PAGE).map(|i| bus + i * PAGE));
            }
        }
        pages.truncate(bo.len / PAGE as usize);
        pages
    }

    /// Unmaps `va`'s pages and flushes; whether the firmware answered.
    fn unmap_range(&mut self, va: u64, len: u64) -> bool {
        let gpu = &mut *self.gpu;
        let Some(mut tables) = gpu.tables.take() else {
            return false;
        };
        for i in 0..len / PAGE {
            let _ = tables.unmap(&mut Tables(&mut gpu.mem), va + i * PAGE);
        }
        let flush = tables.take_flush_flags();
        gpu.tables = Some(tables);
        gpu.mmu_flush(flush)
    }

    /// Waits for `point` with the device's interrupt-ended slices; bridges the kernel fence.
    pub(super) fn wait_point(&mut self, point: TimelinePoint, deadline_ns: u64) -> bool {
        let p = point.0 as u32;
        let render_tl = (point.0 >> QUEUE_SHIFT == RENDER_QUEUE)
            .then(|| self.render.as_ref().map(|r| r.frag_timeline_obj()))
            .flatten();
        let done = self.gpu.wait_until(deadline_ns, |g| match render_tl {
            Some(tl) => g.mem.read_u32(&tl, 0).wrapping_sub(p) < 0x8000_0000,
            None => g
                .context
                .as_ref()
                .is_some_and(|c| job::timeline(g, c).wrapping_sub(p) < 0x8000_0000),
        });
        if let Some(mut ctx) = self.gpu.context.take() {
            job::bridge(&mut self.gpu, &mut ctx);
            self.gpu.context = Some(ctx);
        }
        done
    }
}

impl GfxDevice for Device {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Declared; render and compute are refused by name until their layers (§2).
            queue_kinds: QueueKinds::ALL,
            max_contexts: 1,
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
        _priority: Priority,
    ) -> Result<ContextId, DeviceError> {
        if self.owner.is_some() || self.gpu.context.is_none() {
            return Err(DeviceError::Exhausted);
        }
        self.owner = Some(client);
        Ok(CONTEXT)
    }

    fn destroy_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        self.check_ctx(client, ctx)?;
        let bound: Vec<(u64, u64)> = self.space.bindings().map(|b| (b.va.0, b.len)).collect();
        for (va, len) in bound {
            let _ = self.space.unbind(GpuVa(va));
            self.unmap_range(va, len);
        }
        for slot in self.bos.iter_mut() {
            if slot.as_ref().is_some_and(|b| b.client == client) {
                *slot = None;
            }
        }
        self.owner = None;
        Ok(())
    }

    fn create_bo(
        &mut self,
        client: ClientId,
        bytes: usize,
        mode: CacheMode,
    ) -> Result<BoId, DeviceError> {
        if mode != CacheMode::CpuCached {
            return Err(DeviceError::CacheModeUnsupported(mode));
        }
        if bytes == 0 {
            return Err(DeviceError::Vm(VmError::Unaligned));
        }
        let len = bytes.div_ceil(PAGE as usize) * PAGE as usize;
        let vmo = DmaVmo::anonymous(DEVICE_MMIO_SLOT, len).map_err(|_| DeviceError::Exhausted)?;
        self.insert(Bo { client, len, backing: Backing::Vmo(vmo), bound: false })
    }

    fn vm_bind(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        bo: BoId,
        va: GpuVa,
        access: Access,
    ) -> Result<(), DeviceError> {
        self.check_ctx(client, ctx)?;
        let b = self.bo(client, bo)?;
        if b.bound {
            return Err(DeviceError::Vm(VmError::Overlap));
        }
        let len = b.len as u64;
        let pages = Self::pages(b);
        self.space.bind(Binding { va, len, bo: bo.0, access })?;
        let flags = PageFlags {
            read_only: access == Access::ReadOnly,
            slc_bypass: true,
            pm_fw_protect: false,
        };
        let gpu = &mut *self.gpu;
        let Some(mut tables) = gpu.tables.take() else {
            let _ = self.space.unbind(va);
            return Err(DeviceError::Lost);
        };
        let mut mapped = 0u64;
        let mut failed = false;
        for (i, phys) in pages.iter().enumerate() {
            let page_va = va.0 + i as u64 * PAGE;
            if tables.map(&mut Tables(&mut gpu.mem), page_va, *phys, flags).is_err() {
                failed = true;
                break;
            }
            mapped += 1;
        }
        if failed {
            for i in 0..mapped {
                let _ = tables.unmap(&mut Tables(&mut gpu.mem), va.0 + i * PAGE);
            }
        }
        let flush = tables.take_flush_flags();
        gpu.tables = Some(tables);
        let flushed = gpu.mmu_flush(flush);
        if failed {
            let _ = self.space.unbind(va);
            return Err(DeviceError::Exhausted);
        }
        if !flushed {
            let _ = self.space.unbind(va);
            return Err(DeviceError::Lost);
        }
        if let Some(Some(b)) = self.bos.get_mut(bo.0 as usize) {
            b.bound = true;
        }
        Ok(())
    }

    fn vm_unbind(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        va: GpuVa,
    ) -> Result<(), DeviceError> {
        self.check_ctx(client, ctx)?;
        let binding = self.space.unbind(va)?;
        let answered = self.unmap_range(binding.va.0, binding.len);
        if let Some(Some(b)) = self.bos.get_mut(binding.bo as usize) {
            b.bound = false;
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
        self.check_ctx(client, ctx)?;
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
        if let Some(&pending) = batch.waits.iter().find(|&&p| !self.reached(p)) {
            return Err(DeviceError::WouldBlock(pending));
        }
        let (stream, single_core) = match batch.stream {
            Stream::Transfer { stream, single_core } => (stream, single_core),
            Stream::Commands(_) => return Err(DeviceError::CommandsUnsupported),
            Stream::Code { .. } => return Err(DeviceError::CodeUnsupported),
            Stream::Render(rs) => {
                // The target must be bound in this context (§1: nothing names memory outside it).
                self.space.resolve(batch.target.va, batch.target.bytes())?;
                let feats = self.gpu.fw.render;
                let render = self.render.as_mut().ok_or(DeviceError::NoRenderTarget)?;
                let f2 = render.submit(&mut self.gpu, &rs, feats)?;
                return Ok(TimelinePoint((RENDER_QUEUE << QUEUE_SHIFT) | u64::from(f2)));
            }
        };
        // The target must be bound in this context (§1: nothing names memory outside it).
        self.space.resolve(batch.target.va, batch.target.bytes())?;
        let cmd = TransferCmd::from_stream(stream, self.gpu.fw.stream, single_core)
            .map_err(|_| DeviceError::MalformedStream)?;
        let Some(mut fwctx) = self.gpu.context.take() else {
            return Err(DeviceError::NoSuchContext);
        };
        let r = job::submit_task(&mut self.gpu, &mut fwctx, fwif::ccb::TQ_3D, &cmd.to_bytes());
        self.gpu.context = Some(fwctx);
        r.map(|seq| TimelinePoint(u64::from(seq))).map_err(|_| DeviceError::Lost)
    }

    fn reached(&self, point: TimelinePoint) -> bool {
        self.point_reached(point)
    }

    fn reset_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        self.check_ctx(client, ctx)?;
        // Nothing in flight is cancelled by force in this layer: a job still running is named.
        let last = self.gpu.context.as_ref().map(|c| c.seq).unwrap_or(0);
        if self.reached(TimelinePoint(u64::from(last))) {
            Ok(())
        } else {
            Err(DeviceError::WouldBlock(TimelinePoint(u64::from(last))))
        }
    }
}
