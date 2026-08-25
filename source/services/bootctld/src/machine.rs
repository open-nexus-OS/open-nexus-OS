// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: A/B boot-control state machine + boot targets (TASK-0050,
//! ADR-0055). RELOCATED from `userspace/updates/src/bootctrl.rs` (the
//! proven RFC-0012 machine — semantics unchanged, verbatim where possible)
//! and extended with the RFC-0087 §4 boot-target axis: `boot_target`
//! (persistent mode) and `next_boot` (one-shot; consumed exactly once by
//! init via bootctld). New here: `rollback_slot()` getter and
//! `restore(..)` — the v1 codec could not persist the rollback slot and
//! rebuilt state by REPLAYING stage/switch (a workaround this relocation
//! kills; the v2 record persists every field).
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0 machine semantics; target axis Unstable)
//! TEST_COVERAGE: tests/record_v2.rs (machine flows ported from
//!   tests/updates_host/ota_flow.rs + target/one-shot semantics).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

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

/// Boot target (RFC-0087 §4): which declarative service graph init
/// materializes. `Normal` is the full graph; `Recovery` the minimal
/// ops graph; `Safe` the session graph minus non-essential services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootTarget {
    Normal,
    Recovery,
    Safe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootCtrlError {
    NotStaged,
    AlreadyPending,
    NotPending,
    NoRollbackTarget,
    /// Health report from a bit outside the declared quorum set (or a
    /// non-single-bit mask) — RFC-0089 §13: unknown reporters are rejected.
    UnknownReporter,
}

/// Progress of the health-commit quorum after one report (RFC-0089 §13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuorumProgress {
    /// Confirmed reporters so far (popcount of the mask).
    pub have: u8,
    /// Total declared reporters (popcount of the full mask).
    pub need: u8,
    /// True exactly when this report completed the quorum (commit fired).
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootCtrl {
    active_slot: Slot,
    pending_slot: Option<Slot>,
    staged_slot: Option<Slot>,
    rollback_slot: Option<Slot>,
    tries_left: u8,
    health_ok: bool,
    boot_target: BootTarget,
    next_boot: Option<BootTarget>,
    /// Anti-downgrade floor (RFC-0089 §10). Field lands with record v3;
    /// RAISING it on commit is TASK-0179's apply-engine scope.
    rollback_min_index: u32,
    /// Health-commit v2 quorum: confirmed reporter bits since the last
    /// switch (RFC-0089 §13). Cleared when a switch arms a new trial.
    health_mask: u8,
    /// Absolute wall-clock deadline for the quorum (0 = none). Expiry
    /// without quorum schedules a rollback at the next boot attempt.
    commit_deadline_ns: u64,
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
            boot_target: BootTarget::Normal,
            next_boot: None,
            rollback_min_index: 0,
            health_mask: 0,
            commit_deadline_ns: 0,
        }
    }

    /// Direct field restore (v3 record → live machine). Replaces the v1-era
    /// replay reconstruction; the caller (record codec) owns validation.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        active_slot: Slot,
        pending_slot: Option<Slot>,
        staged_slot: Option<Slot>,
        rollback_slot: Option<Slot>,
        tries_left: u8,
        health_ok: bool,
        boot_target: BootTarget,
        next_boot: Option<BootTarget>,
        rollback_min_index: u32,
        health_mask: u8,
        commit_deadline_ns: u64,
    ) -> Self {
        Self {
            active_slot,
            pending_slot,
            staged_slot,
            rollback_slot,
            tries_left,
            health_ok,
            boot_target,
            next_boot,
            rollback_min_index,
            health_mask,
            commit_deadline_ns,
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

    /// Rollback destination while a switch is pending (v2: persisted).
    pub fn rollback_slot(&self) -> Option<Slot> {
        self.rollback_slot
    }

    pub fn tries_left(&self) -> u8 {
        self.tries_left
    }

    pub fn health_ok(&self) -> bool {
        self.health_ok
    }

    pub fn boot_target(&self) -> BootTarget {
        self.boot_target
    }

    pub fn next_boot(&self) -> Option<BootTarget> {
        self.next_boot
    }

    /// Sets the persistent boot target (policy-gated at the wire layer).
    pub fn set_boot_target(&mut self, target: BootTarget) {
        self.boot_target = target;
    }

    /// Arms a one-shot target for the NEXT boot only.
    pub fn set_next_boot(&mut self, target: BootTarget) {
        self.next_boot = Some(target);
    }

    /// One-shot consumption: returns and CLEARS `next_boot`. The caller
    /// must persist the cleared state in the same transaction that
    /// acknowledges the boot attempt (a crash in between must not loop
    /// the target — RFC-0087 §4).
    pub fn take_next_boot(&mut self) -> Option<BootTarget> {
        self.next_boot.take()
    }

    /// Anti-downgrade floor (RFC-0089 §10).
    pub fn rollback_min_index(&self) -> u32 {
        self.rollback_min_index
    }

    /// Raises the floor (never decreases — RFC-0089 §10). Returns whether
    /// the value actually changed. Called by the commit path once
    /// TASK-0179's apply engine carries real NXBD indices.
    pub fn raise_rollback_min(&mut self, index: u32) -> bool {
        if index > self.rollback_min_index {
            self.rollback_min_index = index;
            true
        } else {
            false
        }
    }

    /// Confirmed quorum reporter bits since the last switch.
    pub fn health_mask(&self) -> u8 {
        self.health_mask
    }

    /// Absolute quorum deadline (0 = none armed).
    pub fn commit_deadline_ns(&self) -> u64 {
        self.commit_deadline_ns
    }

    pub fn stage(&mut self) -> Slot {
        let standby = self.active_slot.other();
        self.staged_slot = Some(standby);
        standby
    }

    /// Arms a trial: `deadline_ns` is the ABSOLUTE wall-clock bound for the
    /// health quorum (0 = tries-only; the caller computes now + window so
    /// the machine stays pure/injectable). Clears the quorum mask.
    pub fn switch(&mut self, tries_left: u8, deadline_ns: u64) -> Result<Slot, BootCtrlError> {
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
        self.health_mask = 0;
        self.commit_deadline_ns = deadline_ns;
        Ok(slot)
    }

    /// Health-commit v2 (RFC-0089 §13): one reporter confirms its declared
    /// bit. Duplicates are idempotent; a bit outside `full_mask` (or a
    /// non-single-bit report) is rejected. Commit fires exactly when the
    /// mask completes.
    pub fn report_health(
        &mut self,
        bit: u8,
        full_mask: u8,
    ) -> Result<QuorumProgress, BootCtrlError> {
        if self.pending_slot.is_none() {
            return Err(BootCtrlError::NotPending);
        }
        if bit.count_ones() != 1 || (bit & full_mask) == 0 {
            return Err(BootCtrlError::UnknownReporter);
        }
        self.health_mask |= bit;
        let confirmed = self.health_mask & full_mask;
        let complete = confirmed == full_mask;
        if complete {
            self.commit_internal();
        }
        Ok(QuorumProgress {
            have: confirmed.count_ones() as u8,
            need: full_mask.count_ones() as u8,
            complete,
        })
    }

    /// The commit itself — reachable only through a completed quorum
    /// (`report_health`); kept private so no path can bypass the mask.
    fn commit_internal(&mut self) {
        self.pending_slot = None;
        self.rollback_slot = None;
        self.tries_left = 0;
        self.health_ok = true;
        self.commit_deadline_ns = 0;
    }

    /// One boot attempt at wall-clock `now_ns`: a pending trial past its
    /// quorum deadline rolls back immediately (regardless of tries left);
    /// otherwise the tries counter decrements and exhaustion rolls back.
    pub fn tick_boot_attempt(&mut self, now_ns: u64) -> Result<Option<Slot>, BootCtrlError> {
        if self.pending_slot.is_none() {
            return Ok(None);
        }
        if self.commit_deadline_ns != 0 && now_ns >= self.commit_deadline_ns {
            let rolled_back = self.rollback()?;
            return Ok(Some(rolled_back));
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
        self.health_mask = 0;
        self.commit_deadline_ns = 0;
        Ok(target)
    }
}
