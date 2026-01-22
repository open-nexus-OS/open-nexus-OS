// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: RAM-based A/B boot control state machine (v1.0 non-persistent)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 7 tests (via tests/updates_host/ota_flow.rs)
//!   - stage/switch/health_commit flow
//!   - rollback on health timeout
//!   - switch without stage fails
//!   - commit_health without switch fails
//!   - double switch fails
//!
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn other(self) -> Self {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootCtrlError {
    NotStaged,
    AlreadyPending,
    NotPending,
    NoRollbackTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootCtrl {
    active_slot: Slot,
    pending_slot: Option<Slot>,
    staged_slot: Option<Slot>,
    rollback_slot: Option<Slot>,
    tries_left: u8,
    health_ok: bool,
}

impl BootCtrl {
    pub fn new(active_slot: Slot) -> Self {
        Self {
            active_slot,
            pending_slot: None,
            staged_slot: None,
            rollback_slot: None,
            tries_left: 0,
            health_ok: false,
        }
    }

    pub fn active_slot(&self) -> Slot {
        self.active_slot
    }

    pub fn pending_slot(&self) -> Option<Slot> {
        self.pending_slot
    }

    pub fn staged_slot(&self) -> Option<Slot> {
        self.staged_slot
    }

    pub fn tries_left(&self) -> u8 {
        self.tries_left
    }

    pub fn health_ok(&self) -> bool {
        self.health_ok
    }

    pub fn stage(&mut self) -> Slot {
        let standby = self.active_slot.other();
        self.staged_slot = Some(standby);
        standby
    }

    pub fn switch(&mut self, tries_left: u8) -> Result<Slot, BootCtrlError> {
        if self.pending_slot.is_some() {
            return Err(BootCtrlError::AlreadyPending);
        }
        let slot = self.staged_slot.take().ok_or(BootCtrlError::NotStaged)?;
        let previous = self.active_slot;
        self.active_slot = slot;
        self.pending_slot = Some(slot);
        self.rollback_slot = Some(previous);
        self.tries_left = tries_left;
        self.health_ok = false;
        Ok(slot)
    }

    pub fn commit_health(&mut self) -> Result<(), BootCtrlError> {
        if self.pending_slot.is_none() {
            return Err(BootCtrlError::NotPending);
        }
        self.pending_slot = None;
        self.rollback_slot = None;
        self.tries_left = 0;
        self.health_ok = true;
        Ok(())
    }

    pub fn tick_boot_attempt(&mut self) -> Result<Option<Slot>, BootCtrlError> {
        if self.pending_slot.is_none() {
            return Ok(None);
        }
        if self.tries_left > 0 {
            self.tries_left = self.tries_left.saturating_sub(1);
        }
        if self.tries_left == 0 {
            let rolled_back = self.rollback()?;
            return Ok(Some(rolled_back));
        }
        Ok(None)
    }

    pub fn rollback(&mut self) -> Result<Slot, BootCtrlError> {
        let target = self.rollback_slot.ok_or(BootCtrlError::NoRollbackTarget)?;
        self.active_slot = target;
        self.pending_slot = None;
        self.rollback_slot = None;
        self.staged_slot = None;
        self.tries_left = 0;
        self.health_ok = false;
        Ok(target)
    }
}
