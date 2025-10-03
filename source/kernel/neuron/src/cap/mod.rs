// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Capability table implementation.

extern crate alloc;

use alloc::vec;      
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
#[derive(Default)]
pub struct CapTable {
    slots: Vec<Option<Capability>>,
}

impl CapTable {
    /// Creates an empty table sized for `slots` entries.
    pub fn with_capacity(slots: usize) -> Self {
        Self { slots: vec![None; slots] }
    }

    /// Convenience constructor for the bootstrap task.
    pub fn new() -> Self {
        Self::with_capacity(32)
    }

    /// Inserts or overwrites a slot.
    pub fn set(&mut self, slot: usize, cap: Capability) -> Result<(), CapError> {
        self.slots
            .get_mut(slot)
            .ok_or(CapError::InvalidSlot)
            .map(|entry| *entry = Some(cap))
    }

    /// Returns a capability without consuming it.
    pub fn get(&self, slot: usize) -> Result<Capability, CapError> {
        self.slots
            .get(slot)
            .and_then(|entry| *entry)
            .ok_or(CapError::InvalidSlot)
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
