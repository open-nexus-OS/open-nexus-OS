// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-IPC plumbing probes (no security claims) — `qos_probe`,
//!   `ipc_payload_roundtrip`,
//!   `nexus_ipc_kernel_loopback_probe`. Exercise the bootstrap endpoint and
//!   `KernelClient` plumbing without asserting any cross-service security
//!   property.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup + ipc_kernel phases.
//!
//! Marker emission is left to the orchestrating phase (`phases::ipc_kernel`).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md
//!
//! The loopback probes WAIT for their echo (a blocking recv with a deadline, TASK-0324
//! P7) — on a self-loopback the frame is queued before the send returns, so the 32- and
//! 128-round "scheduling variance" retries they used to carry never retried anything.

use nexus_abi::{
    ipc_recv_v1, ipc_send_v1_nb, task_qos_get, task_qos_set_self, MsgHeader, QosClass,
    IPC_SYS_TRUNCATE,
};
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

pub(crate) fn qos_probe() -> core::result::Result<(), ()> {
    let current = task_qos_get().map_err(|_| ())?;
    if current != QosClass::Normal {
        return Err(());
    }
    // Exercise the set path without perturbing scheduler behavior for later probes.
    task_qos_set_self(current).map_err(|_| ())?;
    let got = task_qos_get().map_err(|_| ())?;
    if got != current {
        return Err(());
    }

    let higher = match current {
        QosClass::Idle => Some(QosClass::Normal),
        QosClass::Normal => Some(QosClass::Interactive),
        QosClass::Interactive => Some(QosClass::PerfBurst),
        QosClass::PerfBurst => None,
    };
    if let Some(next) = higher {
        match task_qos_set_self(next) {
            Err(nexus_abi::AbiError::CapabilityDenied) => {}
            _ => return Err(()),
        }
        let after = task_qos_get().map_err(|_| ())?;
        if after != current {
            return Err(());
        }
    }

    Ok(())
}

pub(crate) fn ipc_payload_roundtrip() -> core::result::Result<(), ()> {
    // NOTE: Slot 0 is the bootstrap endpoint capability passed by init-lite (SEND|RECV).
    const BOOTSTRAP_EP: u32 = nexus_abi::BOOTSTRAP_CAP_SLOT;
    const TY: u16 = 0x5a5a;
    const FLAGS: u16 = 0;
    let payload: &[u8] = b"nexus-ipc-v1 roundtrip";

    let header = MsgHeader::new(0, 0, TY, FLAGS, payload.len() as u32);
    ipc_send_v1_nb(BOOTSTRAP_EP, &header, payload).map_err(|_| ())?;

    let mut out_hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut out_buf = [0u8; 64];
    // The echo is queued before the send returned (self-loopback): ONE waited receive.
    let n = ipc_recv_v1(BOOTSTRAP_EP, &mut out_hdr, &mut out_buf, IPC_SYS_TRUNCATE, 0)
        .map_err(|_| ())? as usize;
    if out_hdr.ty != TY || out_hdr.len as usize != payload.len() || n != payload.len() {
        return Err(());
    }
    if &out_buf[..n] != payload {
        return Err(());
    }
    Ok(())
}

pub(crate) fn nexus_ipc_kernel_loopback_probe() -> core::result::Result<(), ()> {
    // NOTE: Service routing is not wired; this probes only the kernel-backed `KernelClient`
    // implementation by sending to the bootstrap endpoint queue and receiving the same frame.
    let bootstrap = nexus_abi::BOOTSTRAP_CAP_SLOT;
    let client = KernelClient::new_with_slots(bootstrap, bootstrap).map_err(|_| ())?;
    let payload: &[u8] = b"nexus-ipc kernel loopback";
    client.send(payload, IpcWait::NonBlocking).map_err(|_| ())?;
    match client.recv(IpcWait::Blocking) {
        Ok(msg) if msg.as_slice() == payload => Ok(()),
        _ => Err(()),
    }
}
