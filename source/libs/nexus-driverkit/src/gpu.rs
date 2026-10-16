// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The GPU's device-side vocabulary (RFC-0101 §1–§4, §6, §7): queue kinds, priorities,
//! cache modes, the timeline point, the capabilities a client queries, and the one thing a
//! device server enforces before anything reaches the hardware — the context's own GPU
//! address space, [`VmSpace`].
//!
//! CONTEXT: isolation is the GPU MMU's, never a parser's (RFC-0033 keeps validation out of
//! the kernel): a batch may name only addresses bound in its context, and the space refuses
//! what the MMU could not hold — an unaligned, overlapping or out-of-range binding. Pure and
//! `no_std`, allocation-free (a fixed binding table), so the host is the oracle; the device
//! server (gpud, Phase 2) programs the MMU from the bindings this space accepted.
//! OWNERS: @gpu @drivers
//! STATUS: Functional (contract; the device consumer is TASK-0329 G1)
//! API_STABILITY: Unstable until RFC-0101 Phase 2
//! TEST_COVERAGE: host (`cargo test -p nexus-driverkit gpu`) — the reject matrix of RFC-0101 §9
//! INVARIANTS: a binding (a range of a buffer: address, length and offset page-aligned) is
//!   non-empty, inside the space and disjoint from every other; a lookup resolves a whole range
//!   inside ONE binding or nothing.

/// The three queue kinds of every submit (§2). A device declares which it executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    Render,
    Compute,
    Transfer,
}

impl QueueKind {
    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// The set of queue kinds a device executes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueKinds(u8);

impl QueueKinds {
    /// No kind at all.
    pub const NONE: Self = Self(0);
    /// Render and transfer — what Phase 2 executes (compute is declared and refused by name).
    pub const RENDER_TRANSFER: Self = Self(QueueKind::Render.bit() | QueueKind::Transfer.bit());
    /// Every kind.
    pub const ALL: Self = Self(Self::RENDER_TRANSFER.0 | QueueKind::Compute.bit());

    /// Whether `kind` is in the set.
    pub const fn executes(self, kind: QueueKind) -> bool {
        self.0 & kind.bit() != 0
    }
}

/// A context's priority (§6); the compositor's context is `Realtime`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    Realtime,
}

/// How a buffer's memory is cached (§3) — the non-coherent machine's rule, by type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheMode {
    /// CPU-cached; cleaned/flushed at the ownership transitions (`DmaBuffer`).
    CpuCached,
    /// Mapped uncached for the CPU; no maintenance.
    Uncached,
    /// Never mapped for the CPU.
    GpuPrivate,
}

/// What a binding lets the GPU do with the memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

/// An address in a context's GPU address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GpuVa(pub u64);

/// A value on the kernel's timeline fence (§5, RFC-0033): monotonic, `u64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TimelinePoint(pub u64);

/// The target GPU code is built for (§7): the device's own id, checked before a binary runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CodeTarget(pub u64);

/// The largest batch any device of this tree accepts; a device may declare less.
pub const MAX_BATCH_COMMANDS: u32 = 4096;

/// What a device can do, readable before the first context (§7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub queue_kinds: QueueKinds,
    pub max_contexts: u32,
    /// Commands in one batch; never above [`MAX_BATCH_COMMANDS`].
    pub max_batch_commands: u32,
    /// Width of a context's address space.
    pub va_bits: u8,
    /// The MMU's page size — every binding is a multiple of it.
    pub page_size: u32,
    /// Whether a binary for [`Self::gpu_code_target`] can be executed at all.
    pub executes_gpu_code: bool,
    pub gpu_code_target: CodeTarget,
}

/// A batch's size against the device's bound (§4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    /// More commands than the capability allows — refused, never split.
    TooLarge { commands: usize, max: u32 },
}

/// Checks a batch of `commands` against `caps`.
pub fn check_batch(commands: usize, caps: &Capabilities) -> Result<(), BatchError> {
    let max = caps.max_batch_commands.min(MAX_BATCH_COMMANDS);
    if commands as u64 > u64::from(max) {
        return Err(BatchError::TooLarge { commands, max });
    }
    Ok(())
}

/// Why a binding was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmError {
    /// Address or length is not a multiple of the page size, or the length is zero.
    Unaligned,
    /// The range leaves the address space.
    OutOfRange,
    /// The range overlaps a binding.
    Overlap,
    /// The binding table is full.
    Full,
    /// No binding holds the range.
    Unbound,
}

/// A range of one buffer bound at one address: `len` bytes of the buffer from `offset`, at `va`
/// (a client binds ranges of a few large buffers, as every production GPU stack does).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub va: GpuVa,
    pub len: u64,
    /// The buffer object, as the device server numbers it.
    pub bo: u32,
    /// The binding's first byte in the buffer (page-aligned).
    pub offset: u64,
    pub access: Access,
}

impl Binding {
    fn end(&self) -> u64 {
        self.va.0.saturating_add(self.len)
    }

    /// Where `va` (inside the binding) lies in the buffer.
    pub fn offset_of(&self, va: GpuVa) -> u64 {
        self.offset + va.0.saturating_sub(self.va.0)
    }

    fn overlaps(&self, other: &Binding) -> bool {
        self.va.0 < other.end() && other.va.0 < self.end()
    }
}

/// A context's GPU address space: at most `N` bindings, page-aligned, disjoint, in range.
#[derive(Clone, Debug)]
pub struct VmSpace<const N: usize> {
    va_bits: u8,
    page: u64,
    table: [Option<Binding>; N],
}

impl<const N: usize> VmSpace<N> {
    /// An empty space of `va_bits` width over pages of `page_size` bytes.
    pub const fn new(va_bits: u8, page_size: u32) -> Self {
        Self { va_bits, page: page_size as u64, table: [const { None }; N] }
    }

    /// One past the highest address.
    fn limit(&self) -> u64 {
        1u64 << self.va_bits.min(63)
    }

    /// Binds; refuses what the MMU could not hold.
    pub fn bind(&mut self, binding: Binding) -> Result<(), VmError> {
        if self.page == 0 || binding.len == 0 {
            return Err(VmError::Unaligned);
        }
        if binding.va.0 % self.page != 0
            || binding.len % self.page != 0
            || binding.offset % self.page != 0
        {
            return Err(VmError::Unaligned);
        }
        if binding.end() > self.limit() || binding.end() < binding.va.0 {
            return Err(VmError::OutOfRange);
        }
        if self.table.iter().flatten().any(|b| b.overlaps(&binding)) {
            return Err(VmError::Overlap);
        }
        match self.table.iter_mut().find(|slot| slot.is_none()) {
            Some(slot) => {
                *slot = Some(binding);
                Ok(())
            }
            None => Err(VmError::Full),
        }
    }

    /// Unbinds the binding that starts at `va`.
    pub fn unbind(&mut self, va: GpuVa) -> Result<Binding, VmError> {
        let slot = self
            .table
            .iter_mut()
            .find(|slot| slot.is_some_and(|b| b.va == va))
            .ok_or(VmError::Unbound)?;
        slot.take().ok_or(VmError::Unbound)
    }

    /// The binding that holds the whole range `va..va+len`, or `Unbound`.
    pub fn resolve(&self, va: GpuVa, len: u64) -> Result<&Binding, VmError> {
        let end = va.0.checked_add(len).ok_or(VmError::OutOfRange)?;
        self.table
            .iter()
            .flatten()
            .find(|b| b.va.0 <= va.0 && end <= b.end())
            .ok_or(VmError::Unbound)
    }

    /// Every binding, in table order.
    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.table.iter().flatten()
    }

    /// Unbinds everything (a context's destruction).
    pub fn clear(&mut self) {
        self.table = [const { None }; N];
    }

    /// Bound bytes — what the client is charged for (M4).
    pub fn bound_bytes(&self) -> u64 {
        self.bindings().map(|b| b.len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: u32 = 4096;

    fn caps() -> Capabilities {
        Capabilities {
            queue_kinds: QueueKinds::RENDER_TRANSFER,
            max_contexts: 8,
            max_batch_commands: 256,
            va_bits: 40,
            page_size: PAGE,
            executes_gpu_code: false,
            gpu_code_target: CodeTarget(0x24001D003400B6),
        }
    }

    fn binding(va: u64, len: u64, bo: u32) -> Binding {
        Binding { va: GpuVa(va), len, bo, offset: 0, access: Access::ReadWrite }
    }

    #[test]
    fn a_binding_resolves_its_whole_range_and_nothing_beyond() {
        let mut space = VmSpace::<4>::new(40, PAGE);
        space.bind(binding(0x10000, 0x4000, 7)).unwrap();
        assert_eq!(space.resolve(GpuVa(0x10000), 0x4000).unwrap().bo, 7);
        assert_eq!(space.resolve(GpuVa(0x11000), 0x100).unwrap().bo, 7);
        assert_eq!(space.resolve(GpuVa(0x13000), 0x1001), Err(VmError::Unbound));
        assert_eq!(space.bound_bytes(), 0x4000);
        assert_eq!(space.unbind(GpuVa(0x10000)).unwrap().bo, 7);
        assert_eq!(space.unbind(GpuVa(0x10000)), Err(VmError::Unbound));
    }

    #[test]
    fn two_ranges_of_one_buffer_resolve_to_their_own_offsets() {
        let mut space = VmSpace::<4>::new(40, PAGE);
        space.bind(Binding { offset: 0x3000, ..binding(0x10000, 0x2000, 7) }).unwrap();
        space.bind(Binding { offset: 0x1000, ..binding(0x80000, 0x1000, 7) }).unwrap();
        let low = space.resolve(GpuVa(0x11000), 0x10).unwrap();
        assert_eq!(low.offset_of(GpuVa(0x11008)), 0x4008);
        let high = space.resolve(GpuVa(0x80000), 0x1000).unwrap();
        assert_eq!(high.offset_of(GpuVa(0x80010)), 0x1010);
    }

    #[test]
    fn test_reject_bind_overlapping_range() {
        let mut space = VmSpace::<4>::new(40, PAGE);
        space.bind(binding(0x10000, 0x4000, 1)).unwrap();
        assert_eq!(space.bind(binding(0x13000, 0x2000, 2)), Err(VmError::Overlap));
        assert_eq!(space.bind(binding(0x0f000, 0x2000, 2)), Err(VmError::Overlap));
        assert_eq!(space.bind(binding(0x14000, 0x1000, 2)), Ok(()), "adjacent is not overlap");
    }

    #[test]
    fn test_reject_unaligned_binding() {
        let mut space = VmSpace::<4>::new(40, PAGE);
        assert_eq!(space.bind(binding(0x10800, 0x1000, 1)), Err(VmError::Unaligned));
        assert_eq!(space.bind(binding(0x10000, 0x0800, 1)), Err(VmError::Unaligned));
        assert_eq!(space.bind(binding(0x10000, 0, 1)), Err(VmError::Unaligned));
        let mid_page = Binding { offset: 0x800, ..binding(0x10000, 0x1000, 1) };
        assert_eq!(space.bind(mid_page), Err(VmError::Unaligned), "a range starts on a page");
    }

    #[test]
    fn test_reject_bind_outside_space() {
        let mut space = VmSpace::<4>::new(32, PAGE);
        assert_eq!(space.bind(binding(0xFFFF_F000, 0x2000, 1)), Err(VmError::OutOfRange));
        assert_eq!(space.bind(binding(u64::MAX - 0xFFF, 0x1000, 1)), Err(VmError::OutOfRange));
        assert_eq!(space.bind(binding(0xFFFF_F000, 0x1000, 1)), Ok(()), "the last page fits");
    }

    #[test]
    fn test_reject_too_many_bindings() {
        let mut space = VmSpace::<2>::new(40, PAGE);
        space.bind(binding(0x1000, 0x1000, 1)).unwrap();
        space.bind(binding(0x3000, 0x1000, 2)).unwrap();
        assert_eq!(space.bind(binding(0x5000, 0x1000, 3)), Err(VmError::Full));
        space.clear();
        assert_eq!(space.bind(binding(0x5000, 0x1000, 3)), Ok(()));
    }

    #[test]
    fn test_reject_lookup_of_an_unbound_va() {
        let space = VmSpace::<4>::new(40, PAGE);
        assert_eq!(space.resolve(GpuVa(0x1000), 16), Err(VmError::Unbound));
        assert_eq!(space.resolve(GpuVa(u64::MAX), 16), Err(VmError::OutOfRange));
    }

    #[test]
    fn test_reject_batch_over_the_capability() {
        let caps = caps();
        assert_eq!(check_batch(256, &caps), Ok(()));
        assert_eq!(check_batch(257, &caps), Err(BatchError::TooLarge { commands: 257, max: 256 }));
        let wide = Capabilities { max_batch_commands: u32::MAX, ..caps };
        assert_eq!(
            check_batch(MAX_BATCH_COMMANDS as usize + 1, &wide),
            Err(BatchError::TooLarge {
                commands: MAX_BATCH_COMMANDS as usize + 1,
                max: MAX_BATCH_COMMANDS
            }),
            "a device never declares more than the tree's bound"
        );
    }

    #[test]
    fn queue_kinds_are_declared_not_assumed() {
        assert!(QueueKinds::RENDER_TRANSFER.executes(QueueKind::Render));
        assert!(QueueKinds::RENDER_TRANSFER.executes(QueueKind::Transfer));
        assert!(!QueueKinds::RENDER_TRANSFER.executes(QueueKind::Compute));
        assert!(QueueKinds::ALL.executes(QueueKind::Compute));
        assert!(!QueueKinds::NONE.executes(QueueKind::Render));
        assert!(Priority::Realtime > Priority::High && Priority::High > Priority::Low);
    }
}
