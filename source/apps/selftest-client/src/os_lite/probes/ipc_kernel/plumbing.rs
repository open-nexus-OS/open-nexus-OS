//! TASK-0023B P2-15: kernel-IPC plumbing probes (no security claims).
//!
//! These probes exercise the bootstrap endpoint and `KernelClient` plumbing
//! without asserting any cross-service security property. Marker emission is
//! left to the orchestrating phase (`phases::ipc_kernel`).
//!
//! Behavior is byte-for-byte identical to the pre-split implementation.

use nexus_abi::{
    ipc_recv_v1, ipc_recv_v1_nb, ipc_send_v1_nb, task_qos_get, task_qos_set_self, yield_,
    MsgHeader, QosClass,
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
    const BOOTSTRAP_EP: u32 = 0;
    const TY: u16 = 0x5a5a;
    const FLAGS: u16 = 0;
    let payload: &[u8] = b"nexus-ipc-v1 roundtrip";

    let header = MsgHeader::new(0, 0, TY, FLAGS, payload.len() as u32);
    ipc_send_v1_nb(BOOTSTRAP_EP, &header, payload).map_err(|_| ())?;

    // Be robust against minor scheduling variance: retry a few times if queue is empty.
    let mut out_hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut out_buf = [0u8; 64];
    for _ in 0..32 {
        match ipc_recv_v1_nb(BOOTSTRAP_EP, &mut out_hdr, &mut out_buf, true) {
            Ok(n) => {
                let n = n as usize;
                if out_hdr.ty != TY {
                    return Err(());
                }
                if out_hdr.len as usize != payload.len() {
                    return Err(());
                }
                if n != payload.len() {
                    return Err(());
                }
                if &out_buf[..n] != payload {
                    return Err(());
                }
                return Ok(());
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
    Err(())
}

pub(crate) fn ipc_deadline_timeout_probe() -> core::result::Result<(), ()> {
    // Blocking recv with a deadline in the past must return TimedOut deterministically.
    const BOOTSTRAP_EP: u32 = 0;
    let mut out_hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut out_buf = [0u8; 8];
    let sys_flags = 0; // blocking
    let deadline_ns = 1; // effectively always in the past
    match ipc_recv_v1(
        BOOTSTRAP_EP,
        &mut out_hdr,
        &mut out_buf,
        sys_flags,
        deadline_ns,
    ) {
        Err(nexus_abi::IpcError::TimedOut) => Ok(()),
        _ => Err(()),
    }
}

pub(crate) fn nexus_ipc_kernel_loopback_probe() -> core::result::Result<(), ()> {
    // NOTE: Service routing is not wired; this probes only the kernel-backed `KernelClient`
    // implementation by sending to the bootstrap endpoint queue and receiving the same frame.
    let client = KernelClient::new_with_slots(0, 0).map_err(|_| ())?;
    let payload: &[u8] = b"nexus-ipc kernel loopback";
    client.send(payload, IpcWait::NonBlocking).map_err(|_| ())?;
    // Bounded wait (avoid hangs): tolerate that the scheduler may reorder briefly.
    for _ in 0..128 {
        match client.recv(IpcWait::NonBlocking) {
            Ok(msg) if msg.as_slice() == payload => return Ok(()),
            Ok(_) => return Err(()),
            Err(nexus_ipc::IpcError::WouldBlock) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
    Err(())
}
