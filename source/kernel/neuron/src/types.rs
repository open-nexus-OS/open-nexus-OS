// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal newtypes for safer syscall decoding (debug-friendly, low overhead)
//! OWNERS: @kernel-team
//! PUBLIC API: VirtAddr, PageLen, SlotIndex, Pid, CapSlot, Asid, HartId, CpuId
//! DEPENDS_ON: mm::page_table::is_canonical_sv39, PAGE_SIZE
//! INVARIANTS: Enforce canonical Sv39 addresses; alignment helpers; prevent type confusion
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
//!
//! ## Newtype Rationale (TASK-0011B)
//!
//! Rust newtypes provide **zero-cost type safety** at compile time:
//! - Prevent accidental mixing of PIDs, ASIDs, capability slots
//! - Make ownership explicit (who can create/destroy these handles?)
//! - Enable future SMP optimizations (e.g., `Pid` can embed CPU affinity)

use crate::mm::{page_table::is_canonical_sv39, PAGE_SIZE};
use core::fmt;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VirtAddr(usize);

impl VirtAddr {
    #[inline]
    pub fn new(addr: usize) -> Option<Self> {
        if is_canonical_sv39(addr) {
            Some(Self(addr))
        } else {
            None
        }
    }

    #[inline]
    pub fn page_aligned(addr: usize) -> Option<Self> {
        Self::new(addr).and_then(|va| if va.0 % PAGE_SIZE == 0 { Some(va) } else { None })
    }

    #[inline]
    pub fn instr_aligned(addr: usize) -> Option<Self> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        const INSTR_ALIGN: usize = 2; // allow compressed 16-bit alignment on RISC-V
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        const INSTR_ALIGN: usize = core::mem::size_of::<u32>();
        Self::new(addr).and_then(|va| if va.0 % INSTR_ALIGN == 0 { Some(va) } else { None })
    }
    #[inline]
    pub fn raw(self) -> usize {
        self.0
    }
    #[inline]
    pub fn checked_add(self, v: usize) -> Option<usize> {
        self.0.checked_add(v)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PageLen(usize);

impl PageLen {
    #[inline]
    pub fn from_bytes_aligned(bytes: u64) -> Option<Self> {
        if bytes == 0 {
            return None;
        }
        let b = bytes as usize;
        if b % PAGE_SIZE == 0 {
            Some(Self(b))
        } else {
            None
        }
    }
    #[inline]
    pub fn raw(self) -> usize {
        self.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SlotIndex(pub usize);

impl SlotIndex {
    #[inline]
    pub fn decode(value: usize) -> Self {
        Self(value)
    }
}

// ——— Additional Newtypes (TASK-0011B) ———

/// Process identifier (PID).
///
/// **Ownership**: Only `TaskTable` can create/destroy PIDs.
/// **Invariant**: PID 0 is reserved for the kernel (never exposed to userspace).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Pid(u32);

impl Pid {
    /// Creates a PID from a raw value (kernel-internal only).
    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw PID value.
    #[inline]
    pub const fn as_raw(self) -> u32 {
        self.0
    }

    /// Returns the PID as an index into task-owned vectors.
    #[inline]
    pub const fn as_index(self) -> usize {
        self.0 as usize
    }

    /// Backward-compatible alias while call sites migrate to `as_raw`.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.as_raw()
    }

    /// Kernel PID (reserved, never exposed to userspace).
    pub const KERNEL: Self = Self(0);
}

impl fmt::Display for Pid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_raw())
    }
}

/// Address space identifier (ASID).
///
/// **Ownership**: Only `AddressSpaceManager` can allocate/free ASIDs.
/// **Invariant**: ASID 0 is reserved for the kernel identity map.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Asid(u16);

impl Asid {
    /// Creates an ASID from a raw value (kernel-internal only).
    #[inline]
    #[allow(dead_code)]
    // NOTE: Bring-up: staged API for AddressSpaceManager + sys_as_* plumbing.
    // REMOVE_WHEN(TASK-0011B): Address space plumbing is fully wired and these helpers are used.
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    /// Returns the raw ASID value.
    #[inline]
    #[allow(dead_code)]
    // NOTE: Bring-up: used once ASID allocation + SATP switching plumbing lands.
    // REMOVE_WHEN(TASK-0011B): Address space plumbing uses this for SATP writes.
    pub const fn as_raw(self) -> u16 {
        self.0
    }

    /// Backward-compatible alias while call sites migrate to `as_raw`.
    #[inline]
    pub const fn raw(self) -> u16 {
        self.as_raw()
    }

    /// Kernel ASID (reserved for kernel identity map).
    #[allow(dead_code)]
    // NOTE: Bring-up: referenced once AS manager/VM init is fully connected.
    // REMOVE_WHEN(TASK-0011B): Kernel ASID is referenced by the live address-space code.
    pub const KERNEL: Self = Self(0);
}

/// Capability slot index (per-task capability table).
///
/// **Ownership**: Each task owns its capability table.
/// **Invariant**: Slot indices are bounded by table size (validated at syscall boundary).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct CapSlot(u32);

impl CapSlot {
    /// Creates a capability slot from a raw value.
    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw slot index.
    #[inline]
    pub const fn as_raw(self) -> u32 {
        self.0
    }

    /// Returns the slot index as `usize` for table access.
    #[inline]
    pub const fn as_index(self) -> usize {
        self.0 as usize
    }

    /// Backward-compatible alias while call sites migrate to `as_raw`.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.as_raw()
    }

    /// Bootstrap endpoint slot (fixed contract).
    pub const BOOTSTRAP: Self = Self(0);
}

impl fmt::Display for CapSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_raw())
    }
}

/// Hardware hart identifier as reported by RISC-V (`mhartid`).
///
/// We keep this distinct from scheduler-facing CPU IDs so call-sites cannot
/// accidentally mix hardware identity with logical scheduler routing.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct HartId(u16);

impl HartId {
    #[inline]
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn as_raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn as_index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for HartId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_raw())
    }
}

/// Logical CPU identifier used by scheduler and per-CPU kernel state.
///
/// In TASK-0012 v1 this is a 1:1 mapping from `HartId`, but remains a dedicated
/// type to keep future topology/affinity changes explicit.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct CpuId(u16);

impl CpuId {
    pub const BOOT: Self = Self(0);

    #[inline]
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn from_hart(hart: HartId) -> Self {
        Self(hart.as_raw())
    }

    #[inline]
    pub const fn as_raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn as_index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn is_boot(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for CpuId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_raw())
    }
}

// ——— Conversion Helpers (for gradual migration) ———

impl From<u32> for Pid {
    #[inline]
    fn from(raw: u32) -> Self {
        Self(raw)
    }
}

impl From<Pid> for u32 {
    #[inline]
    fn from(pid: Pid) -> Self {
        pid.0
    }
}

impl From<Pid> for usize {
    #[inline]
    fn from(pid: Pid) -> Self {
        pid.as_index()
    }
}

impl From<usize> for Pid {
    #[inline]
    fn from(raw: usize) -> Self {
        Self(raw as u32)
    }
}

impl From<u32> for CapSlot {
    #[inline]
    fn from(raw: u32) -> Self {
        Self(raw)
    }
}

impl From<CapSlot> for u32 {
    #[inline]
    fn from(slot: CapSlot) -> Self {
        slot.0
    }
}

impl From<CapSlot> for usize {
    #[inline]
    fn from(slot: CapSlot) -> Self {
        slot.as_index()
    }
}

impl From<usize> for CapSlot {
    #[inline]
    fn from(raw: usize) -> Self {
        Self(raw as u32)
    }
}
