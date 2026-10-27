// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: reaping a dead task — the parent's `wait` and the kernel's own reap of a thread
//! whose process is gone (TASK-0333 6d, ADR-0082). Split out of `task/mod.rs` at its
//! structure-gate ceiling when the process model gained threads that die with their leader.
//!
//! The one release path is [`TaskTable::release_reaped`]: the table emptied, the address space
//! detached and destroyed when this task was its last owner, the orphaned objects reclaimed.
//! `reap_child` is the parent's view of it (RFC-0081 `wait`/`wait_nohang`); a thread of a dead
//! process has no one to wait for it, so its exit funnel releases it directly.
//!
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: task exit releases ok`, `KSELFTEST: process exit takes threads ok`

use super::{
    BlockReason, ExitReason, Pid, Scheduler, TaskState, TaskTable, WaitError, WakeOutcome,
};
use crate::{
    cap::CapTable,
    mm::{AddressSpaceError, AddressSpaceManager},
    trap::TrapFrame,
};
use alloc::vec::Vec;

impl TaskTable {
    /// The endpoint `child`'s parent bound for its children's deaths (TASK-0333 6a).
    pub fn parent_exit_notify(&self, child: Pid) -> Option<crate::ipc::EndpointId> {
        let parent = self.task(child)?.parent()?;
        self.task(parent)?.exit_notify()
    }

    /// Clears every binding of `endpoint` as a child-death notifier — it died.
    pub fn forget_exit_notify(&mut self, endpoint: crate::ipc::EndpointId) {
        for task in self.tasks.iter_mut() {
            if task.exit_notify == Some(endpoint) {
                task.exit_notify = None;
            }
        }
    }

    /// Wakes `child`'s parent if it is blocked in `wait` for this child.
    pub fn wake_parent_waiter(&mut self, child: Pid, scheduler: &mut Scheduler) {
        let Some(child_task) = self.task(child) else {
            return;
        };
        let Some(parent) = child_task.parent() else {
            return;
        };
        let Some(parent_task) = self.task(parent) else {
            return;
        };
        if !parent_task.is_blocked() {
            return;
        }
        if let Some(BlockReason::WaitChild { target }) = parent_task.block_reason() {
            if target.is_none() || target == Some(child) {
                match self.wake(parent, scheduler) {
                    WakeOutcome::Woken
                    | WakeOutcome::WokenNoopSelftest
                    | WakeOutcome::TaskNotBlocked
                    | WakeOutcome::TaskNotFound
                    | WakeOutcome::EnqueueRejected => {}
                }
            }
        }
    }

    /// Attempts to reap a zombie child belonging to the current task.
    pub fn reap_child(
        &mut self,
        target: Option<Pid>,
        address_spaces: &mut AddressSpaceManager,
    ) -> Result<(Pid, i32, ExitReason), WaitError> {
        let parent_pid = self.current_pid();
        let parent_index = parent_pid.as_index();
        if parent_index >= self.tasks.len() {
            return Err(WaitError::NoChildren);
        }

        let children_snapshot = {
            let parent_task = self.tasks.get(parent_index).ok_or(WaitError::NoChildren)?;
            if parent_task.children.is_empty() {
                return Err(WaitError::NoChildren);
            }
            parent_task.children.clone()
        };

        let selected_pid = if let Some(pid) = target {
            if pid == parent_pid {
                return Err(WaitError::InvalidTarget);
            }
            if !children_snapshot.iter().any(|candidate| *candidate == pid) {
                return Err(WaitError::NoSuchPid);
            }
            pid
        } else {
            let mut found = None;
            for child_pid in &children_snapshot {
                if let Some(child_task) = self.tasks.get(child_pid.as_index()) {
                    if child_task.state == TaskState::Zombie && child_task.exit_code.is_some() {
                        found = Some(*child_pid);
                        break;
                    }
                }
            }
            match found {
                Some(pid) => pid,
                None => return Err(WaitError::WouldBlock),
            }
        };

        let child_index = selected_pid.as_index();
        if child_index >= self.tasks.len() {
            return Err(WaitError::NoSuchPid);
        }

        let (status, reason) = {
            let child_task = self.tasks.get(child_index).ok_or(WaitError::NoSuchPid)?;
            if child_task.parent != Some(parent_pid) {
                return Err(WaitError::NoSuchPid);
            }
            if child_task.state != TaskState::Zombie {
                return Err(WaitError::WouldBlock);
            }
            child_task.exit_code.ok_or(WaitError::WouldBlock)?
        };

        self.release_reaped(selected_pid, address_spaces);
        Ok((selected_pid, status, reason))
    }

    /// The release of a dead task: the parent's child list, the table, the address space (detached;
    /// destroyed when this task was its last owner), the orphaned objects (RFC-0100 §1). Returns
    /// whether the address space was destroyed here.
    ///
    /// TASK-0304 / TASK-0333 6d: a THREAD shares its space with the process leader and its
    /// siblings; `detach` releases this task's reference and `destroy` answers `InUse` while a
    /// co-owner lives — not an error, the space goes with the last owner.
    pub(crate) fn release_reaped(
        &mut self,
        pid: Pid,
        address_spaces: &mut AddressSpaceManager,
    ) -> bool {
        let parent = self.tasks.get(pid.as_index()).and_then(|t| t.parent);
        if let Some(parent_task) = parent.and_then(|p| self.tasks.get_mut(p.as_index())) {
            parent_task.children.retain(|child| *child != pid);
        }
        // The objects the task named at its exit and the ones its space maps — freed below once
        // the space is gone, unless another table names them or another space still maps them.
        let mut orphans = Vec::new();
        let mut freed = false;
        let mut threads_taken = 0u8;
        if let Some(task) = self.tasks.get_mut(pid.as_index()) {
            task.exit_code = None;
            task.parent = None;
            task.caps = CapTable::default();
            task.bootstrap_slot = None;
            task.frame = TrapFrame::default();
            task.children.clear();
            threads_taken = task.threads_taken;
            orphans = core::mem::take(&mut task.orphan_vmos);
            if let Some(handle) = task.address_space.take() {
                if let Ok(space) = address_spaces.get(handle) {
                    space.va_space().vmo_ids(&mut orphans);
                }
                if let Err(err) = address_spaces.detach(handle, pid) {
                    log_error!(
                        target: "task",
                        "TASK: detach failed pid={} err={:?}",
                        pid.as_raw(),
                        err
                    );
                }
                match address_spaces.destroy(handle) {
                    Ok(()) => freed = true,
                    Err(AddressSpaceError::InUse) => {}
                    Err(err) => {
                        log_error!(
                            target: "task",
                            "TASK: destroy as failed pid={} err={:?}",
                            pid.as_raw(),
                            err
                        );
                    }
                }
            }
        }
        // TASK-0333 6d (ADR-0082): a process is its address space — the leader's exit took its
        // threads, and the space they shared is gone with the last of them. The proof line
        // names the process (the leader's pid, or a thread's that outlived the leader's reap).
        if freed && threads_taken != 0 {
            log_info!(
                target: "task",
                "KSELFTEST: process exit takes threads ok (pid={} threads={} space=freed)",
                pid.as_raw(),
                threads_taken
            );
        }

        crate::mm::reclaim::orphans(self, address_spaces, &orphans, "reap");
        crate::mm::usage::report_growth(self, address_spaces);
        freed
    }
}
