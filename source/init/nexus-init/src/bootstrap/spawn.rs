// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service spawn logic — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

use crate::bootstrap::CtrlChannel;
use crate::os_payload::{InitError, ServiceImage, ENDPOINT_FACTORY_CAP_SLOT};
use nexus_abi::Rights;

/// Control-channel queue depth (the init↔service REQ/RSP pair).
pub(crate) const CTRL_EP_DEPTH: usize = 8;
/// The child's deterministic control slots (userspace `nexus-ipc` uses 1/2).
pub(crate) const CTRL_CHILD_SEND_SLOT: u32 = 1;
pub(crate) const CTRL_CHILD_RECV_SLOT: u32 = 2;

/// Creates the private control endpoints (REQ/RSP) for a freshly spawned
/// service and transfers them FIRST so the child sees them at slots 1/2
/// (deterministic slot assignment — the kernel IPC backend relies on it).
/// Init-owned endpoints (no `cap_clone`) keep the bring-up syscall count
/// low. Shared by the embedded spawn loop and the volume spawn pass
/// (TASK-0321) so both produce identical channels. Returns the channel and
/// the child's (send, recv) slots for the caller's diagnostics.
pub(crate) fn attach_ctrl_channel(
    name: &'static str,
    pid: u32,
) -> Result<(CtrlChannel, u32, u32), InitError> {
    let ctrl_req_parent_slot =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, CTRL_EP_DEPTH)
            .map_err(InitError::Abi)?;
    let ctrl_rsp_parent_slot =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, CTRL_EP_DEPTH)
            .map_err(InitError::Abi)?;
    let child_send_slot = nexus_abi::cap_transfer_to_slot(
        pid,
        ctrl_req_parent_slot,
        Rights::SEND,
        CTRL_CHILD_SEND_SLOT,
    )
    .map_err(InitError::Abi)?;
    let child_recv_slot = nexus_abi::cap_transfer_to_slot(
        pid,
        ctrl_rsp_parent_slot,
        Rights::RECV,
        CTRL_CHILD_RECV_SLOT,
    )
    .map_err(InitError::Abi)?;
    Ok((
        CtrlChannel::new(name, pid, ctrl_req_parent_slot, ctrl_rsp_parent_slot),
        child_send_slot,
        child_recv_slot,
    ))
}

/// Spawn with debug probe output (when probes are enabled).
pub(crate) fn spawn_service_with_probe(
    image: &ServiceImage,
    probes_enabled: bool,
) -> Result<u32, InitError> {
    use crate::os_payload::{debug_write_byte, debug_write_bytes, debug_write_str};

    if image.elf.is_empty() {
        return Err(InitError::MissingElf);
    }
    let stack_pages = image.stack_pages.max(1) as usize;
    if probes_enabled {
        debug_write_bytes(b"!exec call name=");
        debug_write_str(image.name);
        debug_write_byte(b'\n');
    }
    let pid = nexus_abi::exec_v2(image.elf, stack_pages, image.global_pointer, image.name)
        .map_err(InitError::Abi)?;
    if probes_enabled {
        debug_write_bytes(b"!exec ret\n");
    }
    Ok(pid)
}
