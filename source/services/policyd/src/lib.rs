// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: policyd daemon – capability policy checks via IPC
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 unit tests (supply_chain) + QEMU marker ladder (os_lite)
//! ADR: docs/adr/0014-policy-architecture.md

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod abi_host_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod audit_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(any(test, feature = "os-lite"))]
pub mod abi_eval;
#[cfg(any(test, feature = "os-lite"))]
pub mod abi_learn;
#[cfg(test)]
mod abi_learn_roundtrip_tests;
#[cfg(any(test, feature = "os-lite"))]
pub mod abi_mode;
#[cfg(any(test, feature = "os-lite"))]
pub mod abi_profile;
#[cfg(test)]
#[path = "../../../../userspace/policy/src/learn_gen.rs"]
#[allow(dead_code)]
mod learn_gen;
/// RFC-0091 learn record format — the SAME file the host `policy` crate
/// compiles (`userspace/policy/src/learn_record.rs`), included by path so
/// the OS writer and the host reader cannot drift.
#[cfg(any(test, feature = "os-lite"))]
#[path = "../../../../userspace/policy/src/learn_record.rs"]
#[allow(dead_code)]
pub mod learn_record;
#[cfg(any(test, feature = "os-lite"))]
pub mod lite_protocol;
/// Host-only test helpers: the shared schema + generator (by path) for the
/// learn → generate → parse → evaluate roundtrip proof.
#[cfg(test)]
#[path = "../../../../userspace/policy/src/schema.rs"]
#[allow(dead_code)]
mod schema;

pub mod supply_chain;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
extern crate nexus_policy;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
