// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's live device table (TASK-0246 P1, RFC-0098 C4) — the one owner
//! of each device's description (`crate::dma_reach::DeviceTable`, host-proven).
//! `device_cap_create` registers init's descriptor here and the capability names the
//! record (`DeviceMmio { dev, .. }`); `vmo_create` allocates a device's memory within
//! the record's DMA reach and `vmo_runs` answers in its bus addresses.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the table rules are host-tested in `crate::dma_reach`; the wiring by
//!   `KSELFTEST: vmo reach ok (…)` and every driver's first DMA on every boot
//! INVARIANTS: one table, one lock (a leaf: never held across an allocation or a log
//!   line); a record is immutable once registered and never retired while a
//!   capability names it.

use crate::dma_reach::{DeviceDesc, DeviceId, DeviceTable, DmaReach, TableError};

#[cfg(debug_assertions)]
type TableLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type TableLock<T> = spin::Mutex<T>;

static TABLE: TableLock<DeviceTable> = TableLock::new(DeviceTable::new());

/// The record for `desc` (the existing one when the window is already described
/// identically).
pub fn register(desc: DeviceDesc) -> Result<DeviceId, TableError> {
    TABLE.lock().register(desc)
}

/// What the device `dev` can address (`None`: no such record).
pub fn reach(dev: DeviceId) -> Option<DmaReach> {
    TABLE.lock().get(dev).map(|d| d.reach)
}

/// Drop a record no capability names (the kernel selftest's synthetic devices).
pub fn retire(dev: DeviceId) -> Result<(), TableError> {
    TABLE.lock().retire(dev)
}
