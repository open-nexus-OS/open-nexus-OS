extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::{yield_, MsgHeader};
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

pub(crate) fn policy_check(client: &KernelClient, subject: &str) -> core::result::Result<bool, ()> {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_CHECK: u8 = 1;
    const STATUS_ALLOW: u8 = 0;
    const STATUS_DENY: u8 = 1;
    const STATUS_MALFORMED: u8 = 2;
    let name = subject.as_bytes();
    if name.len() > 48 {
        return Err(());
    }
    let mut frame = Vec::with_capacity(5 + name.len());
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION);
    frame.push(OP_CHECK);
    frame.push(name.len() as u8);
    frame.extend_from_slice(name);
    // Avoid deadline-based blocking IPC (bring-up flakiness); use bounded NONBLOCK loops.
    let (send_slot, recv_slot) = client.slots();
    let hdr = MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(2_000_000_000); // 2s
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v1(
            recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n != 6 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
                    continue;
                }
                if buf[3] != (OP_CHECK | 0x80) {
                    continue;
                }
                return match buf[4] {
                    STATUS_ALLOW => Ok(true),
                    STATUS_DENY => Ok(false),
                    STATUS_MALFORMED => Err(()),
                    _ => Err(()),
                };
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        j = j.wrapping_add(1);
    }
}

pub(crate) fn policyd_check_cap(
    policyd: &KernelClient,
    subject: &str,
    cap: &str,
) -> core::result::Result<bool, ()> {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_CHECK_CAP: u8 = 4;
    const STATUS_ALLOW: u8 = 0;

    let subject_id = nexus_abi::service_id_from_name(subject.as_bytes());
    let cap_b = cap.as_bytes();
    if cap_b.is_empty() || cap_b.len() > 48 {
        return Err(());
    }
    let mut req = alloc::vec::Vec::with_capacity(4 + 8 + 1 + cap_b.len());
    req.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_CHECK_CAP]);
    req.extend_from_slice(&subject_id.to_le_bytes());
    req.push(cap_b.len() as u8);
    req.extend_from_slice(cap_b);

    policyd.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(100))).map_err(|_| ())?;
    let rsp =
        policyd.recv(IpcWait::Timeout(core::time::Duration::from_millis(100))).map_err(|_| ())?;
    if rsp.len() != 5 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
        return Err(());
    }
    if rsp[3] != (OP_CHECK_CAP | 0x80) {
        return Err(());
    }
    Ok(rsp[4] == STATUS_ALLOW)
}

pub(crate) fn keystored_sign_denied(keystored: &KernelClient) -> core::result::Result<(), ()> {
    const MAGIC0: u8 = b'K';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_SIGN: u8 = 5;
    const STATUS_DENY: u8 = 5;

    let payload = [0u8; 8];
    let mut frame = Vec::with_capacity(8 + payload.len());
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_SIGN]);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);

    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(
        &clock,
        keystored,
        &frame,
        core::time::Duration::from_millis(200),
    )
    .map_err(|_| ())?;
    let rsp =
        nexus_ipc::budget::recv_budgeted(&clock, keystored, core::time::Duration::from_millis(200))
            .map_err(|_| ())?;
    if rsp.len() == 7 && rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION {
        if rsp[3] == (OP_SIGN | 0x80) && rsp[4] == STATUS_DENY {
            return Ok(());
        }
    }
    Err(())
}

pub(crate) fn policyd_requester_spoof_denied(
    policyd: &KernelClient,
) -> core::result::Result<(), ()> {
    // Direct policyd v3 call from selftest-client: try to claim requester_id=demo.testsvc.
    // policyd must override/deny because requester_id must match sender_service_id unless caller is init-lite.
    let nonce: nexus_abi::policyd::Nonce = 0xA1B2C3D4;
    let spoof = nexus_abi::service_id_from_name(b"demo.testsvc");
    let target = nexus_abi::service_id_from_name(b"samgrd");
    let mut frame = [0u8; 64];
    let n = nexus_abi::policyd::encode_route_v3_id(nonce, spoof, target, &mut frame).ok_or(())?;
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(
        &clock,
        policyd,
        &frame[..n],
        core::time::Duration::from_secs(2),
    )
    .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, policyd, core::time::Duration::from_secs(2))
        .map_err(|_| ())?;
    let (_ver, _op, rsp_nonce, status) = nexus_abi::policyd::decode_rsp_v2_or_v3(&rsp).ok_or(())?;
    if rsp_nonce != nonce {
        return Err(());
    }
    if status == 1 {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn policyd_fetch_abi_profile(
    policyd: &KernelClient,
    expected_subject_id: u64,
) -> core::result::Result<nexus_abi::abi_filter::AbiProfile, ()> {
    let (send_slot, recv_slot) = policyd.slots();
    let mut req = [0u8; 32];
    let nonce: nexus_abi::policyd::Nonce = 0xB17E_0019;
    let req_len =
        nexus_abi::policyd::encode_abi_profile_get_v2(nonce, expected_subject_id, &mut req)
            .ok_or(())?;
    let hdr = MsgHeader::new(0, 0, 0, 0, req_len as u32);
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(2_000_000_000);
    let mut send_tries = 0usize;
    loop {
        match nexus_abi::ipc_send_v1(
            send_slot,
            &hdr,
            &req[..req_len],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        ) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (send_tries & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        send_tries = send_tries.wrapping_add(1);
    }

    let authority_id = nexus_abi::service_id_from_name(b"policyd");
    let mut recv_tries = 0usize;
    let mut recv_hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut sender_service_id = 0u64;
    let mut rsp_buf = [0u8; 12 + nexus_abi::abi_filter::MAX_PROFILE_BYTES];
    loop {
        if (recv_tries & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| ())?;
            if now >= deadline {
                return Err(());
            }
        }
        match nexus_abi::ipc_recv_v2(
            recv_slot,
            &mut recv_hdr,
            &mut rsp_buf,
            &mut sender_service_id,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, rsp_buf.len());
                let rsp = &rsp_buf[..n];
                let (rsp_nonce, status, profile_bytes) =
                    match nexus_abi::policyd::decode_abi_profile_rsp_v2(rsp) {
                        Some(v) => v,
                        None => continue,
                    };
                if rsp_nonce != nonce {
                    continue;
                }
                if status != nexus_abi::policyd::STATUS_ALLOW {
                    return Err(());
                }
                return nexus_abi::abi_filter::ingest_distributed_profile_v1_typed(
                    profile_bytes,
                    nexus_abi::abi_filter::SenderServiceId::new(sender_service_id),
                    nexus_abi::abi_filter::AuthorityServiceId::new(authority_id),
                    nexus_abi::abi_filter::SubjectServiceId::new(expected_subject_id),
                )
                .map_err(|_| ());
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        recv_tries = recv_tries.wrapping_add(1);
    }
}
