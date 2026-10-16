// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The device's contexts (RFC-0101 §1/§6, TASK-0215 5c-3 P2b): each its client's, each its own
//! GPU address space ([`Space`]: a page catalogue, the firmware memory context naming it), its
//! bindings, its transfer queue (made on its first transfer), its render objects (made when its
//! client sets them up), its pipelines. A context is numbered by its slot and the slot's
//! generation — a destroyed context's number stays refused when the slot serves the next — and
//! its points carry its number and queue above the sequence, so `reached` needs no context. A
//! context goes in the open kernel driver's order: its last points waited, its render objects
//! and its transfer queue cleaned up by the firmware, its bindings dropped, its space given
//! back. When the firmware will not let go of an object, nothing of the context is freed: the
//! slot keeps it in quarantine (the firmware or the GPU may still reach any of its memory).
//! The slots are allocated once, each slot's table storage on its first use and kept (gpud's
//! heap never frees).

use alloc::vec::Vec;

use nexus_driverkit::gpu::{Priority, TimelinePoint, VmSpace};
use nexus_gfx::backend::gpu::fwif;
use nexus_gfx::backend::gpu::mmu::TableStorage;
use nexus_gfx::backend::gpu::pipelines::PipelineTable;
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, DeviceError};

use super::bos::Bos;
use super::device::Gpu;
use super::job::Queue;
use super::render::Render;
use super::space::Space;

/// Contexts the device holds at once (a render context with its transfer queue holds about 60
/// pages of the firmware's 1024).
pub(super) const MAX_CONTEXTS: usize = 8;
/// Bindings one context holds (the render bundle binds 37 ranges).
pub(super) const MAX_BINDINGS: usize = 64;
/// A point: its context's number in bits 40..63, its queue in 32..39, the sequence below.
const QUEUE_SHIFT: u32 = 32;
const CONTEXT_SHIFT: u32 = 40;
pub(super) const TRANSFER: u64 = 0;
pub(super) const RENDER: u64 = 1;
/// The bound of waiting for a context's last points before its cleanup.
const QUIESCE_NS: u64 = 2_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Free,
    Live,
    /// The firmware kept an object of it: nothing is freed, the slot serves no other.
    Quarantined,
}

pub(super) struct Slot {
    gen: u16,
    state: State,
    pub owner: ClientId,
    pub priority: Priority,
    pub space: VmSpace<MAX_BINDINGS>,
    pub mmu: Option<Space>,
    pub transfer: Option<Queue>,
    pub render: Option<Render>,
    pub pipelines: PipelineTable,
    /// The page tables' slot storage, kept from one context to the next.
    storage: Option<TableStorage>,
}

impl Slot {
    /// The context's number (the slot in the low byte, the generation above).
    pub(super) fn id(&self, index: usize) -> ContextId {
        ContextId((u32::from(self.gen) << 8) | index as u32)
    }

    /// The firmware's priority for the context's queues.
    pub(super) fn fw_priority(&self) -> u32 {
        match self.priority {
            Priority::Realtime | Priority::High => fwif::priority::HIGH,
            Priority::Normal => fwif::priority::MEDIUM,
            Priority::Low => fwif::priority::LOW,
        }
    }

    /// The point of `seq` on `queue` of the context numbered `id`.
    pub(super) fn point(id: ContextId, queue: u64, seq: u32) -> TimelinePoint {
        TimelinePoint((u64::from(id.0) << CONTEXT_SHIFT) | (queue << QUEUE_SHIFT) | u64::from(seq))
    }

    /// Whether `queue` reached `seq` (a queue that was never made has nothing outstanding).
    fn queue_reached(&self, gpu: &Gpu, queue: u64, seq: u32) -> bool {
        let reached = |word: u32| word.wrapping_sub(seq) < 0x8000_0000;
        match queue {
            TRANSFER => self.transfer.as_ref().is_none_or(|q| reached(q.word(gpu))),
            _ => self.render.as_ref().is_none_or(|r| reached(r.frag_word(gpu))),
        }
    }

    /// Whether every point the context submitted is reached.
    fn idle(&self, gpu: &Gpu) -> bool {
        self.transfer.as_ref().is_none_or(|q| q.word(gpu).wrapping_sub(q.seq) < 0x8000_0000)
            && self.render.as_ref().is_none_or(|r| {
                let ((gtl, gseq), (ftl, fseq)) = r.progress();
                gpu.mem.read_u32(&gtl, 0).wrapping_sub(gseq) < 0x8000_0000
                    && gpu.mem.read_u32(&ftl, 0).wrapping_sub(fseq) < 0x8000_0000
            })
    }
}

/// What the device holds, for the lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Holdings {
    pub live: usize,
    pub quarantined: usize,
    pub bos: usize,
    pub backing_free: usize,
}

pub(super) struct Contexts {
    slots: Vec<Slot>,
}

impl Contexts {
    pub(super) fn new() -> Self {
        let mut slots = Vec::with_capacity(MAX_CONTEXTS);
        for _ in 0..MAX_CONTEXTS {
            slots.push(Slot {
                gen: 0,
                state: State::Free,
                owner: ClientId(0),
                priority: Priority::Low,
                space: VmSpace::new(40, nexus_gfx::backend::gpu::mmu::PAGE as u32),
                mmu: None,
                transfer: None,
                render: None,
                pipelines: PipelineTable::default(),
                storage: None,
            });
        }
        Self { slots }
    }

    /// A new context of `client`'s: a free slot, its space made.
    pub(super) fn create(
        &mut self,
        gpu: &mut Gpu,
        client: ClientId,
        priority: Priority,
    ) -> Result<ContextId, DeviceError> {
        let index =
            self.slots.iter().position(|s| s.state == State::Free).ok_or(DeviceError::Exhausted)?;
        let slot = &mut self.slots[index];
        let storage = slot.storage.take().unwrap_or_default();
        match Space::create(gpu, storage) {
            Ok(space) => slot.mmu = Some(space),
            Err((what, storage)) => {
                slot.storage = Some(storage);
                super::emit(&alloc::format!("gpud: FAIL gpu context ({what})"));
                return Err(DeviceError::Exhausted);
            }
        }
        // Generation 0 is never a live context's: a zero id stays refused.
        slot.gen = slot.gen.wrapping_add(1).max(1);
        slot.state = State::Live;
        slot.owner = client;
        slot.priority = priority;
        slot.space.clear();
        Ok(slot.id(index))
    }

    fn index(ctx: ContextId) -> (usize, u16) {
        ((ctx.0 & 0xff) as usize, (ctx.0 >> 8) as u16)
    }

    /// `client`'s live context `ctx`.
    pub(super) fn live(
        &mut self,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<&mut Slot, DeviceError> {
        let (index, gen) = Self::index(ctx);
        let slot = self.slots.get_mut(index).ok_or(DeviceError::NoSuchContext)?;
        if slot.state != State::Live || slot.gen != gen {
            return Err(DeviceError::NoSuchContext);
        }
        if slot.owner != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(slot)
    }

    pub(super) fn get(&self, client: ClientId, ctx: ContextId) -> Result<&Slot, DeviceError> {
        let (index, gen) = Self::index(ctx);
        let slot = self.slots.get(index).ok_or(DeviceError::NoSuchContext)?;
        if slot.state != State::Live || slot.gen != gen {
            return Err(DeviceError::NoSuchContext);
        }
        if slot.owner != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(slot)
    }

    /// Whether `point` is reached: a point of a context that is gone counts as reached (its work
    /// was waited for when it went, or the context is in quarantine and its client gone).
    pub(super) fn reached(&self, gpu: &Gpu, point: TimelinePoint) -> bool {
        let id = ContextId((point.0 >> CONTEXT_SHIFT) as u32);
        let queue = (point.0 >> QUEUE_SHIFT) & 0xff;
        let (index, gen) = Self::index(id);
        match self.slots.get(index) {
            Some(s) if s.state == State::Live && s.gen == gen => {
                s.queue_reached(gpu, queue, point.0 as u32)
            }
            _ => true,
        }
    }

    /// Destroys `client`'s context: its last points waited, its render objects and transfer
    /// queue cleaned up by the firmware, its bindings dropped (each buffer's count), its space
    /// given back. A refusal by the firmware quarantines the slot and is returned as `Lost`.
    pub(super) fn destroy(
        &mut self,
        gpu: &mut Gpu,
        bos: &mut Bos,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<(), DeviceError> {
        let slot = self.live(client, ctx)?;
        let deadline = nexus_abi::nsec().unwrap_or(0) + QUIESCE_NS;
        let idle = gpu.wait_until(deadline, |g| slot.idle(g));
        let mut refused = if idle { None } else { Some("its last points were not reached") };
        if refused.is_none() {
            if let Some(render) = slot.render.take() {
                refused = render.destroy(gpu).err();
            }
        }
        if refused.is_none() {
            if let Some(queue) = slot.transfer.take() {
                refused = queue.destroy(gpu).err();
            }
        }
        if let Some(what) = refused {
            slot.state = State::Quarantined;
            super::emit(&alloc::format!(
                "gpud: FAIL gpu context cleanup (ctx={:#x} {what}: quarantined)",
                ctx.0
            ));
            return Err(DeviceError::Lost);
        }
        for b in slot.space.bindings() {
            if let Some(bo) = bos.any_mut(BoId(b.bo)) {
                bo.bindings = bo.bindings.saturating_sub(1);
            }
        }
        slot.space.clear();
        // The programs lived in the context's buffers: its pipelines go with them.
        slot.pipelines = PipelineTable::default();
        if let Some(space) = slot.mmu.take() {
            let (storage, flushed) = space.destroy(gpu);
            slot.storage = Some(storage);
            if !flushed {
                slot.state = State::Quarantined;
                super::emit(&alloc::format!(
                    "gpud: FAIL gpu context cleanup (ctx={:#x} the space's flush got no return: quarantined)",
                    ctx.0
                ));
                return Err(DeviceError::Lost);
            }
        }
        slot.state = State::Free;
        Ok(())
    }

    pub(super) fn holdings(&self, gpu: &Gpu, bos: &Bos) -> Holdings {
        let count = |st: State| self.slots.iter().filter(|s| s.state == st).count();
        Holdings {
            live: count(State::Live),
            quarantined: count(State::Quarantined),
            bos: bos.live(),
            backing_free: gpu.mem.free_pages(),
        }
    }
}
