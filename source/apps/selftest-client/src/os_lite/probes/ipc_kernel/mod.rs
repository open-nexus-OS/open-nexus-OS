// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Aggregator for kernel-IPC probes. Re-exports the same
//!   `pub(crate)` surface (`qos_probe`, `ipc_payload_roundtrip`,
//!   `ipc_deadline_timeout_probe`, `nexus_ipc_kernel_loopback_probe`,
//!   `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`,
//!   `vmo_share_probe`, `ipc_soak_probe`) from focused submodules.
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
//!
//! Behavior, marker timing, and IPC retry budgets are byte-for-byte identical
//! to the pre-split module. The previously-triplicated `ReplyInboxV1` adapter
//! was consolidated into `crate::os_lite::ipc::reply_inbox` in Cut P2-16.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

mod plumbing;
mod security;
mod soak;
mod vmo;

pub(crate) use plumbing::{
    ipc_deadline_timeout_probe, ipc_payload_roundtrip, nexus_ipc_kernel_loopback_probe, qos_probe,
};
pub(crate) use security::{cap_move_reply_probe, sender_pid_probe, sender_service_id_probe};
pub(crate) use soak::ipc_soak_probe;
pub(crate) use vmo::vmo_share_probe;
