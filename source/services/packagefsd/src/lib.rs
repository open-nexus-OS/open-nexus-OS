// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

//! CONTEXT: Read-only package file system registry service
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), ReadyNotifier
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp)
//! INVARIANTS: Separate from Vfsd (dispatcher) and BundleMgr; stable readiness prints
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
