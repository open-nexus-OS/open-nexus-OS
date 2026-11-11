// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Capability table implementation (Endpoint, VMO, IRQ) and rights. A slot holds a
//! `Held` — one held capability, counted at its object's header (`held.rs`, RFC-0102); the
//! descriptor a check reads (`Capability`: kind and rights) stays a plain value.
//! OWNERS: @kernel-cap-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: host — `cap/tests_prop.rs` (slot discipline, growth, the counts against a
//!   census, the `test_reject_*` of the doors); QEMU selftests + boot markers
//! PUBLIC API: CapTable, Capability{Kind,rights}, Rights, Held, HeldCounts
//! DEPENDS_ON: kheap (the table's growth, a header's admission)
//! INVARIANTS: Rights enforced at syscall boundaries; slots validated; no cross-layer leaks;
//!   a slot is filled only with a `Held`, so a counted kind enters a table only through its
//!   object's header or a `dup`, and what a slot holds never changes its kind or rights
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::vec::Vec;
use bitflags::bitflags;
use core::fmt;
use core::marker::PhantomData;

mod held;
pub use held::{Held, HeldCounts};

/// The id of a kernel endpoint — what an endpoint capability names.
pub type EndpointId = u32;

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    /// Rights associated with a capability handle.
    pub struct Rights: u32 {
        const SEND = 1 << 0;
        const RECV = 1 << 1;
        const MAP = 1 << 2;
        const MANAGE = 1 << 3;
        /// Wait on a timeline fence WITHOUT being able to advance it (ADR-0062): the boot-stage
        /// fence is handed to every child with this right alone, so a child can block for a stage
        /// but never release one for the whole fleet.
        const WAIT = 1 << 4;
    }
}

/// Capability handle types exposed to tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityKind {
    /// Kernel message endpoint.
    Endpoint(EndpointId),
    /// Endpoint-factory authority (Phase-2 hardening): holder may create new endpoints.
    EndpointFactory,
    /// A page-backed virtual memory object (`mm::vmo`, TASK-0286 P3a): `id`
    /// names the object, `len` its bytes. Frames are the table's, never the
    /// capability's — a clone or transfer copies the name, not the memory.
    Vmo { id: u32, len: usize },
    /// RFC-0080: a READ-ONLY alias of a VMO's physical pages. Derived from a
    /// `Vmo` via `vmo_share_readonly`; `sys_map` force-strips WRITE|EXECUTE so
    /// the holder can only map it read-only, and `vmo_write` rejects it. Used
    /// to share the glyph atlas across app-hosts without letting any of them
    /// corrupt the pages the others read.
    VmoRo { id: u32, len: usize },
    /// Device MMIO window (physical base + length) plus the device's PLIC line (`irq`, 0 =
    /// none), mapped into userspace only via a dedicated syscall that enforces USER|RW and
    /// never EXEC. The window and the line come from the device tree (RFC-0098 C3): init
    /// mints the capability from the node's `reg` + `interrupts`, the driver queries it
    /// (`cap_query`) — no driver derives an interrupt number from an address.
    ///
    /// Rationale: userspace drivers require MMIO access; this capability keeps the exposed
    /// range fixed and bounded (no ambient physical mappings). `dma_noncoherent`: the
    /// device does not snoop the CPU caches — init read it from the tree like the line
    /// (RFC-0098 C4), the driver maintains its buffers (`DmaBuffer`). `dev` names the
    /// kernel's record of the device (`mm::devices`, TASK-0246 P1), which holds its DMA
    /// reach; the other fields are that record's immutable facts, carried so a map, a
    /// bind or a query needs no table lookup.
    DeviceMmio { base: usize, len: usize, irq: u32, dma_noncoherent: bool, dev: u16 },
    /// Interrupt binding.
    Irq(u32),
    /// Kernel timer capability bound to a per-hart timer table entry.
    ///
    /// The payload is the kernel-local `TimerId` (opaque to userspace).
    Timer(u32),
    /// Waitset capability bound to a per-hart waitset table entry (RFC-0033).
    ///
    /// A waitset lets a task block on multiple endpoints and wake on the first ready.
    /// The payload is the kernel-local `WaitsetId` (opaque to userspace), stored as a
    /// raw `u32` so this enum stays independent of the `waitset` module (mirrors `Timer`).
    Waitset(u32),
    /// Timeline fence capability bound to a per-hart fence table entry (RFC-0033).
    ///
    /// A fence holds a monotonic `u64` value; `fence_wait(target)` blocks until
    /// `value >= target`. The payload is the kernel-local `FenceId` as a raw `u32`.
    Fence(u32),
    /// Scheduler affinity control (TASK-0042): holder may set CPU affinity for other tasks.
    ///
    /// Rationale: Only privileged services (e.g., execd, policyd) should be able to set
    /// CPU affinity for arbitrary tasks. Tasks can always set their own affinity without
    /// this capability (within recipe limits).
    SchedSetAffinity,
}

/// Capability descriptor: what a slot holds, described — kind and rights, a plain value that
/// holds nothing and installs nowhere.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    pub kind: CapabilityKind,
    pub rights: Rights,
}

impl fmt::Debug for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Capability")
            .field("kind", &self.kind)
            .field("rights", &self.rights.bits())
            .finish()
    }
}

/// Errors produced when manipulating the capability table.
#[must_use = "capability errors must be handled explicitly"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// Provided slot is invalid.
    InvalidSlot,
    /// Insufficient rights for the requested operation.
    PermissionDenied,
    /// No free slot, and the kernel heap above its reserve refuses the table's growth
    /// (ADR-0093).
    NoSpace,
}

/// Type tag for endpoint capabilities.
pub enum EndpointCapTag {}

/// Minimal typed wrapper proving a slot holds an endpoint capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointCapRef {
    slot: usize,
    endpoint: EndpointId,
    _tag: PhantomData<EndpointCapTag>,
}

impl EndpointCapRef {
    #[inline]
    pub const fn slot(self) -> usize {
        self.slot
    }

    #[inline]
    pub const fn endpoint(self) -> EndpointId {
        self.endpoint
    }
}

/// Per-task capability table.
#[derive(Default)]
pub struct CapTable {
    slots: Vec<Option<Held>>,
    /// The task this table belongs to: an endpoint's owner holding a send capability to it is
    /// not its own peer (`Held::enter_table`). `None`: a table of nobody (a dead task's).
    holder: Option<u32>,
    // Pre-SMP contract: capability tables are task-local and never shared directly.
    _not_send_sync: PhantomData<*mut ()>,
}
static_assertions::assert_not_impl_any!(CapTable: Send, Sync);

impl CapTable {
    /// Creates an empty table of nobody, sized for `slots` entries.
    pub fn with_capacity(slots: usize) -> Self {
        let mut table: Vec<Option<Held>> = Vec::with_capacity(slots);
        for _ in 0..slots {
            // Avoids potential libc/memset intrinsics on no_std target
            table.push(None);
        }
        Self { slots: table, holder: None, _not_send_sync: PhantomData }
    }

    /// Default slot count for task capability tables. Raised 128 → 256
    /// (2026-07-22, TASK-0147 recorded follow-up): init's table ran AT the
    /// 128 ceiling by the end of wiring — every late `cap_clone` (allocates
    /// caller-side) failed NoSpace, silently breaking runtime `@mint-pair`
    /// and app-launch legs. Since TASK-0352 P7 (ADR-0093) these are the FIRST
    /// 256: a full table doubles, above the kernel heap's reserve (ADR-0091) —
    /// `NoSpace` is the heap's refusal, never a constant's.
    const DEFAULT_CAP_SLOTS: usize = 256;

    /// A table of nobody with the first slots (the tests' seam; a task's table is `new_for`).
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAP_SLOTS)
    }

    /// The table of task `holder`, with the first slots; it grows with the task's use
    /// (ADR-0093).
    pub fn new_for(holder: u32) -> Self {
        let mut table = Self::new();
        table.holder = Some(holder);
        table
    }

    fn enter(&self, mut held: Held) -> Held {
        held.enter_table(self.holder);
        held
    }

    /// Inserts or overwrites a slot; what the slot held is dropped. Refused, `held` is
    /// dropped — counted off.
    pub fn set(&mut self, slot: usize, held: Held) -> Result<(), CapError> {
        if slot >= self.slots.len() {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = writeln!(u, "CAP-E: invalid slot {} (len={})", slot, self.slots.len());
            }
            return Err(CapError::InvalidSlot);
        }
        let held = self.enter(held);
        self.slots[slot] = Some(held);
        Ok(())
    }

    /// Inserts a capability only if the slot is empty.
    pub fn set_if_empty(&mut self, slot: usize, held: Held) -> Result<(), CapError> {
        match self.slots.get(slot) {
            None => Err(CapError::InvalidSlot),
            Some(Some(_)) => Err(CapError::PermissionDenied),
            Some(None) => self.set(slot, held),
        }
    }

    /// The descriptors the table holds.
    fn caps(&self) -> impl Iterator<Item = Capability> + '_ {
        self.slots.iter().flatten().map(Held::cap)
    }

    /// Whether a capability of a device that does not snoop the CPU caches is in the table —
    /// the right to user-mode cache maintenance (ADR-0079).
    pub fn holds_noncoherent_device(&self) -> bool {
        self.caps()
            .any(|cap| matches!(cap.kind, CapabilityKind::DeviceMmio { dma_noncoherent: true, .. }))
    }

    /// Allocates the first free slot and inserts `held`, returning the slot index. A full
    /// table grows first (ADR-0093). Refused, `held` is dropped — counted off.
    pub fn allocate(&mut self, held: Held) -> Result<usize, CapError> {
        self.try_allocate(held).map_err(|(err, _dropped)| err)
    }

    /// `allocate` that hands a refused capability back: one that travelled in a message stays
    /// with its message.
    pub fn try_allocate(&mut self, held: Held) -> Result<usize, (CapError, Held)> {
        let index = match self.slots.iter().position(Option::is_none) {
            Some(index) => index,
            None => match self.grow() {
                Ok(index) => index,
                Err(err) => return Err((err, held)),
            },
        };
        let held = self.enter(held);
        self.slots[index] = Some(held);
        Ok(index)
    }

    /// Slots the table holds, free or not (observability; the tests' seam).
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Doubles the table (TASK-0352 P7, ADR-0093): the first new slot's index — or `NoSpace`,
    /// when the kernel heap above its reserve (ADR-0091) refuses the growth.
    fn grow(&mut self) -> Result<usize, CapError> {
        let first = self.slots.len();
        let more = first.max(8);
        let bytes =
            more.checked_mul(core::mem::size_of::<Option<Held>>()).ok_or(CapError::NoSpace)?;
        if !crate::kheap::admits(bytes) || self.slots.try_reserve(more).is_err() {
            return Err(CapError::NoSpace);
        }
        self.slots.resize_with(first + more, || None);
        Ok(first)
    }

    /// Allocates the first free slot scanning from the TOP of the table
    /// downward (RFC-0069 slot discipline): kernel objects a task creates for
    /// ITSELF land high, while caps installed by someone else (exec-time
    /// installs, `cap_transfer`, CAP_MOVE deliveries) keep packing low — so
    /// init's deterministic child layout and a service's own early object
    /// creation can never race for the same slot.
    pub fn allocate_high(&mut self, held: Held) -> Result<usize, CapError> {
        let index = match self.slots.iter().rposition(Option::is_none) {
            Some(index) => index,
            // A full table grows (ADR-0093); the task's own object lands at the new top.
            None => self.grow().map(|_| self.slots.len() - 1)?,
        };
        let held = self.enter(held);
        self.slots[index] = Some(held);
        Ok(index)
    }

    /// How many capabilities in THIS table hold the memory object `id` (`Vmo` and its `VmoRo`
    /// aliases alike). How many exist anywhere is the object's own count (`HeldCounts::any`).
    pub fn vmo_ref_count(&self, id: u32) -> usize {
        self.caps()
            .filter(|cap| match cap.kind {
                CapabilityKind::Vmo { id: i, .. } | CapabilityKind::VmoRo { id: i, .. } => i == id,
                _ => false,
            })
            .count()
    }

    /// The distinct endpoints this table holds a live SEND right to (TASK-0324 P7-b): what
    /// a dying task must be scanned for so every receiver it was the last peer of is woken.
    pub fn endpoint_send_ids(&self, out: &mut alloc::vec::Vec<EndpointId>) {
        self.endpoint_ids_with(Rights::SEND, out);
    }

    /// The distinct endpoints this table names with every right in `rights`, each once
    /// (`Rights::empty()` = every endpoint it names) — what a dying task's exit must judge.
    pub fn endpoint_ids_with(&self, rights: Rights, out: &mut alloc::vec::Vec<EndpointId>) {
        for cap in self.caps() {
            if let CapabilityKind::Endpoint(id) = cap.kind {
                if cap.rights.contains(rights) && !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }

    /// Every VMO this table names (`Vmo` and its `VmoRo` aliases alike), each once — what a
    /// dying task may leave orphaned (TASK-0333 6a, RFC-0100 §1).
    pub fn vmo_ids(&self, out: &mut alloc::vec::Vec<u32>) {
        for cap in self.caps() {
            if let CapabilityKind::Vmo { id, .. } | CapabilityKind::VmoRo { id, .. } = cap.kind {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }

    /// True when some slot holds a device capability carrying PLIC line `irq`
    /// (RFC-0098 C3: the line a task may bind or complete is the one its device
    /// capability carries — never a number it names).
    pub fn holds_device_irq(&self, irq: u32) -> bool {
        irq != 0
            && self.caps().any(|cap| {
                matches!(cap.kind, CapabilityKind::DeviceMmio { irq: line, .. } if line == irq)
                    && cap.rights.contains(Rights::MAP)
            })
    }

    /// The device record named by the capability in `slot` (a `DeviceMmio` held with
    /// MAP): the device a VMO is made for, or whose bus addresses `vmo_runs` answers in
    /// (RFC-0098 C4, TASK-0246 P1).
    pub fn device(&self, slot: usize) -> Result<u16, CapError> {
        match self.derive(slot, Rights::MAP)?.kind {
            CapabilityKind::DeviceMmio { dev, .. } => Ok(dev),
            _ => Err(CapError::PermissionDenied),
        }
    }

    /// What `slot` holds, left where it is.
    pub fn held(&self, slot: usize) -> Result<&Held, CapError> {
        self.slots.get(slot).and_then(Option::as_ref).ok_or(CapError::InvalidSlot)
    }

    /// The descriptor of the capability in `slot`.
    pub fn get(&self, slot: usize) -> Result<Capability, CapError> {
        self.held(slot).map(Held::cap)
    }

    /// The descriptor in `slot`, `None` when the slot is empty (`InvalidSlot` past the table) —
    /// where `get` cannot tell an empty slot from a missing one.
    pub fn peek(&self, slot: usize) -> Result<Option<Capability>, CapError> {
        self.slots.get(slot).map(|entry| entry.as_ref().map(Held::cap)).ok_or(CapError::InvalidSlot)
    }

    /// Closes `slot`: the capability is dropped — counted off — and its descriptor returned,
    /// so the caller judges the object after the drop.
    pub fn take(&mut self, slot: usize) -> Result<Capability, CapError> {
        self.take_held(slot).map(|held| held.cap())
    }

    /// Takes the capability out of `slot` to travel (into a message, or back into a slot). The
    /// object's counts of all, of senders and of receivers do not change: it is still held.
    pub fn take_held(&mut self, slot: usize) -> Result<Held, CapError> {
        let entry = self.slots.get_mut(slot).ok_or(CapError::InvalidSlot)?;
        let mut held = entry.take().ok_or(CapError::InvalidSlot)?;
        held.leave_table();
        Ok(held)
    }

    /// A second capability to what `slot` holds, of the same kind, with `rights` (a subset of
    /// the slot's).
    pub fn dup(&self, slot: usize, rights: Rights) -> Result<Held, CapError> {
        self.held(slot)?.dup(rights)
    }

    /// The descriptor `slot` would have with `rights` (a subset) — a check, not a capability.
    pub fn derive(&self, slot: usize, rights: Rights) -> Result<Capability, CapError> {
        let base = self.get(slot)?;
        if !base.rights.contains(rights) {
            return Err(CapError::PermissionDenied);
        }
        Ok(Capability { kind: base.kind, rights })
    }

    /// Derives an endpoint capability reference with compile-time kind tagging.
    pub fn derive_endpoint_ref(
        &self,
        slot: usize,
        rights: Rights,
    ) -> Result<EndpointCapRef, CapError> {
        let cap = self.derive(slot, rights)?;
        let CapabilityKind::Endpoint(endpoint) = cap.kind else {
            return Err(CapError::PermissionDenied);
        };
        Ok(EndpointCapRef { slot, endpoint, _tag: PhantomData })
    }
}

#[cfg(test)]
mod tests_prop;
