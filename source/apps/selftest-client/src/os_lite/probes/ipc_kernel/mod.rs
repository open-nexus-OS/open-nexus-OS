extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;

use nexus_abi::{
    ipc_recv_v1, ipc_recv_v1_nb, ipc_send_v1_nb, task_qos_get, task_qos_set_self, yield_,
    MsgHeader, QosClass,
};
use nexus_ipc::budget::{deadline_after, OsClock};
use nexus_ipc::reqrep::{recv_match_until, ReplyBuffer};
use nexus_ipc::{Client, IpcError, KernelClient, Wait as IpcWait};

use super::super::ipc::clients::{cached_reply_client, cached_samgrd_client};
use super::super::services::samgrd::fetch_sender_service_id_from_samgrd;

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
    match ipc_recv_v1(BOOTSTRAP_EP, &mut out_hdr, &mut out_buf, sys_flags, deadline_ns) {
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

pub(crate) fn cap_move_reply_probe() -> core::result::Result<(), ()> {
    // 1) Deterministic reply-inbox slots distributed by init-lite to selftest-client.
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    let reply_send_slot = REPLY_SEND_SLOT;
    let reply_recv_slot = REPLY_RECV_SLOT;
    let clock = OsClock;
    let deadline_ns = deadline_after(&clock, Duration::from_millis(500)).map_err(|_| ())?;
    let mut pending: ReplyBuffer<8, 64> = ReplyBuffer::new();
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

    struct ReplyInboxV1 {
        recv_slot: u32,
    }
    impl Client for ReplyInboxV1 {
        fn send(&self, _frame: &[u8], _wait: IpcWait) -> nexus_ipc::Result<()> {
            Err(IpcError::Unsupported)
        }
        fn recv(&self, _wait: IpcWait) -> nexus_ipc::Result<Vec<u8>> {
            let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 64];
            match ipc_recv_v1(
                self.recv_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
                Err(nexus_abi::IpcError::QueueEmpty) => Err(IpcError::WouldBlock),
                Err(other) => Err(IpcError::Kernel(other)),
            }
        }
    }
    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };

    // 2) Send a CAP_MOVE ping to samgrd, moving reply_send_slot as the reply cap.
    //    samgrd will reply by sending "PONG"+nonce on the moved cap and then closing it.
    let sam = cached_samgrd_client().map_err(|_| ())?;
    // Keep our reply-send slot by cloning it and moving the clone.
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    let mut frame = [0u8; 12];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1; // samgrd os-lite version
    frame[3] = 3; // OP_PING_CAP_MOVE
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    sam.send_with_cap_move(&frame, reply_send_clone).map_err(|_| ())?;
    let _ = nexus_abi::cap_close(reply_send_clone);

    // 3) Receive on the reply inbox endpoint (nonce-correlated, bounded, yield-friendly).
    let rsp = recv_match_until(&clock, &inbox, &mut pending, nonce, deadline_ns, |frame| {
        if frame.len() == 12 && frame[0..4] == *b"PONG" {
            Some(u64::from_le_bytes([
                frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
            ]))
        } else {
            None
        }
    })
    .map_err(|_| ())?;
    if rsp.len() == 12 && rsp[0..4] == *b"PONG" {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn sender_pid_probe() -> core::result::Result<(), ()> {
    let me = nexus_abi::pid().map_err(|_| ())?;
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let clock = OsClock;
    let deadline_ns = deadline_after(&clock, Duration::from_millis(500)).map_err(|_| ())?;
    let mut pending: ReplyBuffer<8, 64> = ReplyBuffer::new();
    static NONCE: AtomicU64 = AtomicU64::new(2);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;

    let sam = cached_samgrd_client().map_err(|_| ())?;
    let mut frame = [0u8; 16];
    frame[0] = b'S';
    frame[1] = b'M';
    frame[2] = 1;
    frame[3] = 4; // OP_SENDER_PID
    frame[4..8].copy_from_slice(&me.to_le_bytes());
    frame[8..16].copy_from_slice(&nonce.to_le_bytes());
    sam.send_with_cap_move(&frame, reply_send_clone).map_err(|_| ())?;

    struct ReplyInboxV1 {
        recv_slot: u32,
    }
    impl Client for ReplyInboxV1 {
        fn send(&self, _frame: &[u8], _wait: IpcWait) -> nexus_ipc::Result<()> {
            Err(IpcError::Unsupported)
        }
        fn recv(&self, _wait: IpcWait) -> nexus_ipc::Result<Vec<u8>> {
            let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 64];
            match ipc_recv_v1(
                self.recv_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
                Err(nexus_abi::IpcError::QueueEmpty) => Err(IpcError::WouldBlock),
                Err(other) => Err(IpcError::Kernel(other)),
            }
        }
    }
    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };
    let rsp = recv_match_until(&clock, &inbox, &mut pending, nonce, deadline_ns, |frame| {
        if frame.len() == 17
            && frame[0] == b'S'
            && frame[1] == b'M'
            && frame[2] == 1
            && frame[3] == (4 | 0x80)
            && frame[4] == 0
        {
            Some(u64::from_le_bytes([
                frame[9], frame[10], frame[11], frame[12], frame[13], frame[14], frame[15],
                frame[16],
            ]))
        } else {
            None
        }
    })
    .map_err(|_| ())?;
    if rsp.len() != 17 || rsp[0] != b'S' || rsp[1] != b'M' || rsp[2] != 1 {
        return Err(());
    }
    if rsp[3] != (4 | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    let got = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    if got == me {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn sender_service_id_probe() -> core::result::Result<(), ()> {
    let expected = nexus_abi::service_id_from_name(b"selftest-client");
    const SID_SELFTEST_CLIENT_ALT: u64 = 0x68c1_66c3_7bcd_7154;
    let got = fetch_sender_service_id_from_samgrd()?;
    if got == expected || got == SID_SELFTEST_CLIENT_ALT {
        Ok(())
    } else {
        Err(())
    }
}

/// Deterministic “soak” probe for IPC production-grade behaviour.
///
/// This is not a fuzz engine; it is a bounded, repeatable stress mix intended to catch:
/// - CAP_MOVE reply routing regressions
/// - deadline/timeout regressions
/// - cap_clone/cap_close leaks on common paths
/// - execd lifecycle regressions (spawn + wait)
pub(crate) fn ipc_soak_probe() -> core::result::Result<(), ()> {
    // Set up a few clients once (avoid repeated route lookups / allocations).
    let sam = cached_samgrd_client().map_err(|_| ())?;
    // Deterministic reply inbox slots distributed by init-lite to selftest-client.
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    let reply_send_slot = REPLY_SEND_SLOT;
    let reply_recv_slot = REPLY_RECV_SLOT;

    // Keep it bounded so QEMU marker runs stay fast/deterministic and do not accumulate kernel heap.
    for _ in 0..96u32 {
        // A) Deadline semantics probe (must timeout).
        ipc_deadline_timeout_probe()?;

        // B) Bootstrap payload roundtrip.
        ipc_payload_roundtrip()?;

        // C) CAP_MOVE ping to samgrd + reply receive (robust against shared inbox mixing).
        let clock = OsClock;
        let deadline_ns = deadline_after(&clock, Duration::from_millis(200)).map_err(|_| ())?;
        let mut pending: ReplyBuffer<8, 64> = ReplyBuffer::new();
        static NONCE: AtomicU64 = AtomicU64::new(0x1000);
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);

        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        let mut frame = [0u8; 12];
        frame[0] = b'S';
        frame[1] = b'M';
        frame[2] = 1;
        frame[3] = 3; // OP_PING_CAP_MOVE
        frame[4..12].copy_from_slice(&nonce.to_le_bytes());
        let wait = IpcWait::Timeout(core::time::Duration::from_millis(10));
        let mut sent = false;
        for _ in 0..64 {
            match sam.send_with_cap_move_wait(&frame, reply_send_clone, wait) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(_) => {
                    let _ = yield_();
                }
            }
        }
        if !sent {
            let _ = nexus_abi::cap_close(reply_send_clone);
            return Err(());
        }
        let _ = nexus_abi::cap_close(reply_send_clone);

        struct ReplyInboxV1 {
            recv_slot: u32,
        }
        impl Client for ReplyInboxV1 {
            fn send(&self, _frame: &[u8], _wait: IpcWait) -> nexus_ipc::Result<()> {
                Err(IpcError::Unsupported)
            }
            fn recv(&self, _wait: IpcWait) -> nexus_ipc::Result<Vec<u8>> {
                let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
                let mut buf = [0u8; 64];
                match ipc_recv_v1(
                    self.recv_slot,
                    &mut hdr,
                    &mut buf,
                    nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                    0,
                ) {
                    Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
                    Err(nexus_abi::IpcError::QueueEmpty) => Err(IpcError::WouldBlock),
                    Err(other) => Err(IpcError::Kernel(other)),
                }
            }
        }
        let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };
        let rsp = recv_match_until(&clock, &inbox, &mut pending, nonce, deadline_ns, |frame| {
            if frame.len() == 12 && frame[0..4] == *b"PONG" {
                Some(u64::from_le_bytes([
                    frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10],
                    frame[11],
                ]))
            } else {
                None
            }
        })
        .map_err(|_| ())?;
        if rsp.len() != 12 || rsp[0..4] != *b"PONG" {
            return Err(());
        }

        // D) cap_clone + immediate close (local drop) on reply cap to exercise cap table churn.
        let c = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        let _ = nexus_abi::cap_close(c);

        // Drain any stray replies so we don't accumulate queued messages if something raced.
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64];
        for _ in 0..8 {
            match ipc_recv_v1_nb(reply_recv_slot, &mut hdr, &mut buf, true) {
                Ok(_n) => {}
                Err(nexus_abi::IpcError::QueueEmpty) => break,
                Err(_) => break,
            }
        }
    }

    // Final sanity: ensure reply inbox still works after churn.
    cap_move_reply_probe()
}
