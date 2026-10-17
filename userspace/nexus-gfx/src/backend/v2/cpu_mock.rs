// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The mock device — `GfxDevice` (RFC-0101) on the host, the oracle every later
//! package is proven against: contexts per client with their own [`VmSpace`], buffers as
//! bytes with their cache mode (destroyed by their client once no context binds them), ranges
//! of them bound, one batch per submit checked against the context's space and the device's
//! bound, points signalled on the one timeline. It executes the tree's
//! command vocabulary only as far as proving the data flow (`DrawTiles` lands in the bound
//! target); pixels through v2 are Phase 3's. GPU code is refused by name: the mock executes
//! none, and says so. A context takes its pipeline library once (§7b), validated and placed as
//! the board places it — within the same room, no program byte kept.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests (`cpu_mock/tests.rs`) — the reject matrix of RFC-0101 §9
//! INVARIANTS: no batch runs with a reference outside its context's space; a context or
//!   buffer answers only to its owner; a range binds inside its buffer; a bound buffer is never
//!   destroyed; the timeline never moves backwards; a context holds one library.

use alloc::vec;
use alloc::vec::Vec;

use nexus_driverkit::gpu::{check_batch, Binding, VmSpace};

use super::{
    Batch, BoId, CacheMode, Capabilities, ClientId, CodeTarget, ContextId, DeviceError, GfxDevice,
    GpuVa, LibraryTaken, Mapping, Priority, QueueKind, QueueKinds, Stream, TimelinePoint, VmError,
};
use crate::backend::gpu::pipelines::{
    Heaps, PipelineTable, LIBRARY_CODE_BYTES, LIBRARY_SETUP_BYTES,
};
use crate::command::buffer::Command;
use crate::sync::timeline::Timeline;

/// Bindings a mock context holds.
pub const MOCK_BINDINGS: usize = 32;
/// The mock's page.
pub const MOCK_PAGE: u32 = 4096;
/// Programs and setup programs each start a 64-byte line, as on the board.
const LINE: u32 = 64;

struct Context {
    owner: ClientId,
    priority: Priority,
    space: VmSpace<MOCK_BINDINGS>,
    resets: u32,
    /// The device lost it (a fault): nothing new runs in it.
    lost: bool,
    /// The pipelines its library gave it (§7b); empty until it takes one.
    pipelines: PipelineTable,
}

/// A context's library places in the mock: what each heap took, bounded as the board's places
/// are — the mock keeps no program bytes (it executes none).
struct MockHeaps {
    code: u32,
    setup: u32,
}

impl MockHeaps {
    fn place(next: &mut u32, room: u32, bytes: &[u8]) -> Option<u32> {
        let at = next.next_multiple_of(LINE);
        let end = at.checked_add(u32::try_from(bytes.len()).ok()?)?;
        if end > room {
            return None;
        }
        *next = end;
        Some(at)
    }
}

impl Heaps for MockHeaps {
    fn code(&mut self, bytes: &[u8]) -> Option<u32> {
        Self::place(&mut self.code, LIBRARY_CODE_BYTES, bytes)
    }

    fn setup(&mut self, bytes: &[u8]) -> Option<u32> {
        Self::place(&mut self.setup, LIBRARY_SETUP_BYTES, bytes)
    }
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
            gpu_code_features: 0,
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

    /// The device's report of a fault in `ctx` (a hardware device learns it from its
    /// firmware): the context is lost — §6.
    pub fn lose_context(&mut self, ctx: ContextId) -> Result<(), DeviceError> {
        let context = self
            .contexts
            .get_mut(ctx.0 as usize)
            .and_then(Option::as_mut)
            .ok_or(DeviceError::NoSuchContext)?;
        context.lost = true;
        Ok(())
    }

    /// `client`'s context, refused when the device lost it.
    fn running_context(
        &mut self,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<&mut Context, DeviceError> {
        let context = self.owned_context(client, ctx)?;
        if context.lost {
            return Err(DeviceError::ContextLost);
        }
        Ok(context)
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
            lost: false,
            pipelines: PipelineTable::default(),
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
        self.running_context(client, ctx)?.space.bind(binding)?;
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
        let context = self.running_context(client, ctx)?;
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
        let context = self.running_context(client, ctx)?;
        context.resets += 1;
        Ok(())
    }

    fn take_library(
        &mut self,
        client: ClientId,
        ctx: ContextId,
        library: &[u8],
    ) -> Result<LibraryTaken, DeviceError> {
        let caps = self.caps;
        let context = self.running_context(client, ctx)?;
        let mut heaps = MockHeaps { code: 0, setup: 0 };
        let loaded = context.pipelines.load(
            library,
            caps.gpu_code_target.0,
            caps.gpu_code_features,
            &mut heaps,
        )?;
        Ok(LibraryTaken::from(&loaded))
    }
}

#[cfg(test)]
mod tests;
