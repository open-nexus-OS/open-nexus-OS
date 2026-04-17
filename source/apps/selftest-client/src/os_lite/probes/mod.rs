// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Aggregator for focused proof primitives — small, behavior-only
//!   helpers consumed by the orchestrating phases (`phases::*`). Each probe
//!   returns a `Result` and emits no markers itself; the calling phase owns
//!   the marker ladder.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Indirect via QEMU marker ladder (just test-os).
//!
//! Submodules:
//!   * `rng`           — kernel RNG entropy probe (TASK-0006).
//!   * `device_key`    — keystored device-key public-export + private-export reject.
//!   * `elf`           — header sanity probe over the embedded `HELLO_ELF` payload.
//!   * `core_service`  — generic "is this core service answering?" probe (logd evidence).
//!   * `ipc_kernel`    — kernel-IPC plumbing / security / soak probes (RFC-0005).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

pub(crate) mod core_service;
pub(crate) mod device_key;
pub(crate) mod elf;
pub(crate) mod ipc_kernel;
pub(crate) mod rng;
