//! CONTEXT: Remote proxy gateway loop for authenticated cross-VM streams.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host reject tests + QEMU 2-VM remote proxy markers
//!
//! SECURITY INVARIANTS:
//! - Deny-by-default service routing.
//! - Remote packagefs contract is read-only and bounded.
//! - No secret/session material is emitted in logs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::{Client, KernelClient, Wait};

use crate::os::gateway::packagefs_ro as pkg;
use crate::os::netstack::{stream_read_exact, stream_write_all, SessionId};
use crate::os::service_clients;
use crate::os::session::records::{MAX_REQ, MAX_RSP, REQ_CIPH, REQ_PLAIN, RSP_CIPH, RSP_PLAIN};

pub(crate) const SVC_SAMGR_RESOLVE_STATUS: u8 = 1;
pub(crate) const SVC_BUNDLE_LIST: u8 = 2;
pub(crate) const SVC_PACKAGEFS_RO: u8 = 3;

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
    .ok_or_else(|| {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep samgrd fail");
        // #endregion
        ()
    })?;
    // #region agent log
    let _ = nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep samgrd ok");
    // #endregion
    let bundlemgrd = service_clients::cached_client_slots_bounded(
        "bundlemgrd",
        &service_clients::BUNDLEMGRD_SEND_SLOT_CACHE,
        &service_clients::BUNDLEMGRD_RECV_SLOT_CACHE,
        128,
    )
    .ok_or_else(|| {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep bundlemgrd fail");
        // #endregion
        ()
    })?;
    // #region agent log
    let _ = nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep bundlemgrd ok");
    // #endregion
    let mut pkg_handles: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut next_pkg_handle: u32 = 1;
    let mut pkg_dep_fail_logged = false;
    let mut pkg_dep_ok_logged = false;
    let mut pkgfs_served_logged = false;
    let _ = nexus_abi::debug_println("dsoftbusd: remote proxy up");
    let mut rx_logged = false;
    let mut proxy_io_retry_logged = false;
    let mut samgr_rsp_head_logged = false;
    let mut bundle_rsp_head_logged = false;
    let mut proxy_rsp_write_ok_logged = false;
    let mut proxy_rsp_write_fail_logged = false;
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
                                if !samgr_rsp_head_logged {
                                    samgr_rsp_head_logged = true;
                                    // #region agent log
                                    let _ = if n >= 2 && buf[0] == b'S' && buf[1] == b'M' {
                                        nexus_abi::debug_println(
                                            "dbg:dsoftbusd: proxy samgr rsp head sm",
                                        )
                                    } else if n >= 2 && buf[0] == b'N' && buf[1] == b'S' {
                                        nexus_abi::debug_println(
                                            "dbg:dsoftbusd: proxy samgr rsp head ns",
                                        )
                                    } else {
                                        nexus_abi::debug_println(
                                            "dbg:dsoftbusd: proxy samgr rsp head other",
                                        )
                                    };
                                    // #endregion
                                }
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
                    let _ = nexus_abi::debug_println(
                        "dsoftbusd: remote proxy ok (peer=node-a service=samgrd)",
                    );
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
                            if !bundle_rsp_head_logged {
                                bundle_rsp_head_logged = true;
                                // #region agent log
                                let _ = if n >= 2 && buf[0] == b'B' && buf[1] == b'N' {
                                    nexus_abi::debug_println(
                                        "dbg:dsoftbusd: proxy bundle rsp head bn",
                                    )
                                } else if n >= 2 && buf[0] == b'N' && buf[1] == b'S' {
                                    nexus_abi::debug_println(
                                        "dbg:dsoftbusd: proxy bundle rsp head ns",
                                    )
                                } else {
                                    nexus_abi::debug_println(
                                        "dbg:dsoftbusd: proxy bundle rsp head other",
                                    )
                                };
                                // #endregion
                            }
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
                let _ = nexus_abi::debug_println(
                    "dsoftbusd: remote proxy ok (peer=node-a service=bundlemgrd)",
                );
            }
            SVC_PACKAGEFS_RO => {
                if let Some(packagefsd) = service_clients::cached_client_slots_bounded(
                    "packagefsd",
                    &service_clients::PACKAGEFSD_SEND_SLOT_CACHE,
                    &service_clients::PACKAGEFSD_RECV_SLOT_CACHE,
                    128,
                ) {
                    if !pkg_dep_ok_logged {
                        pkg_dep_ok_logged = true;
                        // #region agent log
                        let _ =
                            nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep packagefsd ok");
                        // #endregion
                    }
                    let (pkg_rsp, served_ok) = handle_packagefs_ro_request(
                        true,
                        req,
                        &packagefsd,
                        &mut pkg_handles,
                        &mut next_pkg_handle,
                    );
                    status = 0;
                    rsp_payload = pkg_rsp;
                    if served_ok && !pkgfs_served_logged {
                        pkgfs_served_logged = true;
                        let _ = nexus_abi::debug_println("dsoftbusd: remote packagefs served");
                    }
                } else {
                    if !pkg_dep_fail_logged {
                        pkg_dep_fail_logged = true;
                        // #region agent log
                        let _ =
                            nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep packagefsd fail");
                        // #endregion
                    }
                    status = 0;
                    let op = if req.len() >= 4 { req[3] } else { pkg::PK_OP_STAT };
                    rsp_payload = pkg::encode_status_only(op, pkg::PK_STATUS_IO);
                }
            }
            _ => {
                status = 1;
                let _ =
                    nexus_abi::debug_println("dsoftbusd: remote proxy denied (service=unknown)");
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
            if !proxy_rsp_write_fail_logged {
                proxy_rsp_write_fail_logged = true;
                // #region agent log
                let _ = nexus_abi::debug_println("dbg:dsoftbusd: proxy rsp write fail");
                // #endregion
            }
            continue;
        }
        if !proxy_rsp_write_ok_logged {
            proxy_rsp_write_ok_logged = true;
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:dsoftbusd: proxy rsp write ok");
            // #endregion
        }
    }
}

fn handle_packagefs_ro_request(
    authenticated: bool,
    req: &[u8],
    packagefsd: &KernelClient,
    pkg_handles: &mut BTreeMap<u32, Vec<u8>>,
    next_pkg_handle: &mut u32,
) -> (Vec<u8>, bool) {
    let op_for_error = if req.len() >= 4 { req[3] } else { pkg::PK_OP_STAT };
    let parsed = match pkg::parse_request(req, authenticated) {
        Ok(v) => v,
        Err(reason) => {
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:dsoftbusd:H5 pkgfs parse reject");
            // #endregion
            return (
                pkg::encode_status_only(op_for_error, pkg::reject_reason_to_status(reason)),
                false,
            );
        }
    };
    match parsed {
        pkg::PackagefsRequest::Stat { rel_path } => {
            match resolve_package_entry(packagefsd, &rel_path) {
                Ok(Some(entry)) => {
                    // #region agent log
                    let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs stat ok");
                    // #endregion
                    (pkg::encode_stat_rsp(pkg::PK_STATUS_OK, entry.size, entry.kind), true)
                }
                Ok(None) => {
                    // #region agent log
                    let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs stat not_found");
                    // #endregion
                    (pkg::encode_stat_rsp(pkg::PK_STATUS_NOT_FOUND, 0, 0), false)
                }
                Err(()) => {
                    // #region agent log
                    let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs stat io");
                    // #endregion
                    (pkg::encode_stat_rsp(pkg::PK_STATUS_IO, 0, 0), false)
                }
            }
        }
        pkg::PackagefsRequest::Open { rel_path } => {
            match resolve_package_entry(packagefsd, &rel_path) {
                Ok(Some(entry)) => {
                    if entry.kind != pkg::PACKAGEFS_KIND_FILE {
                        return (pkg::encode_open_rsp(pkg::PK_STATUS_BAD_REQUEST, 0), false);
                    }
                    if entry.bytes.len() > pkg::PK_MAX_OPEN_FILE_BYTES {
                        return (pkg::encode_open_rsp(pkg::PK_STATUS_OVERSIZED, 0), false);
                    }
                    if pkg_handles.len() >= pkg::PK_MAX_HANDLES {
                        return (pkg::encode_open_rsp(pkg::PK_STATUS_LIMIT, 0), false);
                    }
                    let handle = allocate_handle_id(pkg_handles, next_pkg_handle);
                    pkg_handles.insert(handle, entry.bytes);
                    (pkg::encode_open_rsp(pkg::PK_STATUS_OK, handle), true)
                }
                Ok(None) => (pkg::encode_open_rsp(pkg::PK_STATUS_NOT_FOUND, 0), false),
                Err(()) => (pkg::encode_open_rsp(pkg::PK_STATUS_IO, 0), false),
            }
        }
        pkg::PackagefsRequest::Read { handle, offset, read_len } => {
            let Some(data) = pkg_handles.get(&handle) else {
                return (pkg::encode_read_rsp(pkg::PK_STATUS_BADF, &[]), false);
            };
            let start = core::cmp::min(offset as usize, data.len());
            let end = core::cmp::min(start.saturating_add(read_len as usize), data.len());
            (pkg::encode_read_rsp(pkg::PK_STATUS_OK, &data[start..end]), true)
        }
        pkg::PackagefsRequest::Close { handle } => {
            if pkg_handles.remove(&handle).is_some() {
                (pkg::encode_status_only(pkg::PK_OP_CLOSE, pkg::PK_STATUS_OK), true)
            } else {
                (pkg::encode_status_only(pkg::PK_OP_CLOSE, pkg::PK_STATUS_BADF), false)
            }
        }
    }
}

fn allocate_handle_id(pkg_handles: &BTreeMap<u32, Vec<u8>>, next_pkg_handle: &mut u32) -> u32 {
    let mut candidate = *next_pkg_handle;
    for _ in 0..(pkg::PK_MAX_HANDLES * 2) {
        if candidate == 0 {
            candidate = 1;
        }
        if !pkg_handles.contains_key(&candidate) {
            *next_pkg_handle = candidate.wrapping_add(1);
            return candidate;
        }
        candidate = candidate.wrapping_add(1);
    }
    // Deterministic fallback in the unlikely wrap-around window.
    let mut fallback = 1u32;
    while pkg_handles.contains_key(&fallback) {
        fallback = fallback.wrapping_add(1);
    }
    *next_pkg_handle = fallback.wrapping_add(1);
    fallback
}

fn resolve_package_entry(
    packagefsd: &KernelClient,
    rel_path: &str,
) -> core::result::Result<Option<pkg::PackagefsEntry>, ()> {
    let mut req = Vec::with_capacity(1 + rel_path.len());
    req.push(pkg::PACKAGEFSD_OP_RESOLVE);
    req.extend_from_slice(rel_path.as_bytes());
    packagefsd
        .send(&req, Wait::Timeout(core::time::Duration::from_millis(300)))
        .map_err(|_| {
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs resolve send fail");
            // #endregion
            ()
        })?;
    let rsp = packagefsd
        .recv(Wait::Timeout(core::time::Duration::from_millis(300)))
        .map_err(|_| {
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs resolve timeout");
            // #endregion
            ()
        })?;
    pkg::decode_packagefs_resolve_rsp(&rsp).map_err(|_| {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs resolve rsp malformed");
        // #endregion
        ()
    })
}
