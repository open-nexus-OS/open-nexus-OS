// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![cfg_attr(all(feature = "os-lite", nexus_env = "os"), no_std)]

//! CONTEXT: nxfsd — the user-data filesystem provider (RFC-0071 / ADR-0043).
//! Exposes [`DataStore`]: owns a dedicated virtio-blk device, mounts-or-formats
//! an nxfs container (never a silent reformat), and answers the vfsd `/data`
//! provider protocol (list/stat/read + create/mkdir/writeText/rename/remove).
//!
//! v1 STAGING: `vfsd` hosts the `DataStore` IN-PROCESS as its `/data`
//! provider, so no new init service/endpoint/route is needed — just the
//! device MMIO grant. Extracting the store into a standalone `nxfsd` process
//! (ADR-0043 "one authority per store") is a follow-up: wrap `DataStore` in a
//! `KernelServer` loop and add the vfsd→nxfsd route. The store logic here is
//! process-boundary agnostic on purpose.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0293)
//! API_STABILITY: Unstable
//! ADR: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
extern crate alloc;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod store;
#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use store::{
    readdir_unavailable, stat_unavailable, write_unavailable, DataStore, DATA_MMIO_SLOT,
};
