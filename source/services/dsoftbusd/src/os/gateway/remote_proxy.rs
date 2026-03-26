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
use core::sync::atomic::{AtomicU64, Ordering};
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::{Client, KernelClient, Wait};
use statefs::protocol as sfp;

use crate::os::gateway::packagefs_ro as pkg;
use crate::os::gateway::statefs_rw as stfs;
use crate::os::netstack::{stream_read_exact, stream_write_all, SessionId};
use crate::os::observability;
use crate::os::service_clients;
use crate::os::session::records::{MAX_REQ, MAX_RSP, REQ_CIPH, REQ_PLAIN, RSP_CIPH, RSP_PLAIN};

pub(crate) const SVC_SAMGR_RESOLVE_STATUS: u8 = 1;
pub(crate) const SVC_BUNDLE_LIST: u8 = 2;
pub(crate) const SVC_PACKAGEFS_RO: u8 = 3;
pub(crate) const SVC_STATEFS_RW: u8 = 4;
static STATEFS_PROXY_NONCE: AtomicU64 = AtomicU64::new(1);

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
    let mut statefs_dep_fail_logged = false;
    let mut statefs_dep_ok_logged = false;
    let mut pkgfs_served_logged = false;
    let mut statefs_served_logged = false;
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
                        let _ = nexus_abi::debug_println(
                            "dbg:dsoftbusd: remote proxy dep packagefsd ok",
                        );
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
                        let _ = nexus_abi::debug_println(
                            "dbg:dsoftbusd: remote proxy dep packagefsd fail",
                        );
                        // #endregion
                    }
                    status = 0;
                    let op = if req.len() >= 4 { req[3] } else { pkg::PK_OP_STAT };
                    rsp_payload = pkg::encode_status_only(op, pkg::PK_STATUS_IO);
                }
            }
            SVC_STATEFS_RW => {
                if let Some(statefsd) = service_clients::cached_client_slots_bounded(
                    "statefsd",
                    &service_clients::STATEFSD_SEND_SLOT_CACHE,
                    &service_clients::STATEFSD_RECV_SLOT_CACHE,
                    128,
                ) {
                    if !statefs_dep_ok_logged {
                        statefs_dep_ok_logged = true;
                        let _ =
                            nexus_abi::debug_println("dbg:dsoftbusd: remote proxy dep statefsd ok");
                    }
                    let (statefs_rsp, served_ok, audit_label) =
                        handle_statefs_rw_request(
                            true,
                            req,
                            &statefsd,
                            reply_send_slot,
                            reply_recv_slot,
                        );
                    status = 0;
                    rsp_payload = statefs_rsp;
                    if let Some(label) = audit_label {
                        emit_remote_statefs_audit(label);
                    }
                    if served_ok && !statefs_served_logged {
                        statefs_served_logged = true;
                        let _ = nexus_abi::debug_println("dsoftbusd: remote statefs served");
                    }
                } else {
                    if !statefs_dep_fail_logged {
                        statefs_dep_fail_logged = true;
                        let _ = nexus_abi::debug_println(
                            "dbg:dsoftbusd: remote proxy dep statefsd fail",
                        );
                    }
                    status = 0;
                    let op = stfs::op_from_frame(req).unwrap_or(sfp::OP_SYNC);
                    let nonce = stfs::request_nonce_from_frame(req);
                    rsp_payload = encode_statefs_io_response(op, nonce);
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
    let req = pkg::encode_packagefs_resolve_req(rel_path);
    packagefsd.send(&req, Wait::Timeout(core::time::Duration::from_millis(300))).map_err(|_| {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: pkgfs resolve send fail");
        // #endregion
        ()
    })?;
    let rsp =
        packagefsd.recv(Wait::Timeout(core::time::Duration::from_millis(300))).map_err(|_| {
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

fn handle_statefs_rw_request(
    authenticated: bool,
    req: &[u8],
    statefsd: &KernelClient,
    _reply_send_slot: u32,
    _reply_recv_slot: u32,
) -> (Vec<u8>, bool, Option<&'static str>) {
    let op_for_error = stfs::op_from_frame(req).unwrap_or(sfp::OP_SYNC);
    let parsed = match stfs::parse_request(req, authenticated) {
        Ok(v) => v,
        Err(reason) => {
            return (
                stfs::encode_reject_response(op_for_error, reason),
                false,
                stfs::reject_label_for_request(op_for_error, reason),
            );
        }
    };

    let op = parsed.op();
    let request_nonce = parsed.nonce();
    let served_eligible = stfs::is_mutating_request(&parsed.request);
    let internal_nonce = STATEFS_PROXY_NONCE.fetch_add(1, Ordering::Relaxed);
    let internal_req = match encode_statefs_request_with_nonce(&parsed.request, internal_nonce) {
        Ok(frame) => frame,
        Err(()) => {
            let io_status = sfp::STATUS_IO_ERROR;
            return (
                encode_statefs_io_response(op, request_nonce),
                false,
                stfs::audit_label_for_status(op, io_status),
            );
        }
    };

    if statefsd
        .send(
            internal_req.as_slice(),
            Wait::Timeout(core::time::Duration::from_millis(2_000)),
        )
        .is_err()
    {
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: remote statefs send fail");
        let io_status = sfp::STATUS_IO_ERROR;
        return (
            encode_statefs_io_response(op, request_nonce),
            false,
            stfs::audit_label_for_status(op, io_status),
        );
    }

    let mut matched_rsp: Option<Vec<u8>> = None;
    for _ in 0..256u16 {
        match statefsd.recv(Wait::Timeout(core::time::Duration::from_millis(8))) {
            Ok(frame) => {
                if is_matching_statefs_v2_response(op, internal_nonce, frame.as_slice()) {
                    matched_rsp = Some(frame);
                    break;
                }
            }
            Err(_) => {
                let _ = nexus_abi::yield_();
            }
        }
    }
    let Some(rsp) = matched_rsp else {
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: remote statefs recv fail");
        let io_status = sfp::STATUS_IO_ERROR;
        return (
            encode_statefs_io_response(op, request_nonce),
            false,
            stfs::audit_label_for_status(op, io_status),
        );
    };
    let rsp_status = rsp.get(4).copied().unwrap_or(sfp::STATUS_IO_ERROR);
    let proxy_rsp = match op {
        sfp::OP_GET => {
            if rsp_status == sfp::STATUS_OK {
                match sfp::decode_get_response(&rsp) {
                    Ok(value) => sfp::encode_get_response_with_nonce(
                        sfp::STATUS_OK,
                        value.as_slice(),
                        request_nonce,
                    ),
                    Err(_) => encode_statefs_io_response(op, request_nonce),
                }
            } else {
                sfp::encode_get_response_with_nonce(rsp_status, &[], request_nonce)
            }
        }
        sfp::OP_LIST => {
            if rsp_status == sfp::STATUS_OK {
                match sfp::decode_list_response(&rsp) {
                    Ok(keys) => sfp::encode_list_response_with_nonce(
                        sfp::STATUS_OK,
                        keys.as_slice(),
                        stfs::RS_MAX_RESPONSE_LEN,
                        request_nonce,
                    ),
                    Err(_) => encode_statefs_io_response(op, request_nonce),
                }
            } else {
                sfp::encode_list_response_with_nonce(
                    rsp_status,
                    &[],
                    stfs::RS_MAX_RESPONSE_LEN,
                    request_nonce,
                )
            }
        }
        _ => stfs::encode_status_response(op, rsp_status, request_nonce),
    };

    if stfs::is_mutating_request(&parsed.request) && rsp_status != sfp::STATUS_OK {
        emit_remote_statefs_status_debug(op, rsp_status);
    }
    (
        proxy_rsp,
        served_eligible && rsp_status == sfp::STATUS_OK,
        stfs::audit_label_for_status(op, rsp_status),
    )
}

fn encode_statefs_io_response(op: u8, nonce: Option<u64>) -> Vec<u8> {
    match op {
        sfp::OP_GET => sfp::encode_get_response_with_nonce(sfp::STATUS_IO_ERROR, &[], nonce),
        sfp::OP_LIST => {
            sfp::encode_list_response_with_nonce(sfp::STATUS_IO_ERROR, &[], stfs::RS_MAX_RESPONSE_LEN, nonce)
        }
        _ => stfs::encode_status_response(op, sfp::STATUS_IO_ERROR, nonce),
    }
}

fn encode_statefs_request_with_nonce(req: &sfp::Request<'_>, nonce: u64) -> core::result::Result<Vec<u8>, ()> {
    let mut frame = match req {
        sfp::Request::Put { key, value } => sfp::encode_put_request(key, value).map_err(|_| ())?,
        sfp::Request::Get { key } => sfp::encode_key_only_request(sfp::OP_GET, key).map_err(|_| ())?,
        sfp::Request::Delete { key } => {
            sfp::encode_key_only_request(sfp::OP_DEL, key).map_err(|_| ())?
        }
        sfp::Request::List { prefix, limit } => {
            sfp::encode_list_request(prefix, *limit).map_err(|_| ())?
        }
        sfp::Request::Sync => sfp::encode_sync_request(),
        sfp::Request::Reopen => sfp::encode_reopen_request(),
    };
    if frame.len() < 4 || frame[0] != sfp::MAGIC0 || frame[1] != sfp::MAGIC1 {
        return Err(());
    }
    frame[2] = sfp::VERSION_V2;
    frame.splice(4..4, nonce.to_le_bytes());
    Ok(frame)
}

fn is_matching_statefs_v2_response(op: u8, nonce: u64, frame: &[u8]) -> bool {
    if frame.len() < 13 {
        return false;
    }
    if frame[0] != sfp::MAGIC0 || frame[1] != sfp::MAGIC1 || frame[2] != sfp::VERSION_V2 {
        return false;
    }
    if frame[3] != (op | 0x80) {
        return false;
    }
    let mut got = [0u8; 8];
    got.copy_from_slice(&frame[5..13]);
    u64::from_le_bytes(got) == nonce
}

fn emit_remote_statefs_audit(label: &'static str) {
    if !observability::append_probe_to_logd(b"dsoftbusd", label.as_bytes()) {
        let _ = nexus_abi::debug_println(label);
    }
}

fn emit_remote_statefs_status_debug(op: u8, status: u8) {
    let label = match (op, status) {
        (sfp::OP_PUT, sfp::STATUS_ACCESS_DENIED) => {
            "dbg:dsoftbusd: remote statefs put status access_denied"
        }
        (sfp::OP_PUT, sfp::STATUS_IO_ERROR) => "dbg:dsoftbusd: remote statefs put status io",
        (sfp::OP_PUT, sfp::STATUS_VALUE_TOO_LARGE) => {
            "dbg:dsoftbusd: remote statefs put status value_too_large"
        }
        (sfp::OP_PUT, sfp::STATUS_INVALID_KEY) => {
            "dbg:dsoftbusd: remote statefs put status invalid_key"
        }
        (sfp::OP_DEL, sfp::STATUS_ACCESS_DENIED) => {
            "dbg:dsoftbusd: remote statefs delete status access_denied"
        }
        (sfp::OP_DEL, sfp::STATUS_IO_ERROR) => "dbg:dsoftbusd: remote statefs delete status io",
        _ => "dbg:dsoftbusd: remote statefs mutating status other",
    };
    let _ = nexus_abi::debug_println(label);
}
