// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The device's contexts (RFC-0101 §1/§6, TASK-0215 5c-3 P2b/P2c): each its client's, each its
//! own GPU address space ([`Space`]: a page catalogue, the firmware memory context naming it),
//! its bindings, its transfer queue (made on its first transfer), its render objects (made when
//! its client sets them up), its pipelines. A context is numbered by its slot and the slot's
//! generation — a destroyed context's number stays refused when the slot serves the next — and
//! its points carry its number and queue above the sequence, so `reached` needs no context; its
//! free lists are numbered from its number, so the firmware's messages name one list of one
//! context. A context the firmware's reset is held against is lost ([`super::recovery`]):
//! nothing new runs in it (`ContextLost`), its points count as reached, its client destroys it.
//! A context goes in the open kernel driver's order: its last points waited, its render objects
//! and its transfer queue let go by the firmware (its messages served between busy answers),
//! their memory back, its bindings dropped, its space given back. When the firmware will not
//! let go of an object, nothing of the context is freed: the slot keeps it in quarantine (the
//! firmware or the GPU may still reach any of its memory). The slots are allocated once, each
//! slot's table storage on its first use and kept (gpud's heap never frees).

use alloc::vec::Vec;

use nexus_driverkit::gpu::{GpuVa, Priority, TimelinePoint, VmSpace};
use nexus_gfx::backend::gpu::fwif;
use nexus_gfx::backend::gpu::mmu::TableStorage;
use nexus_gfx::backend::gpu::pipelines::PipelineTable;
use nexus_gfx::backend::v2::{BoId, ClientId, ContextId, DeviceError};

use super::bos::Bos;
use super::device::Gpu;
use super::job::Queue;
use super::recovery;
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
    /// The firmware's reset was held against it: nothing new runs in it, its points count as
    /// reached; its memory stays its client's until the client destroys it.
    Lost,
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

    /// The ids of the context's free lists (local, global), as the firmware names them in its
    /// messages: the context's number above the list's bit — never zero (no context's number
    /// is), one context's lists apart from every other's.
    pub(super) fn freelist_ids(id: ContextId) -> [u32; 2] {
        [id.0 << 1, (id.0 << 1) | 1]
    }

    /// Whether `queue` reached `seq` (a queue that was never made has nothing outstanding).
    fn queue_reached(&self, gpu: &Gpu, queue: u64, seq: u32) -> bool {
        let reached = |word: u32| word.wrapping_sub(seq) < 0x8000_0000;
        match queue {
            TRANSFER => self.transfer.as_ref().is_none_or(|q| reached(q.word(gpu))),
            _ => self.render.as_ref().is_none_or(|r| reached(r.frag_word(gpu))),
        }
    }

    /// Whether every point the context submitted is reached — as the firmware took them, also
    /// for a lost context (its words tell whether the firmware still works on it).
    fn idle(&self, gpu: &Gpu) -> bool {
        self.transfer.as_ref().is_none_or(|q| q.word(gpu).wrapping_sub(q.seq) < 0x8000_0000)
            && self.render.as_ref().is_none_or(|r| {
                let ((gtl, gseq), (ftl, fseq)) = r.progress();
                gpu.mem.read_u32(&gtl, 0).wrapping_sub(gseq) < 0x8000_0000
                    && gpu.mem.read_u32(&ftl, 0).wrapping_sub(fseq) < 0x8000_0000
            })
    }

    /// Free list `list`'s entries written into the client's list buffer — each parameter
    /// page's number, the list filled from its end — the lines cleaned (CPU → device): when the
    /// render objects are made, and again when the firmware's recovery asks for the list.
    pub(super) fn write_entries(&self, bos: &mut Bos, list: usize, block: usize) -> bool {
        let Some(render) = self.render.as_ref() else { return false };
        let (space, client) = (&self.space, self.owner);
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
    }
}

/// What the device holds, for the lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Holdings {
    pub live: usize,
    pub lost: usize,
    pub quarantined: usize,
    pub bos: usize,
    pub backing_free: usize,
}

/// What the device did about the firmware's recovery (TASK-0215 5c-3 P2c), for the lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Recovered {
    /// Reset notifications served.
    pub resets: u32,
    /// Contexts lost (each once).
    pub lost: u32,
    /// Resets that held nothing against the context they named.
    pub innocent: u32,
    /// Resets that named no context the device holds.
    pub unattributed: u32,
    /// Reconstruction requests answered; the lists rebuilt; lists named that no context holds.
    pub reconstructions: u32,
    pub rebuilt: u32,
    pub unknown_lists: u32,
    /// Growth requests answered (with no pages: the device never grows a list).
    pub grows: u32,
    /// The last loss: the context, the catalogue its reset named (zero when named by its id),
    /// the address that faulted.
    pub last: Option<(ContextId, u64, u64)>,
}

pub(super) struct Contexts {
    slots: Vec<Slot>,
    pub recovered: Recovered,
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
        Self { slots, recovered: Recovered::default() }
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

    /// The slot of `client`'s context `ctx`, live or lost (a lost context's memory is still its
    /// client's to release, and the client's to destroy).
    pub(super) fn find(&self, client: ClientId, ctx: ContextId) -> Result<usize, DeviceError> {
        let (index, gen) = Self::index(ctx);
        let slot = self.slots.get(index).ok_or(DeviceError::NoSuchContext)?;
        if !matches!(slot.state, State::Live | State::Lost) || slot.gen != gen {
            return Err(DeviceError::NoSuchContext);
        }
        if slot.owner != client {
            return Err(DeviceError::NotOwner);
        }
        Ok(index)
    }

    pub(super) fn get(&self, client: ClientId, ctx: ContextId) -> Result<&Slot, DeviceError> {
        Ok(&self.slots[self.find(client, ctx)?])
    }

    pub(super) fn get_mut(
        &mut self,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<&mut Slot, DeviceError> {
        let index = self.find(client, ctx)?;
        Ok(&mut self.slots[index])
    }

    /// `client`'s context `ctx` for new work: a lost one is refused by name.
    pub(super) fn running(
        &mut self,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<&mut Slot, DeviceError> {
        let slot = self.get_mut(client, ctx)?;
        if slot.state == State::Lost {
            return Err(DeviceError::ContextLost);
        }
        Ok(slot)
    }

    pub(super) fn slot(&self, index: usize) -> &Slot {
        &self.slots[index]
    }

    /// Whether `point` is reached: a point of a context that is gone or lost counts as reached
    /// (a gone context's work was waited for when it went, or its slot is in quarantine and its
    /// client gone; nothing waits for a lost context's work).
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

    /// Whether `client`'s context `ctx` is lost.
    pub(super) fn lost(&self, client: ClientId, ctx: ContextId) -> bool {
        self.get(client, ctx).is_ok_and(|s| s.state == State::Lost)
    }

    /// The context whose page catalogue is `catalogue` lost — one that is lost already or in
    /// quarantine stays so; its number. `None`: no context of the device has that catalogue.
    pub(super) fn lose_catalogue(&mut self, catalogue: u64) -> Option<ContextId> {
        // The firmware names the catalogue's page; its low bits carry flags (the valid bit).
        let page = catalogue & !0xfff;
        let index = self.slots.iter().position(|s| {
            s.state != State::Free && s.mmu.as_ref().is_some_and(|m| m.root_phys() == page)
        })?;
        Some(self.lose_at(index))
    }

    /// The context the firmware numbers `id` lost (its common contexts carry its number).
    pub(super) fn lose(&mut self, id: u32) -> Option<ContextId> {
        let (index, gen) = Self::index(ContextId(id));
        let slot = self.slots.get(index)?;
        (slot.state != State::Free && slot.gen == gen).then(|| self.lose_at(index))
    }

    fn lose_at(&mut self, index: usize) -> ContextId {
        let slot = &mut self.slots[index];
        if slot.state == State::Live {
            slot.state = State::Lost;
            self.recovered.lost = self.recovered.lost.saturating_add(1);
        }
        slot.id(index)
    }

    /// The context and list the firmware names by free-list `id` ([`Slot::freelist_ids`]); a
    /// context whose render objects are gone holds none.
    pub(super) fn list_owner(&self, id: u32) -> Option<(usize, usize)> {
        let (index, gen) = Self::index(ContextId(id >> 1));
        let slot = self.slots.get(index)?;
        if slot.state == State::Free || slot.gen != gen {
            return None;
        }
        Some((index, slot.render.as_ref()?.list_of(id)?))
    }

    /// The firmware lets go of `objects` (`(kind, fw_addr)`) one after the other, its messages
    /// served between busy answers; the first refusal is returned.
    fn let_go(
        &mut self,
        gpu: &mut Gpu,
        bos: &mut Bos,
        objects: &[(u32, u32)],
    ) -> Result<(), &'static str> {
        for &(kind, fw_addr) in objects {
            gpu.cleanup(kind, fw_addr, &mut |g| recovery::serve(g, self, bos))?;
        }
        Ok(())
    }

    /// Slot `index`'s render objects let go by the firmware, then their memory back (the render
    /// dropped, its parameter pages with it). On a refusal the render stays in the slot.
    pub(super) fn drop_render(
        &mut self,
        index: usize,
        gpu: &mut Gpu,
        bos: &mut Bos,
    ) -> Result<(), &'static str> {
        let Some(objects) = self.slots[index].render.as_ref().map(Render::fw_objects) else {
            return Ok(());
        };
        self.let_go(gpu, bos, &objects)?;
        let slot = &mut self.slots[index];
        if !slot.render.as_ref().is_some_and(|r| r.release(gpu)) {
            return Err("the flush after the cleanup got no return");
        }
        slot.render = None;
        Ok(())
    }

    /// Slot `index`'s transfer queue let go by the firmware, then its memory back.
    fn drop_transfer(
        &mut self,
        index: usize,
        gpu: &mut Gpu,
        bos: &mut Bos,
    ) -> Result<(), &'static str> {
        let Some(object) = self.slots[index].transfer.as_ref().map(Queue::fw_object) else {
            return Ok(());
        };
        self.let_go(gpu, bos, &[object])?;
        let slot = &mut self.slots[index];
        if !slot.transfer.as_ref().is_some_and(|q| q.release(gpu)) {
            return Err("the flush after the cleanup got no return");
        }
        slot.transfer = None;
        Ok(())
    }

    fn quarantine(&mut self, index: usize, ctx: ContextId, what: &str, idle: bool) -> DeviceError {
        self.slots[index].state = State::Quarantined;
        super::emit(&alloc::format!(
            "gpud: FAIL gpu context cleanup (ctx={:#x} idle={idle} {what}: quarantined)",
            ctx.0
        ));
        DeviceError::Lost
    }

    /// Destroys `client`'s context, live or lost: its last points waited (as far as the firmware
    /// takes a lost context's), its render objects and transfer queue let go by the firmware and
    /// their memory back, its bindings dropped (each buffer's count), its space given back — the
    /// firmware's messages served throughout. A refusal by the firmware quarantines the slot and
    /// is returned as `Lost`.
    pub(super) fn destroy(
        &mut self,
        gpu: &mut Gpu,
        bos: &mut Bos,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<(), DeviceError> {
        let index = self.find(client, ctx)?;
        let deadline = nexus_abi::nsec().unwrap_or(0) + QUIESCE_NS;
        let idle = gpu.wait_until(deadline, |g| {
            recovery::serve(g, self, bos);
            self.slots[index].idle(g)
        });
        let gone =
            self.drop_render(index, gpu, bos).and_then(|()| self.drop_transfer(index, gpu, bos));
        if let Err(what) = gone {
            return Err(self.quarantine(index, ctx, what, idle));
        }
        let slot = &mut self.slots[index];
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
                return Err(self.quarantine(index, ctx, "the space's flush got no return", idle));
            }
        }
        slot.state = State::Free;
        Ok(())
    }

    pub(super) fn holdings(&self, gpu: &Gpu, bos: &Bos) -> Holdings {
        let count = |st: State| self.slots.iter().filter(|s| s.state == st).count();
        Holdings {
            live: count(State::Live),
            lost: count(State::Lost),
            quarantined: count(State::Quarantined),
            bos: bos.live(),
            backing_free: gpu.mem.free_pages(),
        }
    }
}
