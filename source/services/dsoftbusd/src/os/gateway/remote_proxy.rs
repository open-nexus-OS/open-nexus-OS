//! Remote proxy service ID constants.

use alloc::vec::Vec;
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::{KernelClient, Wait};

use crate::os::netstack::{stream_read_exact, stream_write_all, SessionId};
use crate::os::service_clients;
use crate::os::session::records::{MAX_REQ, MAX_RSP, REQ_CIPH, REQ_PLAIN, RSP_CIPH, RSP_PLAIN};

pub(crate) const SVC_SAMGR_RESOLVE_STATUS: u8 = 1;
pub(crate) const SVC_BUNDLE_LIST: u8 = 2;

pub(crate) fn run_remote_proxy_loop(
    transport: &mut nexus_noise_xk::Transport,
    pending_replies: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    sid: SessionId,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<(), ()> {
    let samgrd = service_clients::cached_client_slots_bounded(
        "samgrd",
        &service_clients::SAMGRD_SEND_SLOT_CACHE,
        &service_clients::SAMGRD_RECV_SLOT_CACHE,
        128,
    )
    .ok_or(())?;
    let bundlemgrd = service_clients::cached_client_slots_bounded(
        "bundlemgrd",
        &service_clients::BUNDLEMGRD_SEND_SLOT_CACHE,
        &service_clients::BUNDLEMGRD_RECV_SLOT_CACHE,
        128,
    )
    .ok_or(())?;
    let _ = nexus_abi::debug_println("dsoftbusd: remote proxy up");
    let mut rx_logged = false;
    let mut proxy_io_retry_logged = false;
    loop {
        let mut ciph = [0u8; REQ_CIPH];
        if stream_read_exact(
            pending_replies,
            nonce_ctr,
            net,
            sid,
            &mut ciph,
            reply_recv_slot,
            reply_send_slot,
        )
        .is_err()
        {
            if !proxy_io_retry_logged {
                proxy_io_retry_logged = true;
                let _ = nexus_abi::debug_println("dbg:dsoftbusd: proxy io retry");
            }
            let _ = nexus_abi::yield_();
            continue;
        }
        if !rx_logged {
            let _ = nexus_abi::debug_println("dsoftbusd: remote proxy rx");
            rx_logged = true;
        }
        let mut plain = [0u8; REQ_PLAIN];
        let n = match transport.decrypt(&ciph, &mut plain) {
            Ok(v) => v,
            Err(_) => {
                continue;
            }
        };
        if n != REQ_PLAIN {
            let _ = nexus_abi::debug_println("dsoftbusd: remote proxy denied (malformed)");
            continue;
        }
        let svc = plain[0];
        let used = u16::from_le_bytes([plain[1], plain[2]]) as usize;
        if used > MAX_REQ {
            let _ = nexus_abi::debug_println("dsoftbusd: remote proxy denied (oversized)");
            continue;
        }
        let req = &plain[3..3 + used];

        let mut status = 0u8;
        let mut rsp_payload: Vec<u8> = Vec::new();
        match svc {
            SVC_SAMGR_RESOLVE_STATUS => {
                if req.len() < 5 || req[0] != b'S' || req[1] != b'M' || req[2] != 1 {
                    status = 1;
                } else {
                    // CAP_MOVE reply: move a cloned reply SEND cap so samgrd can respond on it.
                    let cap = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
                    samgrd
                        .send_with_cap_move_wait(
                            req,
                            cap,
                            Wait::Timeout(core::time::Duration::from_millis(300)),
                        )
                        .map_err(|_| {
                            let _ = nexus_abi::cap_close(cap);
                            ()
                        })?;
                    // Receive response on our deterministic reply inbox (bounded, non-blocking).
                    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
                    let mut buf = [0u8; 512];
                    let mut got = false;
                    for _ in 0..30_000 {
                        match nexus_abi::ipc_recv_v1(
                            reply_recv_slot,
                            &mut rh,
                            &mut buf,
                            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                            0,
                        ) {
                            Ok(n) => {
                                let n = core::cmp::min(n as usize, buf.len());
                                rsp_payload.extend_from_slice(&buf[..n]);
                                got = true;
                                break;
                            }
                            Err(nexus_abi::IpcError::QueueEmpty) => {
                                let _ = nexus_abi::yield_();
                            }
                            Err(_) => break,
                        }
                    }
                    if !got {
                        status = 1;
                    }
                    let _ =
                        nexus_abi::debug_println("dsoftbusd: remote proxy ok (peer=node-a service=samgrd)");
                }
            }
            SVC_BUNDLE_LIST => {
                let cap = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
                bundlemgrd
                    .send_with_cap_move_wait(
                        req,
                        cap,
                        Wait::Timeout(core::time::Duration::from_millis(300)),
                    )
                    .map_err(|_| {
                        let _ = nexus_abi::cap_close(cap);
                        ()
                    })?;
                let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
                let mut buf = [0u8; 512];
                let mut got = false;
                for _ in 0..30_000 {
                    match nexus_abi::ipc_recv_v1(
                        reply_recv_slot,
                        &mut rh,
                        &mut buf,
                        nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                        0,
                    ) {
                        Ok(n) => {
                            let n = core::cmp::min(n as usize, buf.len());
                            rsp_payload.extend_from_slice(&buf[..n]);
                            got = true;
                            break;
                        }
                        Err(nexus_abi::IpcError::QueueEmpty) => {
                            let _ = nexus_abi::yield_();
                        }
                        Err(_) => break,
                    }
                }
                if !got {
                    status = 1;
                }
                let _ =
                    nexus_abi::debug_println("dsoftbusd: remote proxy ok (peer=node-a service=bundlemgrd)");
            }
            _ => {
                status = 1;
                let _ = nexus_abi::debug_println("dsoftbusd: remote proxy denied (service=unknown)");
            }
        }

        // Build fixed-size response record.
        let mut rsp_plain = [0u8; RSP_PLAIN];
        rsp_plain[0] = status;
        let len = core::cmp::min(rsp_payload.len(), MAX_RSP);
        rsp_plain[1..3].copy_from_slice(&(len as u16).to_le_bytes());
        rsp_plain[3..3 + len].copy_from_slice(&rsp_payload[..len]);

        let mut rsp_ciph = [0u8; RSP_CIPH];
        let n = match transport.encrypt(&rsp_plain, &mut rsp_ciph) {
            Ok(v) => v,
            Err(_) => {
                continue;
            }
        };
        if n != RSP_CIPH {
            continue;
        }
        if stream_write_all(
            pending_replies,
            nonce_ctr,
            net,
            sid,
            &rsp_ciph,
            reply_recv_slot,
            reply_send_slot,
        )
        .is_err()
        {
            continue;
        }
    }
}
