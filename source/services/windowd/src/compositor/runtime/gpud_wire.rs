// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the gpud wire helpers: the IPC error loggers
//! (generic + cap-sensitive) and the damage/attach frame encoders. Split out of
//! `runtime/mod.rs` (structure ratchet, TASK-0066); pure path plumbing, no behavior change.
//! OWNERS: @ui @runtime
//! STATUS: Experimental

use super::{GPUD_WIRED_RECV_SLOT, GPUD_WIRED_SEND_SLOT};
use crate::compositor::damage::DamageRect;
use nexus_abi::debug_println;

pub(super) fn log_gpud_ipc_error(prefix: &str, err: nexus_ipc::IpcError) {
    let label = match err {
        nexus_ipc::IpcError::WouldBlock => "would-block",
        nexus_ipc::IpcError::Timeout => "timeout",
        nexus_ipc::IpcError::Disconnected => "disconnected",
        nexus_ipc::IpcError::NoSpace => "no-space",
        nexus_ipc::IpcError::Unsupported => "unsupported",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint) => "kernel-no-endpoint",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::QueueFull) => "kernel-queue-full",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::QueueEmpty) => "kernel-queue-empty",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied) => {
            "kernel-permission-denied"
        }
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::TimedOut) => "kernel-timeout",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSpace) => "kernel-no-space",
        nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::Unsupported) => "kernel-unsupported",
        _ => "other",
    };
    let _ = debug_println(&alloc::format!("{prefix} {label}"));
}

/// Like [`log_gpud_ipc_error`] but for the cap-sensitive gpud sends (the VMO
/// cap-move handoff + present). On a `kernel-permission-denied` — the classic
/// "the cap at this slot lacks SEND, or the send slot points at the wrong cap" —
/// it names the gpud SEND slot and the slot contract, so a future cap regression
/// (e.g. init displacing the gpud caps off slots 5/6) is diagnosable from one
/// boot line instead of a log dig. Other errors defer to the generic logger.
pub(super) fn log_gpud_cap_error(prefix: &str, err: nexus_ipc::IpcError, send_slot: u32) {
    if matches!(err, nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied)) {
        let _ = debug_println(&alloc::format!(
            "{prefix} kernel-permission-denied (gpud send_slot={send_slot}: cap lacks SEND or slot \
             points at the wrong cap — windowd→gpud handoff contract is slots \
             {GPUD_WIRED_SEND_SLOT}/{GPUD_WIRED_RECV_SLOT}, declared in nexus-service-topology — \
             look for init: FAIL declared slot)"
        ));
    } else {
        log_gpud_ipc_error(prefix, err);
    }
}

pub(super) fn encode_gpud_damage_frame(
    rect: DamageRect,
) -> [u8; nexus_display_proto::DAMAGE_FRAME_LEN] {
    nexus_display_proto::encode_damage_frame(rect.x, rect.y, rect.width, rect.height)
}

pub(super) fn encode_gpud_attach_frame(handoff_id: u32) -> [u8; 5] {
    nexus_display_proto::encode_attach_frame(handoff_id)
}
