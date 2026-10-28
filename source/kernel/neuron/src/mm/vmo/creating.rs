// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: objects between a create's phases (TASK-0333 6a, RFC-0100 §1): reserved and being
//! zeroed with the kernel lock dropped, named by no capability yet and mapped nowhere — exactly
//! what the lifetime rule would free. `set_creating` marks them from the reserve to the finish,
//! and `reclaim` leaves a marked object alone. One slot per hart at most is in that window. A
//! child module of `vmo` (split out under the module-size ratchet when 6f-1 grew the commit
//! path); re-exported as `vmo::set_creating` / `vmo::is_creating`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: vm reclaim (`, every `vmo_create` of every boot

use super::{TableLock, VmoId};

const CREATING_SLOTS: usize = crate::smp::MAX_CPUS * 2;
static CREATING: TableLock<[VmoId; CREATING_SLOTS]> = TableLock::new([0; CREATING_SLOTS]);

/// Mark (`on`) or clear the in-creation state of `id`.
pub fn set_creating(id: VmoId, on: bool) {
    let mut set = CREATING.lock();
    if on {
        if let Some(slot) = set.iter_mut().find(|s| **s == 0) {
            *slot = id;
        }
    } else {
        for slot in set.iter_mut().filter(|s| **s == id) {
            *slot = 0;
        }
    }
}

/// Whether `id` is between a create's phases.
pub fn is_creating(id: VmoId) -> bool {
    id != 0 && CREATING.lock().contains(&id)
}
