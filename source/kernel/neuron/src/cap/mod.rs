// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Capability table implementation.

extern crate alloc;

use alloc::vec::Vec;
use bitflags::bitflags;
use core::fmt;

use crate::ipc::EndpointId;

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    /// Rights associated with a capability handle.
    pub struct Rights: u32 {
        const SEND = 1 << 0;
        const RECV = 1 << 1;
        const MAP = 1 << 2;
        const MANAGE = 1 << 3;
    }
}

/// Capability handle types exposed to tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityKind {
    /// Kernel message endpoint.
    Endpoint(EndpointId),
    /// Virtual memory object.
    Vmo { base: usize, len: usize },
    /// Interrupt binding.
    Irq(u32),
}

/// Capability descriptor stored in the table.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// Provided slot is invalid.
    InvalidSlot,
    /// Insufficient rights for the requested operation.
    PermissionDenied,
}

/// Per-task capability table.
#[derive(Default, Clone)]
pub struct CapTable {
    slots: Vec<Option<Capability>>,
}

impl CapTable {
    /// Creates an empty table sized for `slots` entries.
    pub fn with_capacity(slots: usize) -> Self {
        let mut table: Vec<Option<Capability>> = Vec::with_capacity(slots);
        for _ in 0..slots {
            // Avoids potential libc/memset intrinsics on no_std target
            table.push(None);
        }
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "CAP: with_capacity slots={}\n", slots);
        }
        Self { slots: table }
    }

    /// Convenience constructor for the bootstrap task.
    pub fn new() -> Self {
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "CAP: new enter\n");
        }
        let table = Self::with_capacity(32);
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "CAP: new exit\n");
        }
        table
    }

    /// Inserts or overwrites a slot.
    pub fn set(&mut self, slot: usize, cap: Capability) -> Result<(), CapError> {
        if slot >= self.slots.len() {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "CAP-E: invalid slot {} (len={})\n", slot, self.slots.len());
            return Err(CapError::InvalidSlot);
        }
        if let Some(entry) = self.slots.get_mut(slot) {
            *entry = Some(cap);
            Ok(())
        } else {
            Err(CapError::InvalidSlot)
        }
    }

    /// Allocates the first free slot and inserts `cap`, returning the slot index.
    pub fn allocate(&mut self, cap: Capability) -> Result<usize, CapError> {
        for (index, entry) in self.slots.iter_mut().enumerate() {
            if entry.is_none() {
                *entry = Some(cap);
                return Ok(index);
            }
        }
        // Grow the table by one when no free slot is available.
        self.slots.push(Some(cap));
        Ok(self.slots.len() - 1)
    }

    /// Returns a capability without consuming it.
    pub fn get(&self, slot: usize) -> Result<Capability, CapError> {
        self.slots.get(slot).and_then(|entry| *entry).ok_or(CapError::InvalidSlot)
    }

    /// Derives a new capability with intersected rights.
    pub fn derive(&self, slot: usize, rights: Rights) -> Result<Capability, CapError> {
        let base = self.get(slot)?;
        if !base.rights.contains(rights) {
            return Err(CapError::PermissionDenied);
        }
        Ok(Capability { kind: base.kind, rights })
    }
}

#[cfg(test)]
mod tests_prop;
