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
    stage_fence: u32,
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
        crate::service_topology::CTRL_SLOTS.send,
    )
    .map_err(InitError::Abi)?;
    let child_recv_slot = nexus_abi::cap_transfer_to_slot(
        pid,
        ctrl_rsp_parent_slot,
        Rights::RECV,
        crate::service_topology::CTRL_SLOTS.recv,
    )
    .map_err(InitError::Abi)?;
    // ADR-0062: the boot-stage fence, with `Rights::WAIT` ALONE — a child may block for a stage,
    // never release one for the fleet (the kernel refuses its signal, TASK-0324 P5-a). Pinned
    // here, at spawn time, so it is in place before the task is ever resumed: the early-running
    // invariant holds by construction rather than by ordering luck.
    nexus_abi::cap_transfer_to_slot(
        pid,
        stage_fence,
        Rights::WAIT,
        crate::service_topology::STAGE_FENCE_SLOT,
    )
    .map_err(InitError::Abi)?;
    Ok((
        CtrlChannel::new(name, pid, ctrl_req_parent_slot, ctrl_rsp_parent_slot),
        child_send_slot,
        child_recv_slot,
    ))
}

/// A core service's sealed image (ADR-0080/0081): the bootfs object every spawn and respawn of
/// the service maps its text and rodata from. The kernel made it; init keeps the capability.
pub(crate) struct SealedImage {
    pub(crate) name: &'static str,
    /// init's capability slot of the sealed object.
    pub(crate) image: u32,
    pub(crate) stack_pages: usize,
    pub(crate) global_pointer: u64,
}

impl SealedImage {
    /// The launch record of a bootfs image — already sealed, nothing to make.
    pub(crate) fn of(image: &ServiceImage) -> Self {
        Self {
            name: image.name,
            image: image.image,
            stack_pages: image.stack_pages.max(1) as usize,
            global_pointer: image.global_pointer,
        }
    }
}

/// Spawn `image` at boot, remembering its launch record in `sealed` for every respawn.
/// ADR-0081: the launcher's images — bootfs files init never spawns — go into execd's declared
/// slots here, while it is still suspended (a respawn never comes through this path).
pub(crate) fn spawn_image(
    image: &ServiceImage,
    sealed: &mut alloc::vec::Vec<SealedImage>,
    probes_enabled: bool,
) -> Result<u32, InitError> {
    let sealed_image = SealedImage::of(image);
    let pid = spawn_service_with_probe(&sealed_image, probes_enabled)?;
    sealed.push(sealed_image);
    if image.name == "execd" {
        crate::bootstrap::bootfs::hand_off_to_execd(pid)?;
    }
    Ok(pid)
}

/// The pid of the service `name` among the attached control channels.
pub(crate) fn find_pid(ctrls: &[CtrlChannel], name: &str) -> Option<u32> {
    ctrls.iter().find(|c| c.svc_name == name).map(|c| c.pid)
}

/// Spawn with debug probe output (when probes are enabled).
pub(crate) fn spawn_service_with_probe(
    image: &SealedImage,
    probes_enabled: bool,
) -> Result<u32, InitError> {
    use crate::os_payload::{debug_write_byte, debug_write_bytes, debug_write_str};

    if probes_enabled {
        debug_write_bytes(b"!exec call name=");
        debug_write_str(image.name);
        debug_write_byte(b'\n');
    }
    // ADR-0082: a service is never killable — init supervises, it does not kill.
    let pid = nexus_abi::exec_vmo(
        image.image,
        image.stack_pages,
        image.global_pointer,
        image.name,
        false,
    )
    .map_err(InitError::Abi)?;
    if probes_enabled {
        debug_write_bytes(b"!exec ret\n");
    }
    Ok(pid)
}
