//! CONTEXT: Local IPC gateway API (`selftest-client` <-> `dsoftbusd`).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU selftest markers + dsoftbusd host seam tests
//!
//! SECURITY INVARIANTS:
//! - Requests are bounded before remote forwarding.
//! - Remote packagefs access is routed via authenticated stream-only path.
//! - Invalid frames fail closed with deterministic status.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use alloc::vec::Vec;
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};

use crate::os::gateway::packagefs_ro as pkg;
use crate::os::gateway::remote_proxy::{
    SVC_BUNDLE_LIST, SVC_PACKAGEFS_RO, SVC_SAMGR_RESOLVE_STATUS,
};
use crate::os::netstack::{stream_read_exact, stream_write_all, SessionId};
use crate::os::observability;
use crate::os::session::records::{MAX_REQ, MAX_RSP, REQ_CIPH, REQ_PLAIN, RSP_CIPH, RSP_PLAIN};

pub(crate) const L0: u8 = b'D';
pub(crate) const L1: u8 = b'S';
pub(crate) const LVER: u8 = 1;
pub(crate) const LOP_REMOTE_RESOLVE: u8 = 1;
pub(crate) const LOP_REMOTE_BUNDLE_LIST: u8 = 2;
pub(crate) const LOP_REMOTE_PKGFS_STAT: u8 = 3;
pub(crate) const LOP_REMOTE_PKGFS_OPEN: u8 = 4;
pub(crate) const LOP_REMOTE_PKGFS_READ: u8 = 5;
pub(crate) const LOP_REMOTE_PKGFS_CLOSE: u8 = 6;
pub(crate) const LOP_LOG_PROBE: u8 = 0x7f;
pub(crate) const LSTATUS_OK: u8 = 0;
pub(crate) const LSTATUS_FAIL: u8 = 1;

fn remote_exchange(
    transport: &mut nexus_noise_xk::Transport,
    pending_replies: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    sid: SessionId,
    reply_recv_slot: u32,
    reply_send_slot: u32,
    svc: u8,
    req: &[u8],
) -> core::result::Result<[u8; RSP_PLAIN], ()> {
    let mut plain = [0u8; REQ_PLAIN];
    plain[0] = svc;
    let used = core::cmp::min(req.len(), MAX_REQ);
    plain[1..3].copy_from_slice(&(used as u16).to_le_bytes());
    plain[3..3 + used].copy_from_slice(&req[..used]);
    let mut ciph = [0u8; REQ_CIPH];
    let n = transport.encrypt(&plain, &mut ciph).map_err(|_| ())?;
    if n != REQ_CIPH {
        return Err(());
    }
    stream_write_all(
        pending_replies,
        nonce_ctr,
        net,
        sid,
        &ciph,
        reply_recv_slot,
        reply_send_slot,
    )?;

    let mut rsp_ciph = [0u8; RSP_CIPH];
    stream_read_exact(
        pending_replies,
        nonce_ctr,
        net,
        sid,
        &mut rsp_ciph,
        reply_recv_slot,
        reply_send_slot,
    )?;
    let mut rsp_plain = [0u8; RSP_PLAIN];
    let n = transport.decrypt(&rsp_ciph, &mut rsp_plain).map_err(|_| ())?;
    if n != RSP_PLAIN {
        return Err(());
    }
    Ok(rsp_plain)
}

pub(crate) fn run_local_ipc_loop(
    transport: &mut nexus_noise_xk::Transport,
    pending_replies: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    sid: SessionId,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<(), ()> {
    let server = loop {
        match KernelServer::new_for("dsoftbusd") {
            Ok(s) => break s,
            Err(_) => {
                let _ = nexus_abi::yield_();
            }
        }
    };
    let mut ipc_logged = false;
    let mut remote_rpc_fail_logged = false;
    loop {
        // Use the plain request/response channel semantics (`Client::send`/`Client::recv`),
        // not the cap-move reply-token style.
        let frame = match server.recv(Wait::Blocking) {
            Ok(x) => x,
            Err(_) => {
                let _ = nexus_abi::yield_();
                continue;
            }
        };
        if !ipc_logged {
            ipc_logged = true;
        }

        let mut out: Vec<u8> = Vec::new();
        if frame.len() < 4 || frame[0] != L0 || frame[1] != L1 || frame[2] != LVER {
            out.extend_from_slice(&[L0, L1, LVER, 0x80, LSTATUS_FAIL]);
        } else {
            match frame[3] {
                LOP_LOG_PROBE => {
                    let ok = observability::append_probe_to_logd(
                        b"dsoftbusd",
                        b"core service log probe: dsoftbusd",
                    );
                    out.extend_from_slice(&[
                        L0,
                        L1,
                        LVER,
                        LOP_LOG_PROBE | 0x80,
                        if ok { LSTATUS_OK } else { LSTATUS_FAIL },
                    ]);
                }
                LOP_REMOTE_RESOLVE => {
                    if frame.len() < 5 {
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            LOP_REMOTE_RESOLVE | 0x80,
                            LSTATUS_FAIL,
                        ]);
                    } else {
                        let n = frame[4] as usize;
                        if n == 0 || frame.len() != 5 + n {
                            out.extend_from_slice(&[
                                L0,
                                L1,
                                LVER,
                                LOP_REMOTE_RESOLVE | 0x80,
                                LSTATUS_FAIL,
                            ]);
                        } else {
                            // Build samgrd resolve-status request frame.
                            let mut req = Vec::with_capacity(5 + n);
                            req.push(b'S');
                            req.push(b'M');
                            req.push(1);
                            req.push(6); // OP_RESOLVE_STATUS
                            req.push(n as u8);
                            req.extend_from_slice(&frame[5..]);

                            // Send remote gateway request.
                            let mut ok = false;
                            let remote_result: core::result::Result<(), ()> = (|| {
                                let mut plain = [0u8; REQ_PLAIN];
                                plain[0] = SVC_SAMGR_RESOLVE_STATUS;
                                let used = core::cmp::min(req.len(), MAX_REQ);
                                plain[1..3].copy_from_slice(&(used as u16).to_le_bytes());
                                plain[3..3 + used].copy_from_slice(&req[..used]);
                                let mut ciph = [0u8; REQ_CIPH];
                                let n = transport.encrypt(&plain, &mut ciph).map_err(|_| ())?;
                                if n != REQ_CIPH {
                                    return Err(());
                                }
                                stream_write_all(
                                    pending_replies,
                                    nonce_ctr,
                                    net,
                                    sid,
                                    &ciph,
                                    reply_recv_slot,
                                    reply_send_slot,
                                )?;

                                let mut rsp_ciph = [0u8; RSP_CIPH];
                                stream_read_exact(
                                    pending_replies,
                                    nonce_ctr,
                                    net,
                                    sid,
                                    &mut rsp_ciph,
                                    reply_recv_slot,
                                    reply_send_slot,
                                )?;
                                let mut rsp_plain = [0u8; RSP_PLAIN];
                                let n =
                                    transport.decrypt(&rsp_ciph, &mut rsp_plain).map_err(|_| ())?;
                                if n != RSP_PLAIN {
                                    return Err(());
                                }
                                let st = rsp_plain[0];
                                let len = u16::from_le_bytes([rsp_plain[1], rsp_plain[2]]) as usize;
                                if st == 0 && len >= 13 {
                                    let p = &rsp_plain[3..3 + len];
                                    ok = p[0] == b'S'
                                        && p[1] == b'M'
                                        && p[2] == 1
                                        && p[3] == (6 | 0x80)
                                        && p[4] == 0;
                                }
                                Ok(())
                            })(
                            );
                            if remote_result.is_err() && !remote_rpc_fail_logged {
                                remote_rpc_fail_logged = true;
                                // #region agent log
                                let _ =
                                    nexus_abi::debug_println("dbg:dsoftbusd: remote rpc fail resolve");
                                // #endregion
                            }

                            out.extend_from_slice(&[
                                L0,
                                L1,
                                LVER,
                                LOP_REMOTE_RESOLVE | 0x80,
                                if ok { LSTATUS_OK } else { LSTATUS_FAIL },
                            ]);
                        }
                    }
                }
                LOP_REMOTE_BUNDLE_LIST => {
                    // bundlemgrd list request: [B,N,1,OP_LIST]
                    let mut ok = false;
                    let mut count: u16 = 0;
                    let remote_result: core::result::Result<(), ()> = (|| {
                        let req = [b'B', b'N', 1, nexus_abi::bundlemgrd::OP_LIST];
                        let mut plain = [0u8; REQ_PLAIN];
                        plain[0] = SVC_BUNDLE_LIST;
                        plain[1..3].copy_from_slice(&(req.len() as u16).to_le_bytes());
                        plain[3..3 + req.len()].copy_from_slice(&req);
                        let mut ciph = [0u8; REQ_CIPH];
                        let n = transport.encrypt(&plain, &mut ciph).map_err(|_| ())?;
                        if n != REQ_CIPH {
                            return Err(());
                        }
                        stream_write_all(
                            pending_replies,
                            nonce_ctr,
                            net,
                            sid,
                            &ciph,
                            reply_recv_slot,
                            reply_send_slot,
                        )?;

                        let mut rsp_ciph = [0u8; RSP_CIPH];
                        stream_read_exact(
                            pending_replies,
                            nonce_ctr,
                            net,
                            sid,
                            &mut rsp_ciph,
                            reply_recv_slot,
                            reply_send_slot,
                        )?;
                        let mut rsp_plain = [0u8; RSP_PLAIN];
                        let n = transport.decrypt(&rsp_ciph, &mut rsp_plain).map_err(|_| ())?;
                        if n != RSP_PLAIN {
                            return Err(());
                        }
                        let st = rsp_plain[0];
                        let len = u16::from_le_bytes([rsp_plain[1], rsp_plain[2]]) as usize;
                        if st == 0 && len >= 8 {
                            let p = &rsp_plain[3..3 + len];
                            if p[0] == b'B'
                                && p[1] == b'N'
                                && p[2] == 1
                                && p[3] == (nexus_abi::bundlemgrd::OP_LIST | 0x80)
                                && p[4] == 0
                            {
                                count = u16::from_le_bytes([p[5], p[6]]);
                                ok = true;
                            }
                        }
                        Ok(())
                    })();
                    if remote_result.is_err() && !remote_rpc_fail_logged {
                        remote_rpc_fail_logged = true;
                        // #region agent log
                        let _ =
                            nexus_abi::debug_println("dbg:dsoftbusd: remote rpc fail bundle-list");
                        // #endregion
                    }
                    out.extend_from_slice(&[
                        L0,
                        L1,
                        LVER,
                        LOP_REMOTE_BUNDLE_LIST | 0x80,
                        if ok { LSTATUS_OK } else { LSTATUS_FAIL },
                    ]);
                    out.extend_from_slice(&count.to_le_bytes());
                }
                LOP_REMOTE_PKGFS_STAT | LOP_REMOTE_PKGFS_OPEN => {
                    if frame.len() < 5 {
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            frame[3] | 0x80,
                            LSTATUS_FAIL,
                        ]);
                    } else {
                        let path_len = frame[4] as usize;
                        if path_len == 0
                            || path_len > pkg::PK_MAX_PATH_LEN
                            || frame.len() != 5 + path_len
                        {
                            out.extend_from_slice(&[
                                L0,
                                L1,
                                LVER,
                                frame[3] | 0x80,
                                LSTATUS_FAIL,
                            ]);
                        } else {
                            let pk_op = if frame[3] == LOP_REMOTE_PKGFS_STAT {
                                pkg::PK_OP_STAT
                            } else {
                                pkg::PK_OP_OPEN
                            };
                            let path = &frame[5..];
                            let mut req = Vec::with_capacity(6 + path.len());
                            req.extend_from_slice(&[pkg::PK_MAGIC0, pkg::PK_MAGIC1, pkg::PK_VERSION, pk_op]);
                            req.extend_from_slice(&(path.len() as u16).to_le_bytes());
                            req.extend_from_slice(path);

                            let mut status = LSTATUS_FAIL;
                            let mut size = 0u64;
                            let mut kind = 0u16;
                            let mut handle = 0u32;
                            let remote_result: core::result::Result<(), ()> = (|| {
                                let rsp = remote_exchange(
                                    transport,
                                    pending_replies,
                                    nonce_ctr,
                                    net,
                                    sid,
                                    reply_recv_slot,
                                    reply_send_slot,
                                    SVC_PACKAGEFS_RO,
                                    &req,
                                )?;
                                if rsp[0] != 0 {
                                    return Err(());
                                }
                                let n = u16::from_le_bytes([rsp[1], rsp[2]]) as usize;
                                if n > MAX_RSP {
                                    return Err(());
                                }
                                let p = &rsp[3..3 + n];
                                if p.len() < 5
                                    || p[0] != pkg::PK_MAGIC0
                                    || p[1] != pkg::PK_MAGIC1
                                    || p[2] != pkg::PK_VERSION
                                    || p[3] != (pk_op | 0x80)
                                {
                                    return Err(());
                                }
                                status = p[4];
                                if pk_op == pkg::PK_OP_STAT && status == pkg::PK_STATUS_OK && p.len() >= 15 {
                                    size = u64::from_le_bytes([
                                        p[5], p[6], p[7], p[8], p[9], p[10], p[11], p[12],
                                    ]);
                                    kind = u16::from_le_bytes([p[13], p[14]]);
                                }
                                if pk_op == pkg::PK_OP_OPEN && p.len() >= 9 {
                                    handle = u32::from_le_bytes([p[5], p[6], p[7], p[8]]);
                                }
                                Ok(())
                            })();
                            if remote_result.is_err() && !remote_rpc_fail_logged {
                                remote_rpc_fail_logged = true;
                                // #region agent log
                                let _ =
                                    nexus_abi::debug_println("dbg:dsoftbusd: remote rpc fail pkgfs-stat-open");
                                // #endregion
                            }
                            out.extend_from_slice(&[L0, L1, LVER, frame[3] | 0x80, status]);
                            if frame[3] == LOP_REMOTE_PKGFS_STAT {
                                out.extend_from_slice(&size.to_le_bytes());
                                out.extend_from_slice(&kind.to_le_bytes());
                            } else {
                                out.extend_from_slice(&handle.to_le_bytes());
                            }
                        }
                    }
                }
                LOP_REMOTE_PKGFS_READ => {
                    if frame.len() != 14 {
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            LOP_REMOTE_PKGFS_READ | 0x80,
                            LSTATUS_FAIL,
                        ]);
                    } else {
                        let mut req = [0u8; 14];
                        req[0] = pkg::PK_MAGIC0;
                        req[1] = pkg::PK_MAGIC1;
                        req[2] = pkg::PK_VERSION;
                        req[3] = pkg::PK_OP_READ;
                        req[4..14].copy_from_slice(&frame[4..14]);
                        let mut status = LSTATUS_FAIL;
                        let mut data_len: u16 = 0;
                        let mut read_data: Vec<u8> = Vec::new();
                        let remote_result: core::result::Result<(), ()> = (|| {
                            let rsp = remote_exchange(
                                transport,
                                pending_replies,
                                nonce_ctr,
                                net,
                                sid,
                                reply_recv_slot,
                                reply_send_slot,
                                SVC_PACKAGEFS_RO,
                                &req,
                            )?;
                            if rsp[0] != 0 {
                                return Err(());
                            }
                            let n = u16::from_le_bytes([rsp[1], rsp[2]]) as usize;
                            if n > MAX_RSP {
                                return Err(());
                            }
                            let p = &rsp[3..3 + n];
                            if p.len() < 7
                                || p[0] != pkg::PK_MAGIC0
                                || p[1] != pkg::PK_MAGIC1
                                || p[2] != pkg::PK_VERSION
                                || p[3] != (pkg::PK_OP_READ | 0x80)
                            {
                                return Err(());
                            }
                            status = p[4];
                            let n = u16::from_le_bytes([p[5], p[6]]) as usize;
                            if n > pkg::PK_MAX_READ_LEN || p.len() < 7 + n {
                                return Err(());
                            }
                            data_len = n as u16;
                            read_data.extend_from_slice(&p[7..7 + n]);
                            Ok(())
                        })();
                        if remote_result.is_err() && !remote_rpc_fail_logged {
                            remote_rpc_fail_logged = true;
                            // #region agent log
                            let _ =
                                nexus_abi::debug_println("dbg:dsoftbusd: remote rpc fail pkgfs-read");
                            // #endregion
                        }
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            LOP_REMOTE_PKGFS_READ | 0x80,
                            status,
                        ]);
                        out.extend_from_slice(&data_len.to_le_bytes());
                        out.extend_from_slice(&read_data);
                    }
                }
                LOP_REMOTE_PKGFS_CLOSE => {
                    if frame.len() != 8 {
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            LOP_REMOTE_PKGFS_CLOSE | 0x80,
                            LSTATUS_FAIL,
                        ]);
                    } else {
                        let mut req = [0u8; 8];
                        req[0] = pkg::PK_MAGIC0;
                        req[1] = pkg::PK_MAGIC1;
                        req[2] = pkg::PK_VERSION;
                        req[3] = pkg::PK_OP_CLOSE;
                        req[4..8].copy_from_slice(&frame[4..8]);
                        let mut status = LSTATUS_FAIL;
                        let remote_result: core::result::Result<(), ()> = (|| {
                            let rsp = remote_exchange(
                                transport,
                                pending_replies,
                                nonce_ctr,
                                net,
                                sid,
                                reply_recv_slot,
                                reply_send_slot,
                                SVC_PACKAGEFS_RO,
                                &req,
                            )?;
                            if rsp[0] != 0 {
                                return Err(());
                            }
                            let n = u16::from_le_bytes([rsp[1], rsp[2]]) as usize;
                            if n > MAX_RSP {
                                return Err(());
                            }
                            let p = &rsp[3..3 + n];
                            if p.len() < 5
                                || p[0] != pkg::PK_MAGIC0
                                || p[1] != pkg::PK_MAGIC1
                                || p[2] != pkg::PK_VERSION
                                || p[3] != (pkg::PK_OP_CLOSE | 0x80)
                            {
                                return Err(());
                            }
                            status = p[4];
                            Ok(())
                        })();
                        if remote_result.is_err() && !remote_rpc_fail_logged {
                            remote_rpc_fail_logged = true;
                            // #region agent log
                            let _ =
                                nexus_abi::debug_println("dbg:dsoftbusd: remote rpc fail pkgfs-close");
                            // #endregion
                        }
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            LOP_REMOTE_PKGFS_CLOSE | 0x80,
                            status,
                        ]);
                    }
                }
                _ => {
                    out.extend_from_slice(&[L0, L1, LVER, (frame[3] | 0x80), LSTATUS_FAIL]);
                }
            }
        }

        let _ = server.send(&out, Wait::Blocking);
    }
}
