// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The mock device — `GfxDevice` (RFC-0101) on the host, the oracle every later
//! package is proven against: contexts per client with their own [`VmSpace`], buffers as
//! bytes with their cache mode (destroyed by their client once no context binds them), ranges
//! of them bound, one batch per submit checked against the context's space and the device's
//! bound, points signalled on the one timeline. It executes the tree's
//! command vocabulary only as far as proving the data flow (`DrawTiles` lands in the bound
//! target); pixels through v2 are Phase 3's. GPU code is refused by name: the mock executes
//! none, and says so.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests (this file) — the reject matrix of RFC-0101 §9
//! INVARIANTS: no batch runs with a reference outside its context's space; a context or
//!   buffer answers only to its owner; a range binds inside its buffer; a bound buffer is never
//!   destroyed; the timeline never moves backwards.

use alloc::vec;
use alloc::vec::Vec;

use nexus_driverkit::gpu::{check_batch, Binding, VmSpace};

use super::{
    Batch, BoId, CacheMode, Capabilities, ClientId, CodeTarget, ContextId, DeviceError, GfxDevice,
    GpuVa, Mapping, Priority, QueueKind, QueueKinds, Stream, TimelinePoint, VmError,
};
use crate::command::buffer::Command;
use crate::sync::timeline::Timeline;

/// Bindings a mock context holds.
pub const MOCK_BINDINGS: usize = 32;
/// The mock's page.
pub const MOCK_PAGE: u32 = 4096;

struct Context {
    owner: ClientId,
    priority: Priority,
    space: VmSpace<MOCK_BINDINGS>,
    resets: u32,
}

struct Bo {
    owner: ClientId,
    mode: CacheMode,
    bytes: Vec<u8>,
    /// Live bindings of the buffer, in every context.
    bindings: u32,
}

/// The mock device.
pub struct CpuMockDevice {
    caps: Capabilities,
    contexts: Vec<Option<Context>>,
    bos: Vec<Option<Bo>>,
    timeline: Timeline,
    batches: u64,
}

impl CpuMockDevice {
    /// A device with `caps`.
    pub fn new(caps: Capabilities) -> Self {
        Self { caps, contexts: Vec::new(), bos: Vec::new(), timeline: Timeline::new(), batches: 0 }
    }

    /// A device as Phase 2 declares it: render and transfer, no GPU code.
    pub fn phase2() -> Self {
        Self::new(Capabilities {
            queue_kinds: QueueKinds::RENDER_TRANSFER,
            max_contexts: 8,
            max_batch_commands: 256,
            va_bits: 40,
            page_size: MOCK_PAGE,
            executes_gpu_code: false,
            gpu_code_target: CodeTarget(0x24001D003400B6),
        })
    }

    /// A buffer's bytes, for the owner (tests read the picture back here).
    pub fn bo_bytes(&self, client: ClientId, bo: BoId) -> Result<&[u8], DeviceError> {
        let bo = self.bo(bo)?;
        if bo.owner != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(&bo.bytes)
    }

    /// A buffer's cache mode.
    pub fn bo_mode(&self, bo: BoId) -> Result<CacheMode, DeviceError> {
        self.bo(bo).map(|b| b.mode)
    }

    /// Batches executed so far.
    pub const fn batches(&self) -> u64 {
        self.batches
    }

    /// A context's priority and reset count.
    pub fn context_state(&self, ctx: ContextId) -> Result<(Priority, u32), DeviceError> {
        self.context(ctx).map(|c| (c.priority, c.resets))
    }

    fn context(&self, ctx: ContextId) -> Result<&Context, DeviceError> {
        self.contexts.get(ctx.0 as usize).and_then(Option::as_ref).ok_or(DeviceError::NoSuchContext)
    }

    fn owned_context(
        &mut self,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<&mut Context, DeviceError> {
        let context = self
            .contexts
            .get_mut(ctx.0 as usize)
            .and_then(Option::as_mut)
            .ok_or(DeviceError::NoSuchContext)?;
        if context.owner != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(context)
    }

    fn bo(&self, bo: BoId) -> Result<&Bo, DeviceError> {
        self.bos.get(bo.0 as usize).and_then(Option::as_ref).ok_or(DeviceError::NoSuchBuffer)
    }

    /// Runs the commands the mock executes: `DrawTiles` into the target's bytes (RGBA8).
    fn execute(target: &mut [u8], width: u32, height: u32, commands: &[Command]) {
        for command in commands {
            if let Command::DrawTiles { tiles, color } = command {
                for tile in tiles {
                    let right = tile.x.saturating_add(tile.width).min(width);
                    let bottom = tile.y.saturating_add(tile.height).min(height);
                    for y in tile.y.min(bottom)..bottom {
                        for x in tile.x.min(right)..right {
                            let at = (y as usize * width as usize + x as usize) * 4;
                            if let Some(px) = target.get_mut(at..at + 4) {
                                px.copy_from_slice(&color.0.to_le_bytes());
                            }
                        }
                    }
                }
            }
        }
    }
}

impl GfxDevice for CpuMockDevice {
    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn create_context(
        &mut self,
        client: ClientId,
        priority: Priority,
    ) -> Result<ContextId, DeviceError> {
        let alive = self.contexts.iter().flatten().count() as u32;
        if alive >= self.caps.max_contexts {
            return Err(DeviceError::Exhausted);
        }
        let context = Context {
            owner: client,
            priority,
            space: VmSpace::new(self.caps.va_bits, self.caps.page_size),
            resets: 0,
        };
        self.contexts.push(Some(context));
        Ok(ContextId((self.contexts.len() - 1) as u32))
    }

    fn destroy_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        let empty = VmSpace::new(self.caps.va_bits, self.caps.page_size);
        let space = core::mem::replace(&mut self.owned_context(client, ctx)?.space, empty);
        for b in space.bindings() {
            if let Some(Some(bo)) = self.bos.get_mut(b.bo as usize) {
                bo.bindings = bo.bindings.saturating_sub(1);
            }
        }
        self.contexts[ctx.0 as usize] = None;
        Ok(())
    }

    fn create_bo(
        &mut self,
        client: ClientId,
        bytes: usize,
        mode: CacheMode,
    ) -> Result<BoId, DeviceError> {
        let page = self.caps.page_size as usize;
        let rounded = bytes.div_ceil(page).checked_mul(page).ok_or(DeviceError::Exhausted)?;
        if rounded == 0 {
            return Err(DeviceError::Vm(nexus_driverkit::gpu::VmError::Unaligned));
        }
        self.bos.push(Some(Bo { owner: client, mode, bytes: vec![0; rounded], bindings: 0 }));
        Ok(BoId((self.bos.len() - 1) as u32))
    }

    fn destroy_bo(&mut self, client: ClientId, bo: BoId) -> Result<(), DeviceError> {
        let buffer = self.bo(bo)?;
        if buffer.owner != client {
            return Err(DeviceError::NotOwner);
        }
        if buffer.bindings > 0 {
            return Err(DeviceError::BufferBound);
        }
        self.bos[bo.0 as usize] = None;
        Ok(())
    }

    fn vm_bind(&mut self, client: ClientId, ctx: ContextId, m: Mapping) -> Result<(), DeviceError> {
        let buffer = self.bo(m.bo)?;
        if buffer.owner != client {
            return Err(DeviceError::NotOwner);
        }
        let end = m.offset.checked_add(m.len).ok_or(VmError::OutOfRange)?;
        if end > buffer.bytes.len() as u64 {
            return Err(DeviceError::Vm(VmError::OutOfRange));
        }
        let binding =
            Binding { va: m.va, len: m.len, bo: m.bo.0, offset: m.offset, access: m.access };
        self.owned_context(client, ctx)?.space.bind(binding)?;
        if let Some(Some(b)) = self.bos.get_mut(m.bo.0 as usize) {
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
        let binding = self.owned_context(client, ctx)?.space.unbind(va)?;
        if let Some(Some(b)) = self.bos.get_mut(binding.bo as usize) {
            b.bindings = b.bindings.saturating_sub(1);
        }
        Ok(())
    }

    fn submit(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        kind: QueueKind,
        batch: &Batch<'_>,
    ) -> Result<TimelinePoint, DeviceError> {
        let caps = self.caps;
        if !caps.queue_kinds.executes(kind) {
            return Err(DeviceError::QueueUnsupported(kind));
        }
        check_batch(batch.stream.commands(), &caps)?;
        if let Some(&pending) = batch.waits.iter().find(|&&p| !self.timeline.reached(p)) {
            return Err(DeviceError::WouldBlock(pending));
        }
        let context = self.owned_context(client, ctx)?;
        let target = *context.space.resolve(batch.target.va, batch.target.bytes())?;
        let commands = match batch.stream {
            Stream::Commands(commands) => commands,
            Stream::Code { shader, at } => {
                context.space.resolve(at, shader.bytes.len() as u64)?;
                if shader.target != caps.gpu_code_target {
                    return Err(DeviceError::CodeTargetMismatch {
                        declared: shader.target,
                        device: caps.gpu_code_target,
                    });
                }
                return Err(DeviceError::CodeUnsupported);
            }
            // A transfer job is the device's work (RFC-0101 §8c); the mock executes commands.
            Stream::Transfer { .. } | Stream::Render(_) => {
                return Err(DeviceError::CodeUnsupported)
            }
        };
        let offset = target.offset_of(batch.target.va) as usize;
        let bytes = self
            .bos
            .get_mut(target.bo as usize)
            .and_then(Option::as_mut)
            .ok_or(DeviceError::NoSuchBuffer)?
            .bytes
            .get_mut(offset..offset + batch.target.bytes() as usize)
            .ok_or(DeviceError::NoSuchBuffer)?;
        Self::execute(bytes, batch.target.width, batch.target.height, commands);
        self.batches += 1;
        self.timeline.signal(batch.signal);
        Ok(batch.signal)
    }

    fn reached(&self, point: TimelinePoint) -> bool {
        self.timeline.reached(point)
    }

    fn reset_context(&mut self, client: ClientId, ctx: ContextId) -> Result<(), DeviceError> {
        let context = self.owned_context(client, ctx)?;
        context.resets += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::v2::{Access, RenderTarget, Shader};
    use crate::command::buffer::{Command, RgbaColor};
    use crate::core::types::TileRect;
    use nexus_driverkit::gpu::BatchError;

    const WINDOWD: ClientId = ClientId(0x600e3345cc7104bb);
    const APP: ClientId = ClientId(0xdd8c136b3d65f5dc);
    const VA: GpuVa = GpuVa(0x10_0000);

    fn target(va: GpuVa) -> RenderTarget {
        RenderTarget { va, width: 8, height: 8 }
    }

    fn fill() -> Vec<Command> {
        vec![Command::DrawTiles {
            tiles: vec![TileRect { x: 1, y: 1, width: 2, height: 1 }],
            color: RgbaColor::new(1, 2, 3, 255),
        }]
    }

    fn batch<'a>(va: GpuVa, commands: &'a [Command], signal: u64) -> Batch<'a> {
        Batch {
            target: target(va),
            stream: Stream::Commands(commands),
            waits: &[],
            signal: TimelinePoint(signal),
        }
    }

    fn bound_device() -> (CpuMockDevice, ContextId, BoId) {
        let mut dev = CpuMockDevice::phase2();
        let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
        let bo = dev.create_bo(WINDOWD, 8 * 8 * 4, CacheMode::CpuCached).unwrap();
        dev.vm_bind(WINDOWD, ctx, Mapping::whole(bo, 4096, VA, Access::ReadWrite)).unwrap();
        (dev, ctx, bo)
    }

    #[test]
    fn a_batch_lands_in_the_bound_target_and_signals_its_point() {
        let (mut dev, ctx, bo) = bound_device();
        let commands = fill();
        assert!(!dev.reached(TimelinePoint(1)));
        let point = dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)).unwrap();
        assert_eq!(point, TimelinePoint(1));
        assert!(dev.reached(point));
        let bytes = dev.bo_bytes(WINDOWD, bo).unwrap();
        assert_eq!(&bytes[(8 + 1) * 4..(8 + 3) * 4], &[1, 2, 3, 255, 1, 2, 3, 255]);
        assert_eq!(&bytes[0..4], &[0, 0, 0, 0]);
        assert_eq!(dev.bo_mode(bo), Ok(CacheMode::CpuCached));
        assert_eq!(dev.batches(), 1);
        assert_eq!(dev.bo_bytes(APP, bo), Err(DeviceError::NotOwner));
        // The timeline never moves backwards: an older point is a no-op, still returned.
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Transfer, &batch(VA, &commands, 0)),
            Ok(TimelinePoint(0))
        );
        assert!(dev.reached(TimelinePoint(1)));
    }

    #[test]
    fn test_reject_bind_bo_of_another_client() {
        let mut dev = CpuMockDevice::phase2();
        let app = dev.create_context(APP, Priority::Normal).unwrap();
        let bo = dev.create_bo(WINDOWD, 4096, CacheMode::GpuPrivate).unwrap();
        let read = |bo| Mapping::whole(bo, 4096, VA, Access::ReadOnly);
        assert_eq!(dev.vm_bind(APP, app, read(bo)), Err(DeviceError::NotOwner));
        let own = dev.create_bo(APP, 4096, CacheMode::Uncached).unwrap();
        let other = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
        assert_eq!(dev.vm_bind(APP, other, read(own)), Err(DeviceError::NotOwner));
        assert_eq!(dev.vm_unbind(WINDOWD, app, VA), Err(DeviceError::NotOwner));
        assert_eq!(dev.destroy_bo(APP, bo), Err(DeviceError::NotOwner));
        assert_eq!(dev.vm_bind(APP, app, read(own)), Ok(()));
    }

    #[test]
    fn test_reject_mapping_past_the_buffer() {
        let mut dev = CpuMockDevice::phase2();
        let ctx = dev.create_context(WINDOWD, Priority::Realtime).unwrap();
        let bo = dev.create_bo(WINDOWD, 2 * 4096, CacheMode::CpuCached).unwrap();
        let range = |offset, len| Mapping { bo, offset, len, va: VA, access: Access::ReadWrite };
        let past = Err(DeviceError::Vm(VmError::OutOfRange));
        assert_eq!(dev.vm_bind(WINDOWD, ctx, range(4096, 2 * 4096)), past);
        assert_eq!(dev.vm_bind(WINDOWD, ctx, range(2 * 4096, 4096)), past);
        assert_eq!(dev.vm_bind(WINDOWD, ctx, range(u64::MAX - 0xfff, 4096)), past);
        assert_eq!(
            dev.vm_bind(WINDOWD, ctx, range(2048, 4096)),
            Err(DeviceError::Vm(VmError::Unaligned)),
            "a range starts on a page"
        );
        assert_eq!(dev.vm_bind(WINDOWD, ctx, range(4096, 4096)), Ok(()));
    }

    #[test]
    fn test_reject_destroy_bound_bo() {
        let (mut dev, ctx, bo) = bound_device();
        assert_eq!(dev.destroy_bo(WINDOWD, bo), Err(DeviceError::BufferBound));
        assert_eq!(dev.destroy_bo(APP, bo), Err(DeviceError::NotOwner));
        dev.vm_unbind(WINDOWD, ctx, VA).unwrap();
        assert_eq!(dev.destroy_bo(WINDOWD, bo), Ok(()));
        assert_eq!(dev.destroy_bo(WINDOWD, bo), Err(DeviceError::NoSuchBuffer));
        let whole = Mapping::whole(bo, 4096, VA, Access::ReadWrite);
        assert_eq!(dev.vm_bind(WINDOWD, ctx, whole), Err(DeviceError::NoSuchBuffer));
    }

    #[test]
    fn two_contexts_reach_their_own_memory_through_the_same_address() {
        let mut dev = CpuMockDevice::phase2();
        let commands = fill();
        let mut own = Vec::new();
        for client in [WINDOWD, APP] {
            let ctx = dev.create_context(client, Priority::Normal).unwrap();
            let bo = dev.create_bo(client, 2 * 4096, CacheMode::CpuCached).unwrap();
            // The picture is the buffer's second page: a range, not the whole buffer.
            let m = Mapping { bo, offset: 4096, len: 4096, va: VA, access: Access::ReadWrite };
            dev.vm_bind(client, ctx, m).unwrap();
            own.push((client, ctx, bo));
        }
        let (windowd, app) = (own[0], own[1]);
        dev.submit(windowd.0, windowd.1, QueueKind::Render, &batch(VA, &commands, 1)).unwrap();
        let painted = |dev: &CpuMockDevice, (client, _, bo): (ClientId, ContextId, BoId)| {
            dev.bo_bytes(client, bo).unwrap()[4096 + (8 + 1) * 4] == 1
        };
        assert!(painted(&dev, windowd), "the range's page, at the binding's offset");
        assert!(!painted(&dev, app), "the same address in another context is other memory");
        assert_eq!(dev.bo_bytes(windowd.0, windowd.2).unwrap()[(8 + 1) * 4], 0);
        // A context goes with its bindings; the buffers stay their client's.
        dev.destroy_context(windowd.0, windowd.1).unwrap();
        assert!(painted(&dev, windowd));
        assert_eq!(dev.destroy_bo(windowd.0, windowd.2), Ok(()));
        assert_eq!(dev.destroy_bo(app.0, app.2), Err(DeviceError::BufferBound));
    }

    #[test]
    fn test_reject_submit_references_unbound_va() {
        let (mut dev, ctx, _) = bound_device();
        let commands = fill();
        let elsewhere = GpuVa(0x20_0000);
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(elsewhere, &commands, 1)),
            Err(DeviceError::Vm(VmError::Unbound))
        );
        // A target that starts inside the binding but runs past its end is unbound too.
        let late = GpuVa(VA.0 + 4096 - 16);
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(late, &commands, 1)),
            Err(DeviceError::Vm(VmError::Unbound))
        );
        assert!(!dev.reached(TimelinePoint(1)), "nothing refused signals");
    }

    #[test]
    fn test_reject_compute_queue_until_built() {
        let (mut dev, ctx, _) = bound_device();
        let commands = fill();
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Compute, &batch(VA, &commands, 1)),
            Err(DeviceError::QueueUnsupported(QueueKind::Compute))
        );
        assert!(!dev.capabilities().queue_kinds.executes(QueueKind::Compute));
    }

    #[test]
    fn test_reject_oversized_batch() {
        let (mut dev, ctx, _) = bound_device();
        let one = fill();
        let commands: Vec<Command> = core::iter::repeat(one[0].clone()).take(257).collect();
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)),
            Err(DeviceError::Batch(BatchError::TooLarge { commands: 257, max: 256 }))
        );
    }

    #[test]
    fn test_reject_destroyed_context() {
        let (mut dev, ctx, bo) = bound_device();
        let commands = fill();
        assert_eq!(dev.destroy_context(APP, ctx), Err(DeviceError::NotOwner));
        dev.destroy_context(WINDOWD, ctx).unwrap();
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 1)),
            Err(DeviceError::NoSuchContext)
        );
        assert_eq!(
            dev.vm_bind(WINDOWD, ctx, Mapping::whole(bo, 4096, VA, Access::ReadWrite)),
            Err(DeviceError::NoSuchContext)
        );
        assert_eq!(dev.destroy_bo(WINDOWD, bo), Ok(()), "the context's end unbound it");
        assert_eq!(dev.reset_context(WINDOWD, ctx), Err(DeviceError::NoSuchContext));
        let again = dev.create_context(WINDOWD, Priority::High).unwrap();
        assert_eq!(dev.context_state(again), Ok((Priority::High, 0)));
        dev.reset_context(WINDOWD, again).unwrap();
        assert_eq!(dev.context_state(again), Ok((Priority::High, 1)));
    }

    #[test]
    fn test_reject_gpu_code_for_another_target() {
        let (mut dev, ctx, _) = bound_device();
        let code = [0u8; 16];
        let foreign = Shader { target: CodeTarget(1), entry: 0, bytes: &code };
        let own = Shader { target: dev.capabilities().gpu_code_target, entry: 0, bytes: &code };
        let at = GpuVa(VA.0 + 1024);
        let run = |dev: &mut CpuMockDevice, shader| {
            let b = Batch {
                target: target(VA),
                stream: Stream::Code { shader, at },
                waits: &[],
                signal: TimelinePoint(1),
            };
            dev.submit(WINDOWD, ctx, QueueKind::Render, &b)
        };
        assert_eq!(
            run(&mut dev, foreign),
            Err(DeviceError::CodeTargetMismatch {
                declared: CodeTarget(1),
                device: dev.capabilities().gpu_code_target
            })
        );
        assert_eq!(run(&mut dev, own), Err(DeviceError::CodeUnsupported), "declared, not run");
        let unbound = Batch {
            target: target(VA),
            stream: Stream::Code { shader: own, at: GpuVa(0x30_0000) },
            waits: &[],
            signal: TimelinePoint(1),
        };
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Render, &unbound),
            Err(DeviceError::Vm(VmError::Unbound))
        );
    }

    #[test]
    fn a_wait_the_mock_cannot_defer_is_refused_not_ignored() {
        let (mut dev, ctx, _) = bound_device();
        let commands = fill();
        let waits = [TimelinePoint(7)];
        let b = Batch {
            target: target(VA),
            stream: Stream::Commands(&commands),
            waits: &waits,
            signal: TimelinePoint(8),
        };
        assert_eq!(
            dev.submit(WINDOWD, ctx, QueueKind::Render, &b),
            Err(DeviceError::WouldBlock(TimelinePoint(7)))
        );
        dev.submit(WINDOWD, ctx, QueueKind::Render, &batch(VA, &commands, 7)).unwrap();
        assert_eq!(dev.submit(WINDOWD, ctx, QueueKind::Render, &b), Ok(TimelinePoint(8)));
        let full = CpuMockDevice::new(Capabilities { max_contexts: 1, ..dev.capabilities() });
        let mut full = full;
        full.create_context(APP, Priority::Low).unwrap();
        assert_eq!(full.create_context(APP, Priority::Low), Err(DeviceError::Exhausted));
    }
}
