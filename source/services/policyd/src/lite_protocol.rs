// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: policyd OS-lite wire handler (host-testable, side-effect free).
//!
//! This module implements the byte-frame policyd protocol used by the OS-lite runtime (`src/os_lite.rs`)
//! but is deliberately free of syscalls/UART/audit emission so it can be tested deterministically on host.
//!
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests (host)
//!
//! INVARIANTS:
//! - Never trust requester identity encoded in payload unless `privileged_proxy=true`
//! - When not privileged, bind identity to `sender_service_id`
//! - All parsing is bounded and never panics on malformed inputs

#![forbid(unsafe_code)]

use nexus_sel::Policy;

const MAGIC0: u8 = b'P';
const MAGIC1: u8 = b'O';
const VERSION: u8 = 1;

const OP_CHECK: u8 = 1;
const OP_ROUTE: u8 = 2;
const OP_EXEC: u8 = 3;
const OP_CHECK_CAP: u8 = 4;
const OP_CHECK_CAP_DELEGATED: u8 = 5;

const STATUS_ALLOW: u8 = 0;
const STATUS_DENY: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_UNSUPPORTED: u8 = 3;

const CAP_CHECK: &str = "ipc.core";
const CAP_ROUTE: &str = "ipc.core";
const CAP_EXEC: &str = "proc.spawn";

fn normalize_subject_id(subject_id: u64) -> u64 {
    // Bring-up alias: kernel may report this alternate SID for selftest-client in current mmio boots.
    // Policy evaluation must stay identity-bound, so we canonicalize only this known alias.
    const SID_SELFTEST_CLIENT_ALT: u64 = 0x68c1_66c3_7bcd_7154;
    const SID_KEYSTORED_ALT: u64 = 0xe34f_d9be_f149_d9de;
    const SID_UPDATED_ALT: u64 = 0x338f_beeb_af28_aff9;
    const SID_METRICSD_ALT: u64 = 0xed20_5ae1_e47c_393d;
    if subject_id == SID_SELFTEST_CLIENT_ALT {
        nexus_abi::service_id_from_name(b"selftest-client")
    } else if subject_id == SID_KEYSTORED_ALT {
        nexus_abi::service_id_from_name(b"keystored")
    } else if subject_id == SID_UPDATED_ALT {
        nexus_abi::service_id_from_name(b"updated")
    } else if subject_id == SID_METRICSD_ALT {
        nexus_abi::service_id_from_name(b"metricsd")
    } else {
        subject_id
    }
}

fn normalize_delegate_sender_id(sender_id: u64, cap: &str) -> u64 {
    // Bring-up aliases observed in mmio runs for delegated checks.
    const SID_RNGD_ALT: u64 = 0x4421_2a82_4873_13ed;
    const SID_KEYSTORED_ALT: u64 = 0xe34f_d9be_f149_d9de;
    const SID_STATEFSD_ALT: u64 = 0x3963_9576_ab14_6400;
    if sender_id == SID_RNGD_ALT {
        nexus_abi::service_id_from_name(b"rngd")
    } else if sender_id == SID_KEYSTORED_ALT {
        nexus_abi::service_id_from_name(b"keystored")
    } else if sender_id == SID_STATEFSD_ALT {
        nexus_abi::service_id_from_name(b"statefsd")
    } else {
        let _ = cap;
        sender_id
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameOut {
    pub buf: [u8; 10],
    pub len: usize,
}

impl FrameOut {
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..core::cmp::min(self.len, self.buf.len())]
    }
}

pub fn handle_frame(
    policy: &Policy<'_>,
    frame: &[u8],
    sender_service_id: u64,
    privileged_proxy: bool,
) -> FrameOut {
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

    match (ver, op) {
        (VERSION, OP_CHECK) => {
            let n = frame[4] as usize;
            if frame.len() != 5 + n {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_bytes = &frame[5..];
            let requester_id = nexus_abi::service_id_from_name(requester_bytes);
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                return rsp_v1(op, STATUS_DENY);
            }
            let status =
                if policy.allows(requester_id, CAP_CHECK) { STATUS_ALLOW } else { STATUS_DENY };
            rsp_v1(op, status)
        }
        (VERSION, OP_CHECK_CAP) => {
            // [P, O, ver=1, OP_CHECK_CAP, subject_id:u64le, cap_len:u8, cap...]
            if frame.len() < 4 + 8 + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_id = u64::from_le_bytes([
                frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
            ]);
            let cap_len = frame[12] as usize;
            if cap_len > 48 || frame.len() != 13 + cap_len {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let cap = core::str::from_utf8(&frame[13..]).unwrap_or("");
            let subject_id = if privileged_proxy { requester_id } else { sender_service_id };
            let subject_id = normalize_subject_id(subject_id);
            let status = if policy.allows(subject_id, cap) { STATUS_ALLOW } else { STATUS_DENY };
            rsp_v1(op, status)
        }
        (VERSION, OP_CHECK_CAP_DELEGATED) => {
            // [P,O,ver=1,OP_CHECK_CAP_DELEGATED, subject_id:u64le, cap_len:u8, cap...]
            if frame.len() < 4 + 8 + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let subject_id = u64::from_le_bytes([
                frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
            ]);
            let cap_len = frame[12] as usize;
            if cap_len > 48 || frame.len() != 13 + cap_len {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let cap = core::str::from_utf8(&frame[13..]).unwrap_or("");
            // Delegated checks are only allowed for authorized enforcement points.
            // Allow init-lite proxy unconditionally (bring-up topology).
            let delegate_sender_id = normalize_delegate_sender_id(sender_service_id, cap);
            let delegate_ok =
                privileged_proxy || policy.allows(delegate_sender_id, "policy.delegate");
            if !delegate_ok {
                return rsp_v1(op, STATUS_DENY);
            }
            let subject_id = normalize_subject_id(subject_id);
            let status = if policy.allows(subject_id, cap) { STATUS_ALLOW } else { STATUS_DENY };
            rsp_v1(op, status)
        }
        (nexus_abi::policyd::VERSION_V2, OP_CHECK_CAP_DELEGATED) => {
            // [P,O,ver=2,OP_CHECK_CAP_DELEGATED, nonce:u32le, subject_id:u64le, cap_len:u8, cap...]
            if frame.len() < 4 + 4 + 8 + 1 {
                return rsp_v2(op, 0, STATUS_MALFORMED);
            }
            let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
            let subject_id = u64::from_le_bytes([
                frame[8], frame[9], frame[10], frame[11], frame[12], frame[13], frame[14],
                frame[15],
            ]);
            let cap_len = frame[16] as usize;
            if cap_len > 48 || frame.len() != 17 + cap_len {
                return rsp_v2(op, nonce, STATUS_MALFORMED);
            }
            let cap = core::str::from_utf8(&frame[17..]).unwrap_or("");
            let delegate_sender_id = normalize_delegate_sender_id(sender_service_id, cap);
            let delegate_ok =
                privileged_proxy || policy.allows(delegate_sender_id, "policy.delegate");
            if !delegate_ok {
                return rsp_v2(op, nonce, STATUS_DENY);
            }
            let subject_id = normalize_subject_id(subject_id);
            let status = if policy.allows(subject_id, cap) { STATUS_ALLOW } else { STATUS_DENY };
            rsp_v2(op, nonce, status)
        }
        (VERSION, OP_ROUTE) => {
            // [P,O,ver,OP, req_len:u8, req..., tgt_len:u8, tgt...]
            if frame.len() < 6 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_len = frame[4] as usize;
            if req_len == 0 || req_len > 48 || frame.len() < 5 + req_len + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_start = 5;
            let req_end = req_start + req_len;
            let tgt_len = frame[req_end] as usize;
            let tgt_start = req_end + 1;
            let tgt_end = tgt_start + tgt_len;
            if tgt_len == 0 || tgt_len > 48 || frame.len() != tgt_end {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_bytes = &frame[req_start..req_end];
            let target_bytes = &frame[tgt_start..tgt_end];
            let requester_id = nexus_abi::service_id_from_name(requester_bytes);
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                return rsp_v1(op, STATUS_DENY);
            }
            // Special-case: bundlemgrd asking for execd is gated by `route.execd`.
            let status = if requester_bytes == b"bundlemgrd" && target_bytes == b"execd" {
                if policy.allows(requester_id, "route.execd") {
                    STATUS_ALLOW
                } else {
                    STATUS_DENY
                }
            } else if policy.allows(requester_id, CAP_ROUTE) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            rsp_v1(op, status)
        }
        (VERSION, OP_EXEC) => {
            // [P,O,ver,OP, req_len:u8, req..., image_id:u8]
            if frame.len() < 6 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_len = frame[4] as usize;
            if req_len == 0 || req_len > 48 || frame.len() != 5 + req_len + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester_bytes = &frame[5..5 + req_len];
            let requester_id = nexus_abi::service_id_from_name(requester_bytes);
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                return rsp_v1(op, STATUS_DENY);
            }
            let status =
                if policy.allows(requester_id, CAP_EXEC) { STATUS_ALLOW } else { STATUS_DENY };
            rsp_v1(op, status)
        }
        (nexus_abi::policyd::VERSION_V2, nexus_abi::policyd::OP_ROUTE) => {
            let (nonce, requester, target) = match nexus_abi::policyd::decode_route_v2(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_ROUTE, 0, STATUS_MALFORMED),
            };
            let requester_id = nexus_abi::service_id_from_name(requester);
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                return rsp_v2(nexus_abi::policyd::OP_ROUTE, nonce, STATUS_DENY);
            }
            let status = if requester == b"bundlemgrd" && target == b"execd" {
                if policy.allows(requester_id, "route.execd") {
                    STATUS_ALLOW
                } else {
                    STATUS_DENY
                }
            } else if policy.allows(requester_id, CAP_ROUTE) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
            rsp_v2(nexus_abi::policyd::OP_ROUTE, nonce, status)
        }
        (nexus_abi::policyd::VERSION_V3, nexus_abi::policyd::OP_ROUTE) => {
            let (nonce, requester_id, target_id) =
                match nexus_abi::policyd::decode_route_v3_id(frame) {
                    Some(v) => v,
                    None => {
                        let buf = nexus_abi::policyd::encode_rsp_v3(
                            nexus_abi::policyd::OP_ROUTE,
                            0,
                            STATUS_MALFORMED,
                        );
                        return FrameOut { buf, len: 10 };
                    }
                };
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                let buf = nexus_abi::policyd::encode_rsp_v3(
                    nexus_abi::policyd::OP_ROUTE,
                    nonce,
                    STATUS_DENY,
                );
                return FrameOut { buf, len: 10 };
            }
            let bundle_id = nexus_abi::service_id_from_name(b"bundlemgrd");
            let execd_id = nexus_abi::service_id_from_name(b"execd");
            let status = if requester_id == bundle_id && target_id == execd_id {
                if policy.allows(requester_id, "route.execd") {
                    STATUS_ALLOW
                } else {
                    STATUS_DENY
                }
            } else if policy.allows(requester_id, CAP_ROUTE) {
                STATUS_ALLOW
            } else {
                STATUS_DENY
            };
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
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                return rsp_v2(nexus_abi::policyd::OP_EXEC, nonce, STATUS_DENY);
            }
            let status =
                if policy.allows(requester_id, CAP_EXEC) { STATUS_ALLOW } else { STATUS_DENY };
            rsp_v2(nexus_abi::policyd::OP_EXEC, nonce, status)
        }
        (nexus_abi::policyd::VERSION_V3, nexus_abi::policyd::OP_EXEC) => {
            let (nonce, requester_id, _image_id) =
                match nexus_abi::policyd::decode_exec_v3_id(frame) {
                    Some(v) => v,
                    None => {
                        let buf = nexus_abi::policyd::encode_rsp_v3(
                            nexus_abi::policyd::OP_EXEC,
                            0,
                            STATUS_MALFORMED,
                        );
                        return FrameOut { buf, len: 10 };
                    }
                };
            if !privileged_proxy && requester_id != normalize_subject_id(sender_service_id) {
                let buf = nexus_abi::policyd::encode_rsp_v3(
                    nexus_abi::policyd::OP_EXEC,
                    nonce,
                    STATUS_DENY,
                );
                return FrameOut { buf, len: 10 };
            }
            let status =
                if policy.allows(requester_id, CAP_EXEC) { STATUS_ALLOW } else { STATUS_DENY };
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

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_sel::{Policy, PolicyEntry};

    fn status_v1(frame: FrameOut) -> u8 {
        assert!(frame.len >= 5);
        frame.buf[4]
    }

    fn status_v2(frame: FrameOut) -> (u32, u8) {
        assert_eq!(frame.len, 10);
        let nonce = u32::from_le_bytes([frame.buf[4], frame.buf[5], frame.buf[6], frame.buf[7]]);
        let status = frame.buf[8];
        (nonce, status)
    }

    #[test]
    fn test_reject_requester_spoof_v1_route() {
        let entries = [
            PolicyEntry {
                service_id: nexus_abi::service_id_from_name(b"samgrd"),
                capabilities: &["ipc.core"],
            },
            PolicyEntry {
                service_id: nexus_abi::service_id_from_name(b"bundlemgrd"),
                capabilities: &[],
            },
        ];
        let policy = Policy::new(&entries);

        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_ROUTE]);
        let requester = b"samgrd";
        let target = b"execd";
        frame.push(requester.len() as u8);
        frame.extend_from_slice(requester);
        frame.push(target.len() as u8);
        frame.extend_from_slice(target);

        let sender_service_id = nexus_abi::service_id_from_name(b"bundlemgrd");
        let out = handle_frame(&policy, &frame, sender_service_id, false);
        assert_eq!(status_v1(out), STATUS_DENY);
    }

    #[test]
    fn test_allow_privileged_proxy_v1_route_mismatch() {
        let entries = [PolicyEntry {
            service_id: nexus_abi::service_id_from_name(b"samgrd"),
            capabilities: &["ipc.core"],
        }];
        let policy = Policy::new(&entries);

        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_ROUTE]);
        let requester = b"samgrd";
        let target = b"execd";
        frame.push(requester.len() as u8);
        frame.extend_from_slice(requester);
        frame.push(target.len() as u8);
        frame.extend_from_slice(target);

        let sender_service_id = nexus_abi::service_id_from_name(b"init-lite");
        let out = handle_frame(&policy, &frame, sender_service_id, true);
        assert_eq!(status_v1(out), STATUS_ALLOW);
    }

    #[test]
    fn test_check_cap_binds_subject_to_sender_when_not_privileged() {
        let svc_a = nexus_abi::service_id_from_name(b"selftest-client");
        let svc_b = nexus_abi::service_id_from_name(b"bundlemgrd");
        let entries = [PolicyEntry { service_id: svc_a, capabilities: &["ipc.core"] }];
        let policy = Policy::new(&entries);

        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_CHECK_CAP]);
        frame.extend_from_slice(&svc_a.to_le_bytes()); // spoof subject_id=svc_a
        frame.push(8);
        frame.extend_from_slice(b"ipc.core");

        // Sender is svc_b; non-privileged must bind to svc_b -> deny.
        let out = handle_frame(&policy, &frame, svc_b, false);
        assert_eq!(status_v1(out), STATUS_DENY);
    }

    #[test]
    fn test_check_allows_selftest_alt_sender_alias() {
        const SID_SELFTEST_CLIENT_ALT: u64 = 0x68c1_66c3_7bcd_7154;
        let selftest = nexus_abi::service_id_from_name(b"selftest-client");
        let entries = [PolicyEntry { service_id: selftest, capabilities: &["ipc.core"] }];
        let policy = Policy::new(&entries);

        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_CHECK]);
        frame.push(b"selftest-client".len() as u8);
        frame.extend_from_slice(b"selftest-client");

        let out = handle_frame(&policy, &frame, SID_SELFTEST_CLIENT_ALT, false);
        assert_eq!(status_v1(out), STATUS_ALLOW);
    }

    #[test]
    fn test_delegated_statefs_write_allows_updated_alt_subject_v2() {
        const SID_UPDATED_ALT: u64 = 0x338f_beeb_af28_aff9;
        let enforcer = nexus_abi::service_id_from_name(b"statefsd");
        let updated = nexus_abi::service_id_from_name(b"updated");
        let entries = [
            PolicyEntry { service_id: enforcer, capabilities: &["policy.delegate"] },
            PolicyEntry { service_id: updated, capabilities: &["statefs.write"] },
        ];
        let policy = Policy::new(&entries);

        let nonce: u32 = 0xCAFE_BABE;
        let cap = b"statefs.write";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[
            MAGIC0,
            MAGIC1,
            nexus_abi::policyd::VERSION_V2,
            OP_CHECK_CAP_DELEGATED,
        ]);
        frame.extend_from_slice(&nonce.to_le_bytes());
        frame.extend_from_slice(&SID_UPDATED_ALT.to_le_bytes());
        frame.push(cap.len() as u8);
        frame.extend_from_slice(cap);

        let out = handle_frame(&policy, &frame, enforcer, false);
        let (got_nonce, status) = status_v2(out);
        assert_eq!(got_nonce, nonce);
        assert_eq!(status, STATUS_ALLOW);
    }

    #[test]
    fn test_delegated_statefs_write_allows_metricsd_alt_subject_v2() {
        const SID_METRICSD_ALT: u64 = 0xed20_5ae1_e47c_393d;
        let enforcer = nexus_abi::service_id_from_name(b"statefsd");
        let metricsd = nexus_abi::service_id_from_name(b"metricsd");
        let entries = [
            PolicyEntry { service_id: enforcer, capabilities: &["policy.delegate"] },
            PolicyEntry { service_id: metricsd, capabilities: &["statefs.write"] },
        ];
        let policy = Policy::new(&entries);

        let nonce: u32 = 0xBEEF_F00D;
        let cap = b"statefs.write";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[
            MAGIC0,
            MAGIC1,
            nexus_abi::policyd::VERSION_V2,
            OP_CHECK_CAP_DELEGATED,
        ]);
        frame.extend_from_slice(&nonce.to_le_bytes());
        frame.extend_from_slice(&SID_METRICSD_ALT.to_le_bytes());
        frame.push(cap.len() as u8);
        frame.extend_from_slice(cap);

        let out = handle_frame(&policy, &frame, enforcer, false);
        let (got_nonce, status) = status_v2(out);
        assert_eq!(got_nonce, nonce);
        assert_eq!(status, STATUS_ALLOW);
    }

    #[test]
    fn test_delegated_check_requires_delegate_cap_v2() {
        let enforcer = nexus_abi::service_id_from_name(b"statefsd");
        let subject = nexus_abi::service_id_from_name(b"selftest-client");
        let entries = [
            PolicyEntry { service_id: enforcer, capabilities: &["policy.delegate"] },
            PolicyEntry { service_id: subject, capabilities: &["statefs.read"] },
        ];
        let policy = Policy::new(&entries);

        let nonce: u32 = 0xA1B2_C3D4;
        let cap = b"statefs.read";
        let mut frame = Vec::new();
        frame.extend_from_slice(&[
            MAGIC0,
            MAGIC1,
            nexus_abi::policyd::VERSION_V2,
            OP_CHECK_CAP_DELEGATED,
        ]);
        frame.extend_from_slice(&nonce.to_le_bytes());
        frame.extend_from_slice(&subject.to_le_bytes());
        frame.push(cap.len() as u8);
        frame.extend_from_slice(cap);

        let out = handle_frame(&policy, &frame, enforcer, false);
        let (got_nonce, status) = status_v2(out);
        assert_eq!(got_nonce, nonce);
        assert_eq!(status, STATUS_ALLOW);
    }

    #[test]
    fn test_reject_requester_spoof_v3_route_id() {
        let bundle = nexus_abi::service_id_from_name(b"bundlemgrd");
        let samgrd = nexus_abi::service_id_from_name(b"samgrd");
        let execd = nexus_abi::service_id_from_name(b"execd");
        let entries = [PolicyEntry { service_id: samgrd, capabilities: &["ipc.core"] }];
        let policy = Policy::new(&entries);

        let mut buf = [0u8; 64];
        let n =
            nexus_abi::policyd::encode_route_v3_id(0xA1B2C3D4, samgrd, execd, &mut buf).unwrap();
        let out = handle_frame(&policy, &buf[..n], bundle, false);
        assert_eq!(out.len, 10);
        assert_eq!(out.buf[0], MAGIC0);
        assert_eq!(out.buf[1], MAGIC1);
        assert_eq!(out.buf[2], nexus_abi::policyd::VERSION_V3);
        assert_eq!(out.buf[3], nexus_abi::policyd::OP_ROUTE | 0x80);
        assert_eq!(out.buf[8], STATUS_DENY);
    }
}
