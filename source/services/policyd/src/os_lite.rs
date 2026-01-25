#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;
use core::sync::atomic::{AtomicUsize, Ordering};

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::KernelServer;
use nexus_sel::Policy;

/// Result alias used by the lite policyd backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Ready notifier invoked once the service becomes available.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Errors surfaced by the lite policyd backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Placeholder for the unimplemented runtime.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "policyd unsupported"),
        }
    }
}

/// Schema warmer placeholder for interface parity.
pub fn touch_schemas() {}

mod policy_table {
    include!(concat!(env!("OUT_DIR"), "/policy_table.rs"));
}

const CAP_CHECK: &str = "ipc.core";
const CAP_ROUTE: &str = "ipc.core";
const CAP_EXEC: &str = "proc.spawn";

const POLICY: Policy = Policy::new(policy_table::POLICY_ENTRIES);

const MAGIC0: u8 = b'P';
const MAGIC1: u8 = b'O';
const VERSION: u8 = 1;

const OP_CHECK: u8 = 1;
const OP_ROUTE: u8 = 2;
const OP_EXEC: u8 = 3;
const OP_CHECK_CAP: u8 = 4;

const STATUS_ALLOW: u8 = 0;
const STATUS_DENY: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_UNSUPPORTED: u8 = 3;
const OP_LOG_PROBE: u8 = 0x7f;

const AUDIT_SCOPE: &str = "policyd.audit";
const AUDIT_EMIT_LIMIT: usize = 128;
static AUDIT_EMIT_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug)]
enum AuditReason {
    Policy,
    IdentityMismatch,
}

#[derive(Clone, Copy, Debug)]
enum AuditDecision {
    Allow,
    Deny,
}

/// Minimal kernel-IPC backed policyd loop.
///
/// NOTE: This is a bring-up implementation: it only supports allow/deny checks over IPC and
/// returns deterministic decisions based on the compiled policy table.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    // Deterministic IPC slots are pre-distributed by init-lite (RFC-0005). Avoid routing queries
    // here to keep readiness marker ordering stable for `scripts/qemu-test.sh`.
    let server_recv_slot = 3;
    let server_send_slot = 4;
    notifier.notify();
    emit_line("policyd: ready");
    // Private init-lite -> policyd control channels.
    // Slot layout for policyd child (deterministic under current init-lite bring-up):
    // - slot 1/2: init-lite routing control REQ/RSP
    // - slot 3/4: selftest-client <-> policyd service RECV/SEND
    // - slot 5/6: init-lite <-> policyd route control RECV/SEND
    // - slot 7/8: init-lite <-> policyd exec control RECV/SEND
    let ctl_route_recv_slot = 5;
    let ctl_route_send_slot = 6;
    let ctl_exec_recv_slot = 7;
    let ctl_exec_send_slot = 8;
    let init_lite_id = nexus_abi::service_id_from_name(b"init-lite");
    let mut ctl_route_buf = [0u8; 512];
    let mut ctl_exec_buf = [0u8; 512];
    let mut server_buf = [0u8; 512];
    loop {
        // Multiplex both endpoints without blocking on one of them.
        let mut progressed = false;

        match recv_with_meta_nonblock(ctl_route_recv_slot, &mut ctl_route_buf) {
            Ok((_hdr, sender_service_id, n)) => {
                progressed = true;
                let rsp = handle_frame(
                    &ctl_route_buf[..n],
                    sender_service_id,
                    sender_service_id == init_lite_id,
                );
                let _ = send_reply_nonblock(ctl_route_send_slot, &rsp.buf[..rsp.len]);
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {}
            Err(_) => {}
        }

        match recv_with_meta_nonblock(ctl_exec_recv_slot, &mut ctl_exec_buf) {
            Ok((_hdr, sender_service_id, n)) => {
                progressed = true;
                let rsp = handle_frame(
                    &ctl_exec_buf[..n],
                    sender_service_id,
                    sender_service_id == init_lite_id,
                );
                let _ = send_reply_nonblock(ctl_exec_send_slot, &rsp.buf[..rsp.len]);
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {}
            Err(_) => {}
        }

        match recv_with_meta_nonblock(server_recv_slot, &mut server_buf) {
            Ok((_hdr, sender_service_id, n)) => {
                progressed = true;
                let rsp = handle_frame(
                    &server_buf[..n],
                    sender_service_id,
                    sender_service_id == init_lite_id,
                );
                let _ = send_reply_nonblock(server_send_slot, &rsp.buf[..rsp.len]);
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {}
            Err(_) => {}
        }

        if !progressed {
            let _ = yield_();
        }
    }
}

fn recv_with_meta_nonblock(
    recv_slot: u32,
    buf: &mut [u8],
) -> Result<(nexus_abi::MsgHeader, u64, usize), nexus_abi::IpcError> {
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut sid: u64 = 0;
    let n = nexus_abi::ipc_recv_v2(
        recv_slot,
        &mut hdr,
        buf,
        &mut sid,
        nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
        0,
    )?;
    Ok((hdr, sid, n as usize))
}

fn send_reply_nonblock(send_slot: u32, frame: &[u8]) -> Result<(), nexus_abi::IpcError> {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    nexus_abi::ipc_send_v1(send_slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0)?;
    Ok(())
}

struct FrameOut {
    buf: [u8; 10],
    len: usize,
}

fn handle_frame(frame: &[u8], sender_service_id: u64, privileged_proxy: bool) -> FrameOut {
    // v1 CHECK request: [P, O, ver=1, OP_CHECK, name_len:u8, name...]
    // v1 ROUTE request: [P, O, ver=1, OP_ROUTE, req_len:u8, req..., tgt_len:u8, tgt...]
    // v1 EXEC request:  [P, O, ver=1, OP_EXEC, req_len:u8, req..., image_id:u8]
    // v1 response:      [P, O, ver=1, op|0x80, status:u8, _reserved:u8]
    //
    // v2 ROUTE request: [P, O, ver=2, OP_ROUTE, nonce:u32le, req_len:u8, req..., tgt_len:u8, tgt...]
    // v2 EXEC request:  [P, O, ver=2, OP_EXEC,  nonce:u32le, req_len:u8, req..., image_id:u8]
    // v2 response:      [P, O, ver=2, op|0x80, nonce:u32le, status:u8, _reserved:u8]
    if frame.len() < 6 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp_v1(OP_CHECK, STATUS_MALFORMED);
    }
    let ver = frame[2];
    let op = frame[3];
    if ver == VERSION && op == OP_LOG_PROBE {
        let ok = append_probe_to_logd_deterministic();
        return rsp_v1(op, if ok { STATUS_ALLOW } else { STATUS_UNSUPPORTED });
    }
    match (ver, op) {
        (VERSION, OP_CHECK) => {
            // Debug: log CHECK request receipt.
            emit_line("policyd: CHECK rx");
            let n = frame[4] as usize;
            if frame.len() != 5 + n {
                emit_line("policyd: CHECK malformed");
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_bytes = &frame[5..];
            let requester_id = nexus_abi::service_id_from_name(requester_bytes);
            // Identity-binding hardening (v1):
            // never trust requester strings inside payloads; bind to sender_service_id unless init-lite is proxy.
            if !privileged_proxy && requester_id != sender_service_id {
                // Debug: log identity mismatch for selftest-client debugging.
                emit_line("policyd: CHECK id mismatch");
                emit_audit(op, AuditDecision::Deny, sender_service_id, None, AuditReason::IdentityMismatch);
                return rsp_v1(op, STATUS_DENY);
            }
            let status = if POLICY.allows(requester_id, CAP_CHECK) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                None,
                AuditReason::Policy,
            );
            rsp_v1(op, status)
        }
        (VERSION, OP_CHECK_CAP) => {
            // v1 CAP check request:
            // [P, O, ver=1, OP_CHECK_CAP, subject_id:u64le, cap_len:u8, cap...]
            if frame.len() < 4 + 8 + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_id = u64::from_le_bytes([
                frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
            ]);
            let cap_len = frame[12] as usize;
            if cap_len == 0 || cap_len > 48 || frame.len() != 13 + cap_len {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let cap = &frame[13..13 + cap_len];
            // Identity binding: ignore requester_id from payload unless caller is the privileged proxy (init-lite).
            let subject_id = if privileged_proxy {
                requester_id
            } else {
                sender_service_id
            };
            let status = if POLICY.allows(subject_id, core::str::from_utf8(cap).unwrap_or("")) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                subject_id,
                None,
                AuditReason::Policy,
            );
            rsp_v1(op, status)
        }
        (VERSION, OP_ROUTE) => {
            if frame.len() < 7 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_len = frame[4] as usize;
            if frame.len() < 5 + req_len + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_start = 5;
            let req_end = req_start + req_len;
            let tgt_len = frame[req_end] as usize;
            let tgt_start = req_end + 1;
            let tgt_end = tgt_start + tgt_len;
            if frame.len() != tgt_end {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_bytes = &frame[req_start..req_end];
            let requester_id = nexus_abi::service_id_from_name(requester_bytes);
            // Identity-binding hardening (v1):
            // never trust requester strings inside payloads; bind to sender_service_id unless init-lite is proxy.
            if !privileged_proxy && requester_id != sender_service_id {
                emit_audit(op, AuditDecision::Deny, sender_service_id, None, AuditReason::IdentityMismatch);
                return rsp_v1(op, STATUS_DENY);
            }
            let target_name = &frame[tgt_start..tgt_end];
            // Harden routing for sensitive targets without requiring a full target-aware policy model yet:
            // `execd` route lookups are denied unless explicitly granted.
            let status = if target_name == b"execd" {
                if POLICY.allows(requester_id, "route.execd") {
                    STATUS_ALLOW
                } else {
                    STATUS_DENY
                }
            } else if POLICY.allows(requester_id, CAP_ROUTE) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            let target_id = nexus_abi::service_id_from_name(target_name);
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                Some(target_id),
                AuditReason::Policy,
            );
            rsp_v1(op, status)
        }
        (VERSION, OP_EXEC) => {
            if frame.len() < 6 + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_len = frame[4] as usize;
            if req_len == 0 || req_len > 48 || frame.len() != 6 + req_len {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_bytes = &frame[5..5 + req_len];
            let requester_id = nexus_abi::service_id_from_name(requester_bytes);
            // Identity-binding hardening (v1):
            // never trust requester strings inside payloads; bind to sender_service_id unless init-lite is proxy.
            if !privileged_proxy && requester_id != sender_service_id {
                emit_audit(op, AuditDecision::Deny, sender_service_id, None, AuditReason::IdentityMismatch);
                return rsp_v1(op, STATUS_DENY);
            }
            let _image_id = frame[5 + req_len]; // reserved
            let status = if POLICY.allows(requester_id, CAP_EXEC) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                None,
                AuditReason::Policy,
            );
            rsp_v1(op, status)
        }
        (nexus_abi::policyd::VERSION_V2, nexus_abi::policyd::OP_ROUTE) => {
            let (nonce, requester, target) = match nexus_abi::policyd::decode_route_v2(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_ROUTE, 0, STATUS_MALFORMED),
            };
            let requester_id = nexus_abi::service_id_from_name(requester);
            if !privileged_proxy && requester_id != sender_service_id {
                emit_audit(
                    op,
                    AuditDecision::Deny,
                    sender_service_id,
                    Some(nexus_abi::service_id_from_name(target)),
                    AuditReason::IdentityMismatch,
                );
                return rsp_v2(nexus_abi::policyd::OP_ROUTE, nonce, STATUS_DENY);
            }
            let status = if target == b"execd" {
                if POLICY.allows(requester_id, "route.execd") {
                    STATUS_ALLOW
                } else {
                    STATUS_DENY
                }
            } else if POLICY.allows(requester_id, CAP_ROUTE) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                Some(nexus_abi::service_id_from_name(target)),
                AuditReason::Policy,
            );
            rsp_v2(nexus_abi::policyd::OP_ROUTE, nonce, status)
        }
        (nexus_abi::policyd::VERSION_V3, nexus_abi::policyd::OP_ROUTE) => {
            let (nonce, requester_id, target_id) =
                match nexus_abi::policyd::decode_route_v3_id(frame) {
                    Some(v) => v,
                    None => return rsp_v2(nexus_abi::policyd::OP_ROUTE, 0, STATUS_MALFORMED),
                };
            if !privileged_proxy && requester_id != sender_service_id {
                emit_audit(
                    op,
                    AuditDecision::Deny,
                    sender_service_id,
                    Some(target_id),
                    AuditReason::IdentityMismatch,
                );
                let buf = nexus_abi::policyd::encode_rsp_v3(
                    nexus_abi::policyd::OP_ROUTE,
                    nonce,
                    STATUS_DENY,
                );
                return FrameOut { buf, len: 10 };
            }
            let status = if target_id == nexus_abi::service_id_from_name(b"execd") {
                if POLICY.allows(requester_id, "route.execd") {
                    STATUS_ALLOW
                } else {
                    STATUS_DENY
                }
            } else if POLICY.allows(requester_id, CAP_ROUTE) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                Some(target_id),
                AuditReason::Policy,
            );
            let buf =
                nexus_abi::policyd::encode_rsp_v3(nexus_abi::policyd::OP_ROUTE, nonce, status);
            FrameOut { buf, len: 10 }
        }
        (nexus_abi::policyd::VERSION_V2, nexus_abi::policyd::OP_EXEC) => {
            let (nonce, requester, _image_id) = match nexus_abi::policyd::decode_exec_v2(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_EXEC, 0, STATUS_MALFORMED),
            };
            let requester_id = nexus_abi::service_id_from_name(requester);
            if !privileged_proxy && requester_id != sender_service_id {
                emit_audit(
                    op,
                    AuditDecision::Deny,
                    sender_service_id,
                    None,
                    AuditReason::IdentityMismatch,
                );
                return rsp_v2(nexus_abi::policyd::OP_EXEC, nonce, STATUS_DENY);
            }
            let status = if POLICY.allows(requester_id, CAP_EXEC) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                None,
                AuditReason::Policy,
            );
            rsp_v2(nexus_abi::policyd::OP_EXEC, nonce, status)
        }
        (nexus_abi::policyd::VERSION_V3, nexus_abi::policyd::OP_EXEC) => {
            let (nonce, requester_id, _image_id) =
                match nexus_abi::policyd::decode_exec_v3_id(frame) {
                    Some(v) => v,
                    None => return rsp_v2(nexus_abi::policyd::OP_EXEC, 0, STATUS_MALFORMED),
                };
            if !privileged_proxy && requester_id != sender_service_id {
                emit_audit(
                    op,
                    AuditDecision::Deny,
                    sender_service_id,
                    None,
                    AuditReason::IdentityMismatch,
                );
                let buf = nexus_abi::policyd::encode_rsp_v3(
                    nexus_abi::policyd::OP_EXEC,
                    nonce,
                    STATUS_DENY,
                );
                return FrameOut { buf, len: 10 };
            }
            let status = if POLICY.allows(requester_id, CAP_EXEC) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            emit_audit(
                op,
                if status == STATUS_ALLOW {
                    AuditDecision::Allow
                } else {
                    AuditDecision::Deny
                },
                sender_service_id,
                None,
                AuditReason::Policy,
            );
            let buf = nexus_abi::policyd::encode_rsp_v3(nexus_abi::policyd::OP_EXEC, nonce, status);
            FrameOut { buf, len: 10 }
        }
        _ => rsp_v1(op, STATUS_UNSUPPORTED),
    }
}

fn rsp_v1(op: u8, status: u8) -> FrameOut {
    let mut buf = [0u8; 10];
    buf[..6].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, op | 0x80, status, 0]);
    FrameOut { buf, len: 6 }
}

fn rsp_v2(op: u8, nonce: nexus_abi::policyd::Nonce, status: u8) -> FrameOut {
    let buf = nexus_abi::policyd::encode_rsp_v2(op, nonce, status);
    FrameOut { buf, len: 10 }
}

#[cfg(all(test, nexus_env = "os", feature = "os-lite"))]
mod tests {
    use super::*;

    fn rsp_status(frame: FrameOut) -> u8 {
        assert!(frame.len >= 5);
        frame.buf[4]
    }

    #[test]
    fn test_reject_requester_spoof_v1_route() {
        let requester = b"samgrd";
        let target = b"execd";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_ROUTE]);
        frame.push(requester.len() as u8);
        frame.extend_from_slice(requester);
        frame.push(target.len() as u8);
        frame.extend_from_slice(target);

        let sender_service_id = nexus_abi::service_id_from_name(b"bundlemgrd");
        let out = handle_frame(&frame, sender_service_id, false);
        assert_eq!(rsp_status(out), STATUS_DENY);
    }

    #[test]
    fn test_allow_init_lite_proxy_v1_route_even_if_requester_mismatch() {
        let requester = b"samgrd";
        let target = b"execd";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_ROUTE]);
        frame.push(requester.len() as u8);
        frame.extend_from_slice(requester);
        frame.push(target.len() as u8);
        frame.extend_from_slice(target);

        // init-lite is a privileged proxy during bring-up (owned by init chain).
        let sender_service_id = nexus_abi::service_id_from_name(b"init-lite");
        let out = handle_frame(&frame, sender_service_id, true);
        // The privileged proxy bypasses the sender-id binding check; policy decision is still
        // evaluated on requester/target (samgrd->execd is allowed in this v1 shim).
        assert_eq!(rsp_status(out), STATUS_ALLOW);
    }

    #[test]
    fn test_reject_requester_spoof_v2_route() {
        let nonce: nexus_abi::policyd::Nonce = 0x11223344;
        let requester = b"samgrd";
        let target = b"execd";

        let mut frame = Vec::new();
        frame.extend_from_slice(&[
            MAGIC0,
            MAGIC1,
            nexus_abi::policyd::VERSION_V2,
            nexus_abi::policyd::OP_ROUTE,
        ]);
        frame.extend_from_slice(&nonce.to_le_bytes());
        frame.push(requester.len() as u8);
        frame.extend_from_slice(requester);
        frame.push(target.len() as u8);
        frame.extend_from_slice(target);

        let sender_service_id = nexus_abi::service_id_from_name(b"bundlemgrd");
        let out = handle_frame(&frame, sender_service_id, false);
        assert_eq!(out.len, 10);
        assert_eq!(out.buf[0], MAGIC0);
        assert_eq!(out.buf[1], MAGIC1);
        assert_eq!(out.buf[2], nexus_abi::policyd::VERSION_V2);
        assert_eq!(out.buf[3], nexus_abi::policyd::OP_ROUTE | 0x80);
        assert_eq!(u32::from_le_bytes(out.buf[4..8].try_into().unwrap()), nonce);
        assert_eq!(out.buf[8], STATUS_DENY);
    }

    #[test]
    fn test_reject_malformed_exec_v1_oversized_requester() {
        let req_len = 49u8; // > 48 => malformed
        let requester = vec![b'a'; req_len as usize];
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_EXEC, req_len]);
        frame.extend_from_slice(&requester);
        frame.push(0); // image_id

        let sender_service_id = nexus_abi::service_id_from_name(b"samgrd");
        let out = handle_frame(&frame, sender_service_id, false);
        assert_eq!(rsp_status(out), STATUS_MALFORMED);
    }

    #[test]
    fn test_policy_check_allows_ipc_core_subject() {
        let subject = b"samgrd";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_CHECK]);
        frame.push(subject.len() as u8);
        frame.extend_from_slice(subject);

        let sender_service_id = nexus_abi::service_id_from_name(subject);
        let out = handle_frame(&frame, sender_service_id, false);
        assert_eq!(rsp_status(out), STATUS_ALLOW);
    }

    #[test]
    fn test_policy_check_denies_missing_capability() {
        let subject = b"demo.testsvc";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_CHECK]);
        frame.push(subject.len() as u8);
        frame.extend_from_slice(subject);

        let sender_service_id = nexus_abi::service_id_from_name(subject);
        let out = handle_frame(&frame, sender_service_id, false);
        assert_eq!(rsp_status(out), STATUS_DENY);
    }
}

fn append_probe_to_logd_deterministic() -> bool {
    append_logd_deterministic(b"policyd", b"core service log probe: policyd")
}

/// Stub transport runner retained for cross-module linkage.
pub fn run_with_transport_ready<T>(_: &mut T, notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("policyd: ready (stub transport)");
    Err(ServerError::Unsupported)
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

fn emit_audit(
    op: u8,
    decision: AuditDecision,
    subject_id: u64,
    target_id: Option<u64>,
    reason: AuditReason,
) {
    if AUDIT_EMIT_COUNT.fetch_add(1, Ordering::Relaxed) >= AUDIT_EMIT_LIMIT {
        return;
    }
    let mut buf = [0u8; 256];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"audit v1 op=");
    let _ = push_bytes(&mut buf, &mut len, audit_op_name(op));
    let _ = push_bytes(&mut buf, &mut len, b" decision=");
    let _ = push_bytes(&mut buf, &mut len, audit_decision_name(decision));
    let _ = push_bytes(&mut buf, &mut len, b" subject=0x");
    write_hex_u64(&mut buf, &mut len, subject_id);
    if let Some(target) = target_id {
        let _ = push_bytes(&mut buf, &mut len, b" target=0x");
        write_hex_u64(&mut buf, &mut len, target);
    }
    let _ = push_bytes(&mut buf, &mut len, b" reason=");
    let _ = push_bytes(&mut buf, &mut len, audit_reason_name(reason));
    let ok = append_logd_deterministic(AUDIT_SCOPE.as_bytes(), &buf[..len]);
    // Debug: trace audit emission
    if ok {
        emit_line("policyd: audit emit ok");
    } else {
        emit_line("policyd: audit emit FAIL");
    }
}

fn audit_op_name(op: u8) -> &'static [u8] {
    match op {
        OP_CHECK => b"check",
        OP_CHECK_CAP => b"check_cap",
        OP_ROUTE => b"route",
        OP_EXEC => b"exec",
        _ => b"unknown",
    }
}

fn audit_decision_name(decision: AuditDecision) -> &'static [u8] {
    match decision {
        AuditDecision::Allow => b"allow",
        AuditDecision::Deny => b"deny",
    }
}

fn audit_reason_name(reason: AuditReason) -> &'static [u8] {
    match reason {
        AuditReason::Policy => b"policy",
        AuditReason::IdentityMismatch => b"identity",
    }
}

fn push_bytes(buf: &mut [u8], len: &mut usize, bytes: &[u8]) -> bool {
    let available = buf.len().saturating_sub(*len);
    if bytes.len() > available {
        return false;
    }
    buf[*len..*len + bytes.len()].copy_from_slice(bytes);
    *len += bytes.len();
    true
}

fn write_hex_u64(buf: &mut [u8], len: &mut usize, value: u64) {
    if buf.len().saturating_sub(*len) < 16 {
        return;
    }
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        buf[*len] = ch;
        *len += 1;
    }
}

fn append_logd_deterministic(scope: &[u8], msg: &[u8]) -> bool {
    // Deterministic slots distributed by init-lite for policyd:
    // - reply inbox: recv=0x9 send=0xA
    // - logd sink:  send=0xB (responses land on reply inbox when using CAP_MOVE)
    const REPLY_RECV_SLOT: u32 = 0x9;
    const REPLY_SEND_SLOT: u32 = 0xA;
    const LOGD_SEND_SLOT: u32 = 0xB;

    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;

    if scope.len() > 64 || msg.len() > 256 {
        return false;
    }

    let mut frame = [0u8; 512];
    let mut len = 0usize;
    if frame.len()
        < 4 + 1 + 1 + 2 + 2 + scope.len().saturating_add(msg.len())
    {
        return false;
    }
    frame[len..len + 4].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    len += 4;
    frame[len] = LEVEL_INFO;
    len += 1;
    frame[len] = scope.len() as u8;
    len += 1;
    frame[len..len + 2].copy_from_slice(&(msg.len() as u16).to_le_bytes());
    len += 2;
    frame[len..len + 2].copy_from_slice(&0u16.to_le_bytes()); // fields_len
    len += 2;
    frame[len..len + scope.len()].copy_from_slice(scope);
    len += scope.len();
    frame[len..len + msg.len()].copy_from_slice(msg);
    len += msg.len();

    // Drain stale replies on the reply inbox.
    for _ in 0..8 {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64];
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(_) => continue,
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }

    let moved = match nexus_abi::cap_clone(REPLY_SEND_SLOT) {
        Ok(slot) => slot,
        Err(_) => return false,
    };
    let hdr = nexus_abi::MsgHeader::new(
        moved,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        len as u32,
    );

    // Send bounded NONBLOCK.
    let start = nexus_abi::nsec().ok().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000);
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(
            LOGD_SEND_SLOT,
            &hdr,
            &frame[..len],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        ) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().ok().unwrap_or(0);
                    if now >= deadline {
                        return false;
                    }
                }
                let _ = yield_();
            }
            Err(_) => return false,
        }
        i = i.wrapping_add(1);
    }

    // Drain the logd append response from the reply inbox (keeps queues bounded).
    let mut ah = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut abuf = [0u8; 64];
    loop {
        let now = nexus_abi::nsec().ok().unwrap_or(0);
        if now >= deadline {
            emit_line("policyd: audit logd timeout");
            return false;
        }
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut ah,
            &mut abuf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                // Debug: check response status
                if n >= 5 && abuf[0] == b'L' && abuf[1] == b'O' && abuf[2] == 1 && abuf[3] == (1 | 0x80) {
                    if abuf[4] != 0 {
                        // logd returned non-OK status
                        emit_line("policyd: audit logd FAIL status");
                    }
                }
                break;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => {
                emit_line("policyd: audit logd recv err");
                return false;
            }
        }
    }
    true
}
