//! TASK-0023B P2-15: aggregator for kernel-IPC probes.
//!
//! Pre-split this file held all kernel-IPC probes (~393 LoC). It now
//! re-exports the same `pub(crate)` surface from focused submodules so all
//! orchestrating phases (`phases::bringup`, `phases::ipc_kernel`) keep
//! working unchanged via `probes::ipc_kernel::*`:
//!
//!   * [`plumbing`] -- bootstrap + `KernelClient` plumbing probes:
//!     `qos_probe`, `ipc_payload_roundtrip`, `ipc_deadline_timeout_probe`,
//!     `nexus_ipc_kernel_loopback_probe`.
//!   * [`security`] -- kernel-attested identity / cap-move probes:
//!     `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`.
//!   * [`soak`]     -- bounded-iteration stress mix: `ipc_soak_probe`.
//!
//! Behavior, marker timing, and IPC retry budgets are byte-for-byte identical
//! to the pre-split module; only file layout changed. The triplicated
//! `ReplyInboxV1` adapter is preserved verbatim and slated for consolidation
//! in P2-16 (move to `ipc/reply_inbox.rs`).

mod plumbing;
mod security;
mod soak;

pub(crate) use plumbing::{
    ipc_deadline_timeout_probe, ipc_payload_roundtrip, nexus_ipc_kernel_loopback_probe, qos_probe,
};
pub(crate) use security::{cap_move_reply_probe, sender_pid_probe, sender_service_id_probe};
pub(crate) use soak::ipc_soak_probe;
