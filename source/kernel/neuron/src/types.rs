// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal newtypes for safer syscall decoding (debug-friendly, low overhead)
//! OWNERS: @kernel-team
//! PUBLIC API: VirtAddr, PageLen, SlotIndex, Pid, AsHandle, CapSlot, Asid
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
pub struct Pid(u32);

impl Pid {
    /// Creates a PID from a raw value (kernel-internal only).
    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw PID value.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// Kernel PID (reserved, never exposed to userspace).
    pub const KERNEL: Self = Self(0);
}

/// Address space identifier (ASID).
///
/// **Ownership**: Only `AddressSpaceManager` can allocate/free ASIDs.
/// **Invariant**: ASID 0 is reserved for the kernel identity map.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
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
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Kernel ASID (reserved for kernel identity map).
    #[allow(dead_code)]
    // NOTE: Bring-up: referenced once AS manager/VM init is fully connected.
    // REMOVE_WHEN(TASK-0011B): Kernel ASID is referenced by the live address-space code.
    pub const KERNEL: Self = Self(0);
}

/// Address space handle (opaque userspace handle).
///
/// **Ownership**: Created by `sys_as_create()`, destroyed by `sys_as_destroy()`.
/// **Invariant**: Handles are opaque to userspace (internal ASID not exposed).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AsHandle(u64);

impl AsHandle {
    /// Creates an address space handle from a raw value.
    #[inline]
    #[allow(dead_code)]
    // NOTE: Bring-up: AS handle syscalls are staged; keep this constructor for the kernel API.
    // REMOVE_WHEN(TASK-0011B): Address-space syscalls use this constructor.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the raw handle value.
    #[inline]
    #[allow(dead_code)]
    // NOTE: Bring-up: used by syscall plumbing once AS handles are passed through tables.
    // REMOVE_WHEN(TASK-0011B): Address-space syscalls and tables use this accessor.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// Capability slot index (per-task capability table).
///
/// **Ownership**: Each task owns its capability table.
/// **Invariant**: Slot indices are bounded by table size (validated at syscall boundary).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapSlot(u32);

impl CapSlot {
    /// Creates a capability slot from a raw value.
    #[inline]
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the raw slot index.
    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
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
