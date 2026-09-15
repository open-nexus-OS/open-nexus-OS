// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Aggregator for kernel-IPC probes. Re-exports the same
//!   `pub(crate)` surface (`qos_probe`, `ipc_payload_roundtrip`,
//!   `nexus_ipc_kernel_loopback_probe`,
//!   `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`,
//!   `ipc_soak_probe`, `ipc_bench_probe`) from focused submodules.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — kernel IPC slice.
//!
//! Sub-split landed in TASK-0023B Cut P2-15. Pre-split this file held all
//! kernel-IPC probes (~393 LoC); it now contains only `mod` declarations and
//! re-exports so the orchestrating phases (`phases::bringup`,
//! `phases::ipc_kernel`) keep working unchanged via `probes::ipc_kernel::*`:
//!
//!   * [`plumbing`] -- bootstrap + `KernelClient` plumbing probes.
//!   * [`security`] -- kernel-attested identity / cap-move probes.
//!   * [`soak`]     -- bounded-iteration stress mix.
//!   * [`bench`]    -- request/reply round-trip number (TASK-0054C P1).
//!
//! Behavior, marker timing, and IPC retry budgets are byte-for-byte identical
//! to the pre-split module. The previously-triplicated `ReplyInboxV1` adapter
//! was consolidated into `crate::os_lite::ipc::reply_inbox` in Cut P2-16.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

mod bench;
mod plumbing;
mod security;
mod soak;

pub(crate) use bench::{ipc_bench_probe, BenchResult};
pub(crate) use plumbing::{ipc_payload_roundtrip, nexus_ipc_kernel_loopback_probe, qos_probe};
pub(crate) use security::{cap_move_reply_probe, sender_pid_probe, sender_service_id_probe};
pub(crate) use soak::ipc_soak_probe;
