// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")),
    forbid(unsafe_code)
)]
#![deny(clippy::all, missing_docs)]

//! CONTEXT: Shared ABI definitions exposed to userland crates
//! OWNERS: @runtime
//! PUBLIC API: MsgHeader, IpcError; OS-only syscalls: yield_, spawn, exit, wait, cap_transfer, as_*, vmo_*, debug_*
//! DEPENDS_ON: no_std (OS), riscv ecall asm (OS), bitflags
//! INVARIANTS: Header is 16 bytes LE; userspace wrappers map to stable kernel syscall IDs
//! ADR: docs/adr/0016-kernel-libs-architecture.md

/// Result type returned by ABI helpers.
pub type Result<T> = core::result::Result<T, IpcError>;

/// Errors surfaced by IPC syscalls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcError {
    /// Referenced endpoint is not present in the router.
    NoSuchEndpoint,
    /// Target queue ran out of space.
    QueueFull,
    /// Queue did not contain a message when operating in non-blocking mode.
    QueueEmpty,
    /// Caller lacks permission to perform the requested operation.
    PermissionDenied,
    /// Blocking IPC operation hit its deadline.
    TimedOut,
    /// Not enough resources to complete the IPC operation (e.g. receiver cap table full).
    NoSpace,
    /// IPC is not supported for this configuration.
    Unsupported,
}

/// IPC message header shared between kernel and userland.
#[repr(C, align(4))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsgHeader {
    /// Source capability slot.
    pub src: u32,
    /// Destination endpoint identifier.
    pub dst: u32,
    /// Message opcode.
    pub ty: u16,
    /// Transport flags.
    pub flags: u16,
    /// Inline payload length.
    pub len: u32,
}

/// IPC message header flags.
///
/// These are interpreted by the kernel IPC transport (not by service-level protocols).
pub mod ipc_hdr {
    /// Move one capability with the message (Phase‑2 scalability/hardening).
    ///
    /// When sending with this flag:
    /// - `MsgHeader.src` is treated as a **capability slot** in the sender and is **consumed**.
    /// - On receive, `MsgHeader.src` is overwritten with the **newly allocated capability slot**
    ///   in the receiver.
    pub const CAP_MOVE: u16 = 1 << 0;
}

/// Execd service frames (bring-up) used for OS-lite exec spawning.
///
/// This is intentionally minimal and byte-oriented (no IDL) to keep early boot deterministic.
pub mod execd {
    /// Frame magic (`'E','X'`).
    #[doc = "First magic byte (`'E'`)."]
    pub const MAGIC0: u8 = b'E';
    #[doc = "Second magic byte (`'X'`)."]
    pub const MAGIC1: u8 = b'X';
    /// Protocol version.
    pub const VERSION: u8 = 1;

    /// Exec image request opcode.
    pub const OP_EXEC_IMAGE: u8 = 1;

    /// Status: operation succeeded.
    pub const STATUS_OK: u8 = 0;
    /// Status: request was malformed.
    pub const STATUS_MALFORMED: u8 = 1;
    /// Status: unsupported operation/version.
    pub const STATUS_UNSUPPORTED: u8 = 2;
    /// Status: exec failed.
    pub const STATUS_FAILED: u8 = 3;
    /// Status: denied by policy.
    pub const STATUS_DENIED: u8 = 4;

    /// Maximum supported requester-name length (bytes).
    pub const MAX_REQUESTER_LEN: usize = 48;

    /// Encodes an `execd` v1 exec-image request frame.
    ///
    /// Frame: `[E,X,ver,OP_EXEC_IMAGE,image_id,stack_pages:u8, requester_len:u8, requester...]`
    pub fn encode_exec_image_req(
        requester: &[u8],
        image_id: u8,
        stack_pages: u8,
        out: &mut [u8],
    ) -> Option<usize> {
        if requester.is_empty()
            || requester.len() > MAX_REQUESTER_LEN
            || out.len() < 7 + requester.len()
        {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_EXEC_IMAGE;
        out[4] = image_id;
        out[5] = stack_pages;
        out[6] = requester.len() as u8;
        out[7..7 + requester.len()].copy_from_slice(requester);
        Some(7 + requester.len())
    }

    /// Decodes an `execd` v1 exec-image request frame.
    pub fn decode_exec_image_req(frame: &[u8]) -> Option<(u8, u8, &[u8])> {
        if frame.len() < 7 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != OP_EXEC_IMAGE {
            return None;
        }
        let image_id = frame[4];
        let stack_pages = frame[5];
        let n = frame[6] as usize;
        if n == 0 || n > MAX_REQUESTER_LEN || frame.len() != 7 + n {
            return None;
        }
        Some((image_id, stack_pages, &frame[7..]))
    }

    /// Encodes an `execd` v1 response frame.
    ///
    /// Frame: `[E,X,ver,OP_EXEC_IMAGE|0x80,status:u8,pid:u32le]`
    pub fn encode_exec_image_rsp(status: u8, pid: u32) -> [u8; 9] {
        let mut out = [0u8; 9];
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_EXEC_IMAGE | 0x80;
        out[4] = status;
        out[5..9].copy_from_slice(&pid.to_le_bytes());
        out
    }

    /// Decodes an `execd` v1 response frame and returns (status, pid).
    pub fn decode_exec_image_rsp(frame: &[u8]) -> Option<(u8, u32)> {
        if frame.len() != 9 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_EXEC_IMAGE | 0x80) {
            return None;
        }
        let status = frame[4];
        let pid = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
        Some((status, pid))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn execd_req_golden() {
            let requester = b"selftest-client";
            let mut buf = [0u8; 64];
            let n = encode_exec_image_req(requester, 1, 19, &mut buf).unwrap();
            const GOLDEN_PREFIX: [u8; 7] = [b'E', b'X', 1, 1, 1, 19, 15];
            assert_eq!(&buf[..7], &GOLDEN_PREFIX);
            assert_eq!(&buf[7..n], requester);
            let (img, sp, req) = decode_exec_image_req(&buf[..n]).unwrap();
            assert_eq!(img, 1);
            assert_eq!(sp, 19);
            assert_eq!(req, requester);
        }

        #[test]
        fn execd_rsp_golden() {
            let frame = encode_exec_image_rsp(STATUS_OK, 0x1122_3344);
            const GOLDEN: [u8; 9] = [b'E', b'X', 1, 0x81, 0, 0x44, 0x33, 0x22, 0x11];
            assert_eq!(frame, GOLDEN);
            let (status, pid) = decode_exec_image_rsp(&frame).unwrap();
            assert_eq!(status, STATUS_OK);
            assert_eq!(pid, 0x1122_3344);
        }
    }
}

/// Updated service frames (system-set staging + boot control).
pub mod updated {
    #![allow(missing_docs)]
    /// First magic byte (`'U'`).
    pub const MAGIC0: u8 = b'U';
    /// Second magic byte (`'D'`).
    pub const MAGIC1: u8 = b'D';
    /// Protocol version.
    pub const VERSION: u8 = 1;

    /// Stage system-set request opcode.
    pub const OP_STAGE: u8 = 1;
    /// Switch to staged slot opcode.
    pub const OP_SWITCH: u8 = 2;
    /// Commit health for pending slot opcode.
    pub const OP_HEALTH_OK: u8 = 3;
    /// Query status opcode.
    pub const OP_GET_STATUS: u8 = 4;
    /// Record a boot attempt (decrement tries / trigger rollback).
    pub const OP_BOOT_ATTEMPT: u8 = 5;

    /// Status: operation succeeded.
    pub const STATUS_OK: u8 = 0;
    /// Status: request was malformed.
    pub const STATUS_MALFORMED: u8 = 1;
    /// Status: unsupported operation/version.
    pub const STATUS_UNSUPPORTED: u8 = 2;
    /// Status: operation failed.
    pub const STATUS_FAILED: u8 = 3;

    /// Maximum inline system-set bytes for stage requests.
    pub const MAX_STAGE_BYTES: usize = 8 * 1024;

    /// Encodes a stage request frame.
    ///
    /// Frame: `[U, D, ver, OP_STAGE, len:u32le, bytes...]`
    pub fn encode_stage_req(bytes: &[u8], out: &mut [u8]) -> Option<usize> {
        if bytes.is_empty() || bytes.len() > MAX_STAGE_BYTES || out.len() < 8 + bytes.len() {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_STAGE;
        let len = bytes.len() as u32;
        out[4..8].copy_from_slice(&len.to_le_bytes());
        out[8..8 + bytes.len()].copy_from_slice(bytes);
        Some(8 + bytes.len())
    }

    /// Decodes a stage request frame and returns the payload bytes.
    pub fn decode_stage_req(frame: &[u8]) -> Option<&[u8]> {
        if frame.len() < 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != OP_STAGE {
            return None;
        }
        let len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
        if len == 0 || len > MAX_STAGE_BYTES || frame.len() != 8 + len {
            return None;
        }
        Some(&frame[8..8 + len])
    }

    /// Encodes a switch request frame.
    ///
    /// Frame: `[U, D, ver, OP_SWITCH, tries_left:u8]`
    pub fn encode_switch_req(tries_left: u8, out: &mut [u8]) -> Option<usize> {
        if tries_left == 0 || out.len() < 5 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_SWITCH;
        out[4] = tries_left;
        Some(5)
    }

    /// Decodes a switch request frame and returns tries_left.
    pub fn decode_switch_req(frame: &[u8]) -> Option<u8> {
        if frame.len() != 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != OP_SWITCH {
            return None;
        }
        let tries = frame[4];
        if tries == 0 {
            return None;
        }
        Some(tries)
    }

    /// Encodes a health-ok request frame.
    ///
    /// Frame: `[U, D, ver, OP_HEALTH_OK]`
    pub fn encode_health_ok_req(out: &mut [u8]) -> Option<usize> {
        if out.len() < 4 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_HEALTH_OK;
        Some(4)
    }

    /// Decodes a health-ok request frame.
    pub fn decode_health_ok_req(frame: &[u8]) -> bool {
        frame.len() == 4
            && frame[0] == MAGIC0
            && frame[1] == MAGIC1
            && frame[2] == VERSION
            && frame[3] == OP_HEALTH_OK
    }

    /// Encodes a get-status request frame.
    ///
    /// Frame: `[U, D, ver, OP_GET_STATUS]`
    pub fn encode_get_status_req(out: &mut [u8]) -> Option<usize> {
        if out.len() < 4 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_GET_STATUS;
        Some(4)
    }

    /// Decodes a get-status request frame.
    pub fn decode_get_status_req(frame: &[u8]) -> bool {
        frame.len() == 4
            && frame[0] == MAGIC0
            && frame[1] == MAGIC1
            && frame[2] == VERSION
            && frame[3] == OP_GET_STATUS
    }

    /// Decodes a boot-attempt response and returns (status, slot).
    pub fn decode_boot_attempt_rsp(frame: &[u8]) -> Option<(u8, u8)> {
        if frame.len() != 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_BOOT_ATTEMPT | 0x80) {
            return None;
        }
        let status = frame[4];
        let slot = frame[5];
        Some((status, slot))
    }

    /// Encodes a boot-attempt request frame.
    ///
    /// Frame: `[U, D, ver, OP_BOOT_ATTEMPT]`
    pub fn encode_boot_attempt_req(out: &mut [u8]) -> Option<usize> {
        if out.len() < 4 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_BOOT_ATTEMPT;
        Some(4)
    }

    /// Decodes a boot-attempt request frame.
    pub fn decode_boot_attempt_req(frame: &[u8]) -> bool {
        frame.len() == 4
            && frame[0] == MAGIC0
            && frame[1] == MAGIC1
            && frame[2] == VERSION
            && frame[3] == OP_BOOT_ATTEMPT
    }

    /// Decodes the opcode from a request frame.
    pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
        if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        Some(frame[3])
    }
}

/// Bootstrap routing protocol frames shared between init-lite and services (RFC-0005).
pub mod routing {
    /// Frame magic bytes (`'R','T'`) to avoid accidental collisions with other message formats.
    #[doc = "First magic byte (`'R'`)."]
    const MAGIC0: u8 = b'R';
    #[doc = "Second magic byte (`'T'`)."]
    const MAGIC1: u8 = b'T';
    /// Routing protocol version.
    pub const VERSION: u8 = 1;

    /// Route query opcode.
    pub const OP_ROUTE_GET: u8 = 0x40;
    /// Route response opcode.
    pub const OP_ROUTE_RSP: u8 = 0x41;

    /// Status code returned in ROUTE_RSP.
    pub const STATUS_OK: u8 = 0;
    /// Service is unknown or not routed for the caller.
    pub const STATUS_NOT_FOUND: u8 = 1;
    /// Request was malformed.
    pub const STATUS_MALFORMED: u8 = 2;
    /// Request was understood but denied by policy.
    pub const STATUS_DENIED: u8 = 3;

    /// Maximum supported service-name length in routing frames.
    pub const MAX_SERVICE_NAME_LEN: usize = 48;

    /// Encodes a ROUTE_GET request into `out` and returns the number of bytes written.
    pub fn encode_route_get(name: &[u8], out: &mut [u8]) -> Option<usize> {
        if name.is_empty() || name.len() > MAX_SERVICE_NAME_LEN || out.len() < 5 + name.len() {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_ROUTE_GET;
        out[4] = name.len() as u8;
        out[5..5 + name.len()].copy_from_slice(name);
        Some(5 + name.len())
    }

    /// Decodes a ROUTE_GET request and returns the requested service name.
    pub fn decode_route_get(frame: &[u8]) -> Option<&[u8]> {
        if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != OP_ROUTE_GET {
            return None;
        }
        let n = frame[4] as usize;
        if n == 0 || n > MAX_SERVICE_NAME_LEN || frame.len() != 5 + n {
            return None;
        }
        Some(&frame[5..])
    }

    /// Encodes a ROUTE_RSP response.
    pub fn encode_route_rsp(status: u8, send_slot: u32, recv_slot: u32) -> [u8; 13] {
        let mut out = [0u8; 13];
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_ROUTE_RSP;
        out[4] = status;
        out[5..9].copy_from_slice(&send_slot.to_le_bytes());
        out[9..13].copy_from_slice(&recv_slot.to_le_bytes());
        out
    }

    /// Decodes a ROUTE_RSP response and returns (status, send_slot, recv_slot).
    pub fn decode_route_rsp(frame: &[u8]) -> Option<(u8, u32, u32)> {
        if frame.len() != 13 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != OP_ROUTE_RSP {
            return None;
        }
        let status = frame[4];
        let send_slot = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
        let recv_slot = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
        Some((status, send_slot, recv_slot))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn route_get_golden() {
            let name = b"vfsd";
            let mut buf = [0u8; 32];
            let n = encode_route_get(name, &mut buf).expect("encode");
            const GOLDEN: [u8; 9] = [b'R', b'T', 1, 0x40, 4, b'v', b'f', b's', b'd'];
            assert_eq!(&buf[..n], &GOLDEN);
            assert_eq!(decode_route_get(&buf[..n]).unwrap(), name);
        }

        #[test]
        fn route_get_roundtrip() {
            let name = b"vfsd";
            let mut buf = [0u8; 32];
            let n = encode_route_get(name, &mut buf).expect("encode");
            assert_eq!(decode_route_get(&buf[..n]).unwrap(), name);
        }

        #[test]
        fn route_rsp_golden() {
            let frame = encode_route_rsp(STATUS_OK, 0x1122_3344, 0xAABB_CCDD);
            const GOLDEN: [u8; 13] = [
                b'R', b'T', 1, 0x41, 0, // status OK
                0x44, 0x33, 0x22, 0x11, // send_slot LE
                0xDD, 0xCC, 0xBB, 0xAA, // recv_slot LE
            ];
            assert_eq!(frame, GOLDEN);
            let (status, send, recv) = decode_route_rsp(&frame).unwrap();
            assert_eq!(status, STATUS_OK);
            assert_eq!(send, 0x1122_3344);
            assert_eq!(recv, 0xAABB_CCDD);
        }

        #[test]
        fn route_rsp_roundtrip() {
            let frame = encode_route_rsp(STATUS_OK, 12, 34);
            let (status, send, recv) = decode_route_rsp(&frame).unwrap();
            assert_eq!(status, STATUS_OK);
            assert_eq!(send, 12);
            assert_eq!(recv, 34);
        }
    }
}

/// Bundle manager (bundlemgrd) service frames used for OS bring-up.
///
/// This is intentionally minimal and byte-oriented (no IDL) to keep early boot deterministic.
pub mod bundlemgrd {
    /// Frame magic (`'B','N'`).
    #[doc = "First magic byte (`'B'`)."]
    pub const MAGIC0: u8 = b'B';
    #[doc = "Second magic byte (`'N'`)."]
    pub const MAGIC1: u8 = b'N';
    /// Protocol version.
    pub const VERSION: u8 = 1;

    /// List installed bundles (bring-up only).
    pub const OP_LIST: u8 = 1;
    /// Probe routing status of a target (bring-up only; used for policyd-gated denial proofs).
    pub const OP_ROUTE_STATUS: u8 = 2;
    /// Fetch a read-only bundle image containing one or more entries.
    pub const OP_FETCH_IMAGE: u8 = 3;
    /// Set the active slot for publication (`a` or `b`).
    pub const OP_SET_ACTIVE_SLOT: u8 = 4;

    /// Operation succeeded.
    pub const STATUS_OK: u8 = 0;
    /// Request frame was malformed.
    pub const STATUS_MALFORMED: u8 = 1;
    /// Operation is not supported by this build.
    pub const STATUS_UNSUPPORTED: u8 = 2;

    /// Encodes a LIST request.
    pub fn encode_list(out: &mut [u8; 4]) {
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_LIST;
    }

    /// Decodes the request opcode from a bundlemgrd v1 request frame.
    pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
        if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        Some(frame[3])
    }

    /// Decodes a LIST response and returns (status, count).
    pub fn decode_list_rsp(frame: &[u8]) -> Option<(u8, u16)> {
        if frame.len() != 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_LIST | 0x80) {
            return None;
        }
        let status = frame[4];
        let count = u16::from_le_bytes([frame[5], frame[6]]);
        Some((status, count))
    }

    /// Encodes a FETCH_IMAGE request.
    pub fn encode_fetch_image(out: &mut [u8; 4]) {
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_FETCH_IMAGE;
    }

    /// Decodes a FETCH_IMAGE response and returns (status, image_bytes).
    pub fn decode_fetch_image_rsp(frame: &[u8]) -> Option<(u8, &[u8])> {
        // Header: [B,N,ver,op|0x80,status,len:u32le] => 9 bytes, then payload.
        if frame.len() < 9 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_FETCH_IMAGE | 0x80) {
            return None;
        }
        let status = frame[4];
        let len = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]) as usize;
        if frame.len() != 9 + len {
            return None;
        }
        Some((status, &frame[9..]))
    }

    /// Encodes a SET_ACTIVE_SLOT request.
    ///
    /// Frame: `[B, N, ver, OP_SET_ACTIVE_SLOT, slot:u8]`
    pub fn encode_set_active_slot_req(slot: u8, out: &mut [u8; 5]) {
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_SET_ACTIVE_SLOT;
        out[4] = slot;
    }

    /// Decodes a SET_ACTIVE_SLOT response and returns (status, slot).
    pub fn decode_set_active_slot_rsp(frame: &[u8]) -> Option<(u8, u8)> {
        if frame.len() != 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_SET_ACTIVE_SLOT | 0x80) {
            return None;
        }
        let status = frame[4];
        let slot = frame[5];
        Some((status, slot))
    }

    /// List installed apps for the launcher / Apps menu (RFC-0065 dynamic apps menu).
    pub const OP_LIST_APPS: u8 = 5;

    /// Byte offset where LIST_APPS response entries begin (after status + count).
    pub const LIST_APPS_BODY_OFFSET: usize = 7;

    /// Encodes a LIST_APPS request: `[B, N, ver, OP_LIST_APPS]`.
    pub fn encode_list_apps(out: &mut [u8; 4]) {
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_LIST_APPS;
    }

    /// Decodes the LIST_APPS response header → `(status, count)`.
    ///
    /// Response frame:
    /// `[B, N, ver, OP_LIST_APPS|0x80, status:u8, count:u16le, entries...]`
    /// where each entry is `[id_len:u8, id..., label_len:u8, label...]` (UTF-8).
    /// Entry parsing (which needs allocation) lives in the consumer.
    pub fn decode_list_apps_header(frame: &[u8]) -> Option<(u8, u16)> {
        if frame.len() < LIST_APPS_BODY_OFFSET
            || frame[0] != MAGIC0
            || frame[1] != MAGIC1
            || frame[2] != VERSION
        {
            return None;
        }
        if frame[3] != (OP_LIST_APPS | 0x80) {
            return None;
        }
        Some((frame[4], u16::from_le_bytes([frame[5], frame[6]])))
    }

    /// Fetch an app's UI-program payload into a caller-provided VMO
    /// (TASK-0080D GET_PAYLOAD). Request:
    /// `[B, N, ver, OP_GET_PAYLOAD, id_len:u8, id...]` with the payload VMO
    /// capability MOVED alongside the message (CAP_MOVE — the gpud-attach /
    /// ADR-0042 SURFACE_CREATE pattern; the message's single cap slot carries
    /// the VMO, so there is no reply frame). bundlemgrd writes the payload
    /// bytes at [`PAYLOAD_DATA_OFFSET`], then the header LAST (header-last =
    /// release ordering for the single writer); the consumer polls the header.
    pub const OP_GET_PAYLOAD: u8 = 6;

    /// Payload-VMO header magic (`"NXPL"`), written after the payload bytes.
    pub const PAYLOAD_MAGIC: [u8; 4] = *b"NXPL";
    /// Header length; the payload bytes start here (8-byte aligned for the
    /// canonical `.nxir` capnp contract).
    pub const PAYLOAD_DATA_OFFSET: usize = 16;
    /// Header status: payload written completely.
    pub const PAYLOAD_STATUS_OK: u8 = 1;
    /// Header status: the app id has no ui-program payload.
    pub const PAYLOAD_STATUS_UNKNOWN: u8 = 2;
    /// Header status: the payload exceeds the provided VMO.
    pub const PAYLOAD_STATUS_TOO_LARGE: u8 = 3;

    /// Encodes a GET_PAYLOAD request; returns the frame length
    /// (`None` when the id is empty or exceeds the routing name bound).
    pub fn encode_get_payload(app_id: &[u8], out: &mut [u8]) -> Option<usize> {
        if app_id.is_empty() || app_id.len() > 48 || out.len() < 5 + app_id.len() {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_GET_PAYLOAD;
        out[4] = app_id.len() as u8;
        out[5..5 + app_id.len()].copy_from_slice(app_id);
        Some(5 + app_id.len())
    }

    /// Decodes a GET_PAYLOAD request body → the app id bytes.
    pub fn decode_get_payload(frame: &[u8]) -> Option<&[u8]> {
        if decode_request_op(frame)? != OP_GET_PAYLOAD || frame.len() < 6 {
            return None;
        }
        let n = frame[4] as usize;
        if n == 0 || n > 48 || frame.len() != 5 + n {
            return None;
        }
        Some(&frame[5..])
    }

    /// Encodes the 16-byte payload-VMO header (`magic, status, len:u32le`).
    pub fn encode_payload_header(status: u8, len: u32) -> [u8; PAYLOAD_DATA_OFFSET] {
        let mut hdr = [0u8; PAYLOAD_DATA_OFFSET];
        hdr[..4].copy_from_slice(&PAYLOAD_MAGIC);
        hdr[4] = status;
        hdr[8..12].copy_from_slice(&len.to_le_bytes());
        hdr
    }

    /// Decodes a payload-VMO header → `(status, len)`; `None` while the
    /// header has not been written yet (or is not a payload header).
    pub fn decode_payload_header(hdr: &[u8]) -> Option<(u8, u32)> {
        if hdr.len() < PAYLOAD_DATA_OFFSET || hdr[..4] != PAYLOAD_MAGIC {
            return None;
        }
        Some((hdr[4], u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]])))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn get_payload_round_trip() {
            let mut buf = [0u8; 64];
            let n = encode_get_payload(b"counter", &mut buf).unwrap();
            assert_eq!(&buf[..n], &[b'B', b'N', 1, OP_GET_PAYLOAD, 7, b'c', b'o', b'u', b'n', b't', b'e', b'r']);
            assert_eq!(decode_get_payload(&buf[..n]).unwrap(), b"counter");
            // Truncated / empty ids rejected.
            assert!(decode_get_payload(&buf[..n - 1]).is_none());
            assert!(encode_get_payload(b"", &mut buf).is_none());
        }

        #[test]
        fn payload_header_round_trip() {
            let hdr = encode_payload_header(PAYLOAD_STATUS_OK, 4096);
            assert_eq!(decode_payload_header(&hdr), Some((PAYLOAD_STATUS_OK, 4096)));
            // An unwritten (zeroed) header decodes to None — the poll contract.
            assert_eq!(decode_payload_header(&[0u8; PAYLOAD_DATA_OFFSET]), None);
        }

        #[test]
        fn list_req_golden() {
            let mut req = [0u8; 4];
            encode_list(&mut req);
            const GOLDEN: [u8; 4] = [b'B', b'N', 1, 1];
            assert_eq!(req, GOLDEN);
            assert_eq!(decode_request_op(&req).unwrap(), OP_LIST);
        }

        #[test]
        fn fetch_image_req_golden() {
            let mut req = [0u8; 4];
            encode_fetch_image(&mut req);
            const GOLDEN: [u8; 4] = [b'B', b'N', 1, 3];
            assert_eq!(req, GOLDEN);
            assert_eq!(decode_request_op(&req).unwrap(), OP_FETCH_IMAGE);
        }

        #[test]
        fn list_apps_req_and_header_golden() {
            let mut req = [0u8; 4];
            encode_list_apps(&mut req);
            assert_eq!(req, [b'B', b'N', 1, OP_LIST_APPS]);
            assert_eq!(decode_request_op(&req).unwrap(), OP_LIST_APPS);

            // A response header for 2 apps decodes to (OK, 2).
            let rsp = [b'B', b'N', 1, OP_LIST_APPS | 0x80, STATUS_OK, 2, 0];
            assert_eq!(decode_list_apps_header(&rsp), Some((STATUS_OK, 2)));
            // Wrong opcode rejected.
            let bad = [b'B', b'N', 1, OP_LIST | 0x80, STATUS_OK, 2, 0];
            assert_eq!(decode_list_apps_header(&bad), None);
        }

        #[test]
        fn set_active_slot_req_golden() {
            let mut req = [0u8; 5];
            encode_set_active_slot_req(1, &mut req);
            const GOLDEN: [u8; 5] = [b'B', b'N', 1, 4, 1];
            assert_eq!(req, GOLDEN);
            assert_eq!(decode_request_op(&req).unwrap(), OP_SET_ACTIVE_SLOT);
        }
    }
}

/// Session-authority wire protocol (TASK-0065B). `sessiond` is the single
/// authority for session state (greeter → active; locked designed-in but
/// reserved) and the user registry; clients (windowd greeter, abilitymgr
/// launch gate) query it request/reply — there is no push channel.
pub mod sessiond {
    /// First magic byte (`'S'`).
    pub const MAGIC0: u8 = b'S';
    /// Second magic byte (`'N'`).
    pub const MAGIC1: u8 = b'N';
    /// Protocol version.
    pub const VERSION: u8 = 1;

    /// Query session state + the user registry.
    pub const OP_GET_STATE: u8 = 1;
    /// Log a user in (greeter → active). Auth docks behind this op later.
    pub const OP_LOGIN: u8 = 2;
    /// Lock the active session. Reserved: answers `STATUS_UNSUPPORTED` today.
    pub const OP_LOCK: u8 = 3;

    /// Operation succeeded.
    pub const STATUS_OK: u8 = 0;
    /// Request frame was malformed.
    pub const STATUS_MALFORMED: u8 = 1;
    /// Operation is not supported (yet).
    pub const STATUS_UNSUPPORTED: u8 = 2;
    /// LOGIN named a user id that is not in the registry.
    pub const STATUS_UNKNOWN_USER: u8 = 3;
    /// Operation is invalid in the current session state.
    pub const STATUS_WRONG_STATE: u8 = 4;

    /// Wire value for the greeter state (no session yet).
    pub const STATE_GREETER: u8 = 0;
    /// Wire value for an active session.
    pub const STATE_ACTIVE: u8 = 1;
    /// Wire value for a locked session (reserved).
    pub const STATE_LOCKED: u8 = 2;
    /// `active_idx` value when no user is active.
    pub const NO_ACTIVE_USER: u8 = 0xFF;

    /// Byte offset where GET_STATE response user entries begin
    /// (after status + state + active_idx + count).
    pub const GET_STATE_BODY_OFFSET: usize = 8;

    /// Encodes a GET_STATE request: `[S, N, ver, OP_GET_STATE]`.
    pub fn encode_get_state(out: &mut [u8; 4]) {
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_GET_STATE;
    }

    /// Decodes the request opcode from a sessiond v1 request frame.
    pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
        if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        Some(frame[3])
    }

    /// Decodes the GET_STATE response header → `(status, state, active_idx, count)`.
    ///
    /// Response frame:
    /// `[S, N, ver, OP_GET_STATE|0x80, status:u8, state:u8, active_idx:u8, count:u8, entries...]`
    /// where each entry is
    /// `[id_len:u8, id..., name_len:u8, name..., product_len:u8, product...]` (UTF-8).
    /// Entry parsing (which needs allocation) lives in the consumer.
    pub fn decode_get_state_header(frame: &[u8]) -> Option<(u8, u8, u8, u8)> {
        if frame.len() < GET_STATE_BODY_OFFSET
            || frame[0] != MAGIC0
            || frame[1] != MAGIC1
            || frame[2] != VERSION
        {
            return None;
        }
        if frame[3] != (OP_GET_STATE | 0x80) {
            return None;
        }
        Some((frame[4], frame[5], frame[6], frame[7]))
    }

    /// Encodes a LOGIN request into `out`: `[S, N, ver, OP_LOGIN, id_len:u8, id...]`.
    /// Returns the frame length, or `None` when `out` is too small or the id
    /// is empty/oversized.
    pub fn encode_login_req(user_id: &[u8], out: &mut [u8]) -> Option<usize> {
        if user_id.is_empty() || user_id.len() > u8::MAX as usize {
            return None;
        }
        let len = 5 + user_id.len();
        if out.len() < len {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_LOGIN;
        out[4] = user_id.len() as u8;
        out[5..len].copy_from_slice(user_id);
        Some(len)
    }

    /// Decodes the user id from a LOGIN request frame body.
    pub fn decode_login_req(frame: &[u8]) -> Option<&[u8]> {
        if decode_request_op(frame) != Some(OP_LOGIN) || frame.len() < 5 {
            return None;
        }
        let id_len = frame[4] as usize;
        if id_len == 0 || frame.len() != 5 + id_len {
            return None;
        }
        Some(&frame[5..])
    }

    /// Decodes a LOGIN response → `(status, product_id_bytes)`.
    ///
    /// Response frame: `[S, N, ver, OP_LOGIN|0x80, status:u8, product_len:u8, product...]`.
    pub fn decode_login_rsp(frame: &[u8]) -> Option<(u8, &[u8])> {
        if frame.len() < 6 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_LOGIN | 0x80) {
            return None;
        }
        let product_len = frame[5] as usize;
        if frame.len() != 6 + product_len {
            return None;
        }
        Some((frame[4], &frame[6..]))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn get_state_req_golden() {
            let mut req = [0u8; 4];
            encode_get_state(&mut req);
            assert_eq!(req, [b'S', b'N', 1, OP_GET_STATE]);
            assert_eq!(decode_request_op(&req).unwrap(), OP_GET_STATE);
        }

        #[test]
        fn get_state_rsp_header_roundtrip() {
            let rsp = [
                b'S',
                b'N',
                1,
                OP_GET_STATE | 0x80,
                STATUS_OK,
                STATE_GREETER,
                NO_ACTIVE_USER,
                1,
            ];
            assert_eq!(
                decode_get_state_header(&rsp),
                Some((STATUS_OK, STATE_GREETER, NO_ACTIVE_USER, 1))
            );
            // Wrong opcode rejected.
            let bad = [b'S', b'N', 1, OP_LOGIN | 0x80, STATUS_OK, 0, 0, 0];
            assert_eq!(decode_get_state_header(&bad), None);
        }

        #[test]
        fn login_roundtrip() {
            let mut req = [0u8; 64];
            let len = encode_login_req(b"jenning", &mut req).unwrap();
            assert_eq!(&req[..len], &[b'S', b'N', 1, OP_LOGIN, 7, b'j', b'e', b'n', b'n', b'i', b'n', b'g']);
            assert_eq!(decode_login_req(&req[..len]).unwrap(), b"jenning");

            let rsp = [b'S', b'N', 1, OP_LOGIN | 0x80, STATUS_OK, 7, b'd', b'e', b'f', b'a', b'u', b'l', b't'];
            let (status, product) = decode_login_rsp(&rsp).unwrap();
            assert_eq!(status, STATUS_OK);
            assert_eq!(product, b"default");
        }

        #[test]
        fn malformed_rejected() {
            // Empty id refused at encode time.
            let mut out = [0u8; 8];
            assert_eq!(encode_login_req(b"", &mut out), None);
            // Truncated login request rejected.
            assert_eq!(decode_login_req(&[b'S', b'N', 1, OP_LOGIN, 3, b'a']), None);
            // Wrong magic rejected.
            assert_eq!(decode_request_op(&[b'X', b'N', 1, OP_GET_STATE]), None);
            // Length mismatch in login rsp rejected.
            assert_eq!(decode_login_rsp(&[b'S', b'N', 1, OP_LOGIN | 0x80, 0, 9, b'x']), None);
        }
    }
}

/// settingsd v1 wire protocol (TASK-0072 Phase 8): a TYPED settings registry.
/// Keys are dotted namespaces (`ui.theme.mode`), values carry a type tag, and
/// every mutation flows through settingsd (validation + apply hook +
/// persistence via statefsd) — no client writes prefs directly.
pub mod settingsd {
    /// First magic byte (`'S'`).
    pub const MAGIC0: u8 = b'S';
    /// Second magic byte (`'T'`).
    pub const MAGIC1: u8 = b'T';
    /// Protocol version.
    pub const VERSION: u8 = 1;

    /// Read one key's typed value.
    pub const OP_GET: u8 = 1;
    /// Write one key's typed value (validated, applied, persisted).
    pub const OP_SET: u8 = 2;

    /// Operation succeeded.
    pub const STATUS_OK: u8 = 0;
    /// Request frame was malformed.
    pub const STATUS_MALFORMED: u8 = 1;
    /// The key is not in the registry.
    pub const STATUS_UNKNOWN_KEY: u8 = 2;
    /// The value's type/content failed the key's validation.
    pub const STATUS_INVALID_VALUE: u8 = 3;
    /// Persisting to statefsd failed (the in-memory value did NOT change —
    /// set is atomic: validate → persist → apply).
    pub const STATUS_PERSIST_FAIL: u8 = 4;

    /// Value type tags. v1 carries every value as UTF-8 TEXT with a tag that
    /// names the key's semantic type — enums (`dark`/`light`) and identifiers
    /// stay human-readable on the wire and in the persisted journal.
    pub const TYPE_TEXT: u8 = 0;

    /// Seed keys (TASK-0225 vocabulary). The registry defines defaults +
    /// validation server-side; clients only name keys.
    pub const KEY_UI_THEME_MODE: &str = "ui.theme.mode";
    /// Accent-palette pick (`"default"` or a `nexus-theme-tokens`
    /// `ACCENT_PALETTE` name — violet/pink/red/orange/green); windowd
    /// applies (packed into the theme push) + persists it.
    pub const KEY_UI_THEME_ACCENT: &str = "ui.theme.accent";
    /// Shell windowing mode (`tablet`/`desktop`) — the Control-Center
    /// Desktop/Tablet toggle; windowd applies + persists it.
    pub const KEY_UI_SHELL_MODE: &str = "ui.shell.mode";
    /// UI font family (read-only default today; live switching is a follow-up).
    pub const KEY_UI_FONT_FAMILY: &str = "ui.font.family";
    /// Prepared (registered, no consumer yet): locale + MIME defaults.
    pub const KEY_UI_LOCALE: &str = "ui.locale";

    /// Encodes a GET request: `[S, T, ver, OP_GET, key_len:u8, key...]`.
    /// Returns the frame length; `None` when the key is empty/oversized or
    /// `out` is too small.
    pub fn encode_get_req(key: &str, out: &mut [u8]) -> Option<usize> {
        let k = key.as_bytes();
        if k.is_empty() || k.len() > u8::MAX as usize {
            return None;
        }
        let len = 5 + k.len();
        if out.len() < len {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_GET;
        out[4] = k.len() as u8;
        out[5..len].copy_from_slice(k);
        Some(len)
    }

    /// Encodes a SET request:
    /// `[S, T, ver, OP_SET, key_len:u8, key..., type:u8, val_len:u8, val...]`.
    pub fn encode_set_req(key: &str, value: &str, out: &mut [u8]) -> Option<usize> {
        let (k, v) = (key.as_bytes(), value.as_bytes());
        if k.is_empty() || k.len() > u8::MAX as usize || v.len() > u8::MAX as usize {
            return None;
        }
        let len = 5 + k.len() + 2 + v.len();
        if out.len() < len {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_SET;
        out[4] = k.len() as u8;
        out[5..5 + k.len()].copy_from_slice(k);
        out[5 + k.len()] = TYPE_TEXT;
        out[6 + k.len()] = v.len() as u8;
        out[7 + k.len()..len].copy_from_slice(v);
        Some(len)
    }

    /// Decodes a request → `(op, key, value)`; `value` is empty for GET.
    /// Returns `None` on any malformed frame (bad magic/version/lengths).
    pub fn decode_request(frame: &[u8]) -> Option<(u8, &str, &str)> {
        if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        let op = frame[3];
        let klen = frame[4] as usize;
        let key_end = 5 + klen;
        if klen == 0 || frame.len() < key_end {
            return None;
        }
        let key = core::str::from_utf8(&frame[5..key_end]).ok()?;
        match op {
            OP_GET => (frame.len() == key_end).then_some((op, key, "")),
            OP_SET => {
                if frame.len() < key_end + 2 || frame[key_end] != TYPE_TEXT {
                    return None;
                }
                let vlen = frame[key_end + 1] as usize;
                let val_end = key_end + 2 + vlen;
                if frame.len() != val_end {
                    return None;
                }
                let value = core::str::from_utf8(&frame[key_end + 2..val_end]).ok()?;
                Some((op, key, value))
            }
            _ => None,
        }
    }

    /// Encodes a response: `[S, T, ver, op|0x80, status:u8, type:u8, val_len:u8, val...]`
    /// (`val` is the key's current value for OK GET/SET; empty otherwise).
    pub fn encode_response(op: u8, status: u8, value: &str, out: &mut [u8]) -> Option<usize> {
        let v = value.as_bytes();
        if v.len() > u8::MAX as usize {
            return None;
        }
        let len = 7 + v.len();
        if out.len() < len {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = op | 0x80;
        out[4] = status;
        out[5] = TYPE_TEXT;
        out[6] = v.len() as u8;
        out[7..len].copy_from_slice(v);
        Some(len)
    }

    /// Decodes a response → `(status, value)`.
    pub fn decode_response(op: u8, frame: &[u8]) -> Option<(u8, &str)> {
        if frame.len() < 7
            || frame[0] != MAGIC0
            || frame[1] != MAGIC1
            || frame[2] != VERSION
            || frame[3] != (op | 0x80)
            || frame[5] != TYPE_TEXT
        {
            return None;
        }
        let vlen = frame[6] as usize;
        if frame.len() != 7 + vlen {
            return None;
        }
        let value = core::str::from_utf8(&frame[7..]).ok()?;
        Some((frame[4], value))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Golden byte layouts: the wire format is a cross-service contract —
        // a layout drift breaks windowd/statefsd interop silently.
        #[test]
        fn get_request_golden_bytes() {
            let mut buf = [0u8; 32];
            let n = encode_get_req("ui.theme.mode", &mut buf).unwrap();
            assert_eq!(&buf[..5], &[b'S', b'T', 1, OP_GET, 13]);
            assert_eq!(&buf[5..n], b"ui.theme.mode");
            let (op, key, value) = decode_request(&buf[..n]).unwrap();
            assert_eq!((op, key, value), (OP_GET, "ui.theme.mode", ""));
        }

        #[test]
        fn set_request_golden_bytes_and_roundtrip() {
            let mut buf = [0u8; 64];
            let n = encode_set_req(KEY_UI_THEME_MODE, "light", &mut buf).unwrap();
            assert_eq!(&buf[..5], &[b'S', b'T', 1, OP_SET, 13]);
            assert_eq!(buf[5 + 13], TYPE_TEXT);
            assert_eq!(buf[6 + 13], 5);
            let (op, key, value) = decode_request(&buf[..n]).unwrap();
            assert_eq!((op, key, value), (OP_SET, "ui.theme.mode", "light"));
        }

        #[test]
        fn response_roundtrip_and_rejects() {
            let mut buf = [0u8; 32];
            let n = encode_response(OP_GET, STATUS_OK, "dark", &mut buf).unwrap();
            assert_eq!(decode_response(OP_GET, &buf[..n]), Some((STATUS_OK, "dark")));
            // Wrong op bit / truncation / bad magic all reject.
            assert_eq!(decode_response(OP_SET, &buf[..n]), None);
            assert_eq!(decode_response(OP_GET, &buf[..n - 1]), None);
            let mut bad = buf;
            bad[0] = b'X';
            assert_eq!(decode_response(OP_GET, &bad[..n]), None);
        }

        #[test]
        fn malformed_requests_reject() {
            // Truncated key, empty key, unknown op, trailing garbage on GET.
            assert_eq!(decode_request(&[b'S', b'T', 1, OP_GET, 5, b'a']), None);
            assert_eq!(decode_request(&[b'S', b'T', 1, OP_GET, 0]), None);
            assert_eq!(decode_request(&[b'S', b'T', 1, 99, 1, b'a']), None);
            let mut buf = [0u8; 32];
            let n = encode_get_req("a", &mut buf).unwrap();
            assert_eq!(decode_request(&buf[..n + 1]), None);
        }
    }
}

/// Read-only bundle image format used in OS bring-up (served by bundlemgrd, consumed by packagefsd).
pub mod bundleimg {
    /// Image magic `NXBI` ("NeXuS Bundle Image").
    const MAGIC: [u8; 4] = *b"NXBI";
    /// Image format version.
    pub const VERSION: u8 = 1;

    /// Entry kind: file.
    pub const KIND_FILE: u16 = 0;

    /// Parsed entry view.
    pub struct Entry<'a> {
        /// Bundle name bytes (UTF-8).
        pub bundle: &'a [u8],
        /// Bundle version bytes (UTF-8).
        pub version: &'a [u8],
        /// Entry path bytes (UTF-8, relative inside the bundle).
        pub path: &'a [u8],
        /// Entry kind (e.g. [`KIND_FILE`]).
        pub kind: u16,
        /// Entry payload bytes (for files).
        pub data: &'a [u8],
    }

    /// Parses the header and returns (entry_count, first_entry_offset).
    pub fn decode_header(frame: &[u8]) -> Option<(u16, usize)> {
        if frame.len() < 4 + 1 + 2 || frame[..4] != MAGIC {
            return None;
        }
        if frame[4] != VERSION {
            return None;
        }
        let count = u16::from_le_bytes([frame[5], frame[6]]);
        Some((count, 7))
    }

    /// Parses the next entry starting at `*off` and advances `off` on success.
    pub fn decode_next<'a>(frame: &'a [u8], off: &mut usize) -> Option<Entry<'a>> {
        let mut i = *off;
        if i >= frame.len() {
            return None;
        }
        let bundle_len = *frame.get(i)? as usize;
        i += 1;
        let bundle = frame.get(i..i + bundle_len)?;
        i += bundle_len;
        let ver_len = *frame.get(i)? as usize;
        i += 1;
        let version = frame.get(i..i + ver_len)?;
        i += ver_len;
        let path_len = u16::from_le_bytes([*frame.get(i)?, *frame.get(i + 1)?]) as usize;
        i += 2;
        let path = frame.get(i..i + path_len)?;
        i += path_len;
        let kind = u16::from_le_bytes([*frame.get(i)?, *frame.get(i + 1)?]);
        i += 2;
        let data_len = u32::from_le_bytes([
            *frame.get(i)?,
            *frame.get(i + 1)?,
            *frame.get(i + 2)?,
            *frame.get(i + 3)?,
        ]) as usize;
        i += 4;
        let data = frame.get(i..i + data_len)?;
        i += data_len;
        *off = i;
        Some(Entry { bundle, version, path, kind, data })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Golden image used by bring-up (mirrors bundlemgrd os-lite image):
        // NXBI v1 with one entry: system@1.0.0/build.prop => "ro.nexus.build=dev\n"
        const GOLDEN_IMG: &[u8] = &[
            b'N', b'X', b'B', b'I', 1, 1, 0, // magic + version + count=1
            6, b's', b'y', b's', b't', b'e', b'm', // bundle "system"
            5, b'1', b'.', b'0', b'.', b'0', // version "1.0.0"
            10, 0, b'b', b'u', b'i', b'l', b'd', b'.', b'p', b'r', b'o',
            b'p', // path len=10 + "build.prop"
            0, 0, // kind=KIND_FILE
            19, 0, 0, 0, // data_len=19
            b'r', b'o', b'.', b'n', b'e', b'x', b'u', b's', b'.', b'b', b'u', b'i', b'l', b'd',
            b'=', b'd', b'e', b'v', b'\n',
        ];

        #[test]
        fn header_golden() {
            let (count, off) = decode_header(GOLDEN_IMG).unwrap();
            assert_eq!(count, 1);
            assert_eq!(off, 7);
        }

        #[test]
        fn entry_golden() {
            let (_count, mut off) = decode_header(GOLDEN_IMG).unwrap();
            let e = decode_next(GOLDEN_IMG, &mut off).unwrap();
            assert_eq!(e.bundle, b"system");
            assert_eq!(e.version, b"1.0.0");
            assert_eq!(e.path, b"build.prop");
            assert_eq!(e.kind, KIND_FILE);
            assert_eq!(e.data, b"ro.nexus.build=dev\n");
            assert_eq!(off, GOLDEN_IMG.len());
        }
    }
}

/// Policy control frames (bring-up) shared between init-lite, policyd, and privileged services.
pub mod policy {
    /// Magic bytes (`'P','C'`) for init-lite control-plane policy queries.
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'C';
    /// Policy control protocol version.
    pub const VERSION: u8 = 1;

    /// Exec authorization request opcode (service -> init-lite).
    pub const OP_EXEC_CHECK: u8 = 1;

    /// Status: operation allowed.
    pub const STATUS_ALLOW: u8 = 0;
    /// Status: operation denied.
    pub const STATUS_DENY: u8 = 1;
    /// Status: malformed request.
    pub const STATUS_MALFORMED: u8 = 2;

    /// Nonce used to correlate requests and responses.
    pub type Nonce = u32;

    /// Encodes an exec-check request into `out`.
    ///
    /// Frame: [P, C, ver, OP_EXEC_CHECK, nonce:u32le, requester_len:u8, requester..., image_id:u8]
    pub fn encode_exec_check(
        nonce: Nonce,
        requester: &[u8],
        image_id: u8,
        out: &mut [u8],
    ) -> Option<usize> {
        if requester.is_empty() || requester.len() > 48 || out.len() < 10 + requester.len() {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_EXEC_CHECK;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = requester.len() as u8;
        out[9..9 + requester.len()].copy_from_slice(requester);
        out[9 + requester.len()] = image_id;
        Some(10 + requester.len())
    }

    /// Decodes an exec-check request.
    pub fn decode_exec_check(frame: &[u8]) -> Option<(Nonce, &[u8], u8)> {
        if frame.len() < 10 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != OP_EXEC_CHECK {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let n = frame[8] as usize;
        if n == 0 || n > 48 || frame.len() != 10 + n {
            return None;
        }
        let requester = &frame[9..9 + n];
        let image_id = frame[9 + n];
        Some((nonce, requester, image_id))
    }

    /// Encodes an exec-check response.
    ///
    /// Frame: [P, C, ver, OP_EXEC_CHECK|0x80, nonce:u32le, status:u8]
    pub fn encode_exec_check_rsp(nonce: Nonce, status: u8) -> [u8; 9] {
        let mut out = [0u8; 9];
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION;
        out[3] = OP_EXEC_CHECK | 0x80;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = status;
        out
    }

    /// Decodes an exec-check response status.
    pub fn decode_exec_check_rsp(frame: &[u8]) -> Option<(Nonce, u8)> {
        if frame.len() != 9 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return None;
        }
        if frame[3] != (OP_EXEC_CHECK | 0x80) {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        Some((nonce, frame[8]))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn exec_check_roundtrip() {
            let mut buf = [0u8; 64];
            let n = encode_exec_check(0x11223344, b"selftest-client", 2, &mut buf).unwrap();
            let (nonce, requester, image) = decode_exec_check(&buf[..n]).unwrap();
            assert_eq!(nonce, 0x11223344);
            assert_eq!(requester, b"selftest-client");
            assert_eq!(image, 2);
        }

        #[test]
        fn exec_check_rsp_roundtrip() {
            let frame = encode_exec_check_rsp(0xAABBCCDD, STATUS_DENY);
            let (nonce, status) = decode_exec_check_rsp(&frame).unwrap();
            assert_eq!(nonce, 0xAABBCCDD);
            assert_eq!(status, STATUS_DENY);
        }
    }
}

/// Policyd service frames (v1/v2) shared between init-lite, policyd, and clients.
pub mod policyd {
    /// Frame magic bytes (`'P','O'`) for the policyd IPC protocol.
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';

    /// Policyd protocol version 1 (legacy bring-up, no correlation).
    pub const VERSION_V1: u8 = 1;
    /// Policyd protocol version 2 (nonce-correlated requests/responses).
    pub const VERSION_V2: u8 = 2;
    /// Policyd protocol version 3 (nonce-correlated, ID-based requester/target).
    pub const VERSION_V3: u8 = 3;

    /// Policy check opcode (bring-up).
    pub const OP_CHECK: u8 = 1;
    /// Route authorization check opcode.
    pub const OP_ROUTE: u8 = 2;
    /// Exec authorization check opcode.
    pub const OP_EXEC: u8 = 3;
    /// Capability check opcode (bring-up, service-id bound).
    pub const OP_CHECK_CAP: u8 = 4;
    /// ABI syscall profile fetch opcode (nonce-correlated, v2).
    pub const OP_ABI_PROFILE_GET: u8 = 6;

    /// Status: allowed.
    pub const STATUS_ALLOW: u8 = 0;
    /// Status: denied.
    pub const STATUS_DENY: u8 = 1;
    /// Status: malformed.
    pub const STATUS_MALFORMED: u8 = 2;
    /// Status: unsupported op/version.
    pub const STATUS_UNSUPPORTED: u8 = 3;

    /// Nonce used to correlate requests and responses (v2).
    pub type Nonce = u32;

    /// Encodes a v2 ROUTE request:
    /// [P,O,ver=2,OP_ROUTE, nonce:u32le, req_len:u8, req..., tgt_len:u8, tgt...]
    pub fn encode_route_v2(
        nonce: Nonce,
        requester: &[u8],
        target: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        if requester.is_empty()
            || requester.len() > 48
            || target.is_empty()
            || target.len() > 48
            || out.len() < 10 + requester.len() + target.len()
        {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V2;
        out[3] = OP_ROUTE;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = requester.len() as u8;
        out[9..9 + requester.len()].copy_from_slice(requester);
        let mut n = 9 + requester.len();
        out[n] = target.len() as u8;
        n += 1;
        out[n..n + target.len()].copy_from_slice(target);
        n += target.len();
        Some(n)
    }

    /// Decodes a v2 ROUTE request.
    pub fn decode_route_v2(frame: &[u8]) -> Option<(Nonce, &[u8], &[u8])> {
        if frame.len() < 10 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V2 {
            return None;
        }
        if frame[3] != OP_ROUTE {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let req_len = frame[8] as usize;
        if req_len == 0 || req_len > 48 || frame.len() < 10 + req_len {
            return None;
        }
        let req_start = 9;
        let req_end = req_start + req_len;
        let tgt_len = *frame.get(req_end)? as usize;
        if tgt_len == 0 || tgt_len > 48 {
            return None;
        }
        let tgt_start = req_end + 1;
        let tgt_end = tgt_start + tgt_len;
        if frame.len() != tgt_end {
            return None;
        }
        Some((nonce, &frame[req_start..req_end], &frame[tgt_start..tgt_end]))
    }

    /// Encodes a v2 EXEC request:
    /// [P,O,ver=2,OP_EXEC, nonce:u32le, req_len:u8, req..., image_id:u8]
    pub fn encode_exec_v2(
        nonce: Nonce,
        requester: &[u8],
        image_id: u8,
        out: &mut [u8],
    ) -> Option<usize> {
        if requester.is_empty() || requester.len() > 48 || out.len() < 10 + requester.len() {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V2;
        out[3] = OP_EXEC;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = requester.len() as u8;
        out[9..9 + requester.len()].copy_from_slice(requester);
        out[9 + requester.len()] = image_id;
        Some(10 + requester.len())
    }

    /// Decodes a v2 EXEC request.
    pub fn decode_exec_v2(frame: &[u8]) -> Option<(Nonce, &[u8], u8)> {
        if frame.len() < 10 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V2 {
            return None;
        }
        if frame[3] != OP_EXEC {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let req_len = frame[8] as usize;
        if req_len == 0 || req_len > 48 || frame.len() != 10 + req_len {
            return None;
        }
        let requester = &frame[9..9 + req_len];
        let image_id = frame[9 + req_len];
        Some((nonce, requester, image_id))
    }

    /// Encodes a v3 ROUTE request:
    /// [P,O,ver=3,OP_ROUTE, nonce:u32le, requester_id:u64le, target_id:u64le]
    pub fn encode_route_v3_id(
        nonce: Nonce,
        requester_id: u64,
        target_id: u64,
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() < 24 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V3;
        out[3] = OP_ROUTE;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8..16].copy_from_slice(&requester_id.to_le_bytes());
        out[16..24].copy_from_slice(&target_id.to_le_bytes());
        Some(24)
    }

    /// Decodes a v3 ROUTE request.
    pub fn decode_route_v3_id(frame: &[u8]) -> Option<(Nonce, u64, u64)> {
        if frame.len() != 24 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V3 {
            return None;
        }
        if frame[3] != OP_ROUTE {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let requester_id = u64::from_le_bytes([
            frame[8], frame[9], frame[10], frame[11], frame[12], frame[13], frame[14], frame[15],
        ]);
        let target_id = u64::from_le_bytes([
            frame[16], frame[17], frame[18], frame[19], frame[20], frame[21], frame[22], frame[23],
        ]);
        Some((nonce, requester_id, target_id))
    }

    /// Encodes a v3 EXEC request:
    /// [P,O,ver=3,OP_EXEC, nonce:u32le, requester_id:u64le, image_id:u8]
    pub fn encode_exec_v3_id(
        nonce: Nonce,
        requester_id: u64,
        image_id: u8,
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() < 17 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V3;
        out[3] = OP_EXEC;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8..16].copy_from_slice(&requester_id.to_le_bytes());
        out[16] = image_id;
        Some(17)
    }

    /// Decodes a v3 EXEC request.
    pub fn decode_exec_v3_id(frame: &[u8]) -> Option<(Nonce, u64, u8)> {
        if frame.len() != 17 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V3 {
            return None;
        }
        if frame[3] != OP_EXEC {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let requester_id = u64::from_le_bytes([
            frame[8], frame[9], frame[10], frame[11], frame[12], frame[13], frame[14], frame[15],
        ]);
        let image_id = frame[16];
        Some((nonce, requester_id, image_id))
    }

    /// Encodes a v2 ABI profile fetch request:
    /// [P,O,ver=2,OP_ABI_PROFILE_GET, nonce:u32le, subject_id:u64le]
    #[must_use = "encoded request length must be checked before send"]
    pub fn encode_abi_profile_get_v2(
        nonce: Nonce,
        subject_id: u64,
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() < 16 {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V2;
        out[3] = OP_ABI_PROFILE_GET;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8..16].copy_from_slice(&subject_id.to_le_bytes());
        Some(16)
    }

    /// Decodes a v2 ABI profile fetch request.
    #[must_use = "decoded request must be validated before policy handling"]
    pub fn decode_abi_profile_get_v2(frame: &[u8]) -> Option<(Nonce, u64)> {
        if frame.len() != 16 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V2 {
            return None;
        }
        if frame[3] != OP_ABI_PROFILE_GET {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let subject_id = u64::from_le_bytes([
            frame[8], frame[9], frame[10], frame[11], frame[12], frame[13], frame[14], frame[15],
        ]);
        Some((nonce, subject_id))
    }

    /// Encodes a v2 ABI profile fetch response:
    /// [P,O,ver=2,OP_ABI_PROFILE_GET|0x80,nonce:u32le,status:u8,_reserved:u8,profile_len:u16le,profile...]
    #[must_use = "encoded response length must be checked before reply send"]
    pub fn encode_abi_profile_rsp_v2(
        nonce: Nonce,
        status: u8,
        profile: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        if profile.len() > crate::abi_filter::MAX_PROFILE_BYTES || profile.len() > u16::MAX as usize
        {
            return None;
        }
        let total = 12 + profile.len();
        if out.len() < total {
            return None;
        }
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V2;
        out[3] = OP_ABI_PROFILE_GET | 0x80;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = status;
        out[9] = 0;
        out[10..12].copy_from_slice(&(profile.len() as u16).to_le_bytes());
        out[12..12 + profile.len()].copy_from_slice(profile);
        Some(total)
    }

    /// Decodes a v2 ABI profile fetch response and returns (nonce, status, profile_bytes).
    #[must_use = "decoded response status/profile must be consumed by caller"]
    pub fn decode_abi_profile_rsp_v2(frame: &[u8]) -> Option<(Nonce, u8, &[u8])> {
        if frame.len() < 12 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V2 {
            return None;
        }
        if frame[3] != (OP_ABI_PROFILE_GET | 0x80) {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let status = frame[8];
        let profile_len = u16::from_le_bytes([frame[10], frame[11]]) as usize;
        if profile_len > crate::abi_filter::MAX_PROFILE_BYTES || frame.len() != 12 + profile_len {
            return None;
        }
        Some((nonce, status, &frame[12..]))
    }

    /// Encodes a v2 response:
    /// [P,O,ver=2,op|0x80, nonce:u32le, status:u8, _reserved:u8]
    pub fn encode_rsp_v2(op: u8, nonce: Nonce, status: u8) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V2;
        out[3] = op | 0x80;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = status;
        out[9] = 0;
        out
    }

    /// Encodes a v3 response:
    /// [P,O,ver=3,op|0x80, nonce:u32le, status:u8, _reserved:u8]
    pub fn encode_rsp_v3(op: u8, nonce: Nonce, status: u8) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0] = MAGIC0;
        out[1] = MAGIC1;
        out[2] = VERSION_V3;
        out[3] = op | 0x80;
        out[4..8].copy_from_slice(&nonce.to_le_bytes());
        out[8] = status;
        out[9] = 0;
        out
    }

    /// Decodes a v2/v3 response and returns (ver, op, nonce, status).
    pub fn decode_rsp_v2_or_v3(frame: &[u8]) -> Option<(u8, u8, Nonce, u8)> {
        if frame.len() != 10 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
            return None;
        }
        let ver = frame[2];
        if ver != VERSION_V2 && ver != VERSION_V3 {
            return None;
        }
        if (frame[3] & 0x80) == 0 {
            return None;
        }
        let op = frame[3] & !0x80;
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        Some((ver, op, nonce, frame[8]))
    }

    /// Decodes a v2 response and returns (op, nonce, status).
    pub fn decode_rsp_v2(frame: &[u8]) -> Option<(u8, Nonce, u8)> {
        if frame.len() != 10 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION_V2 {
            return None;
        }
        let op = frame[3] & !0x80;
        if (frame[3] & 0x80) == 0 {
            return None;
        }
        let nonce = Nonce::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let status = frame[8];
        Some((op, nonce, status))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn route_v3_id_golden() {
            let mut buf = [0u8; 32];
            let n = encode_route_v3_id(
                0x11223344,
                0x0102_0304_0506_0708,
                0xA0A1_A2A3_A4A5_A6A7,
                &mut buf,
            )
            .unwrap();
            const GOLDEN: [u8; 24] = [
                b'P', b'O', 3, 2, // magic + ver + OP_ROUTE
                0x44, 0x33, 0x22, 0x11, // nonce LE
                0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, // requester_id LE
                0xA7, 0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1, 0xA0, // target_id LE
            ];
            assert_eq!(&buf[..n], &GOLDEN);
            let (nonce, req, tgt) = decode_route_v3_id(&buf[..n]).unwrap();
            assert_eq!(nonce, 0x11223344);
            assert_eq!(req, 0x0102_0304_0506_0708);
            assert_eq!(tgt, 0xA0A1_A2A3_A4A5_A6A7);
        }

        #[test]
        fn exec_v3_id_golden() {
            let mut buf = [0u8; 32];
            let n = encode_exec_v3_id(0x01020304, 0x1122_3344_5566_7788, 9, &mut buf).unwrap();
            const GOLDEN: [u8; 17] = [
                b'P', b'O', 3, 3, // magic + ver + OP_EXEC
                0x04, 0x03, 0x02, 0x01, // nonce LE
                0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, // requester_id LE
                9,    // image_id
            ];
            assert_eq!(&buf[..n], &GOLDEN);
            let (nonce, req, img) = decode_exec_v3_id(&buf[..n]).unwrap();
            assert_eq!(nonce, 0x01020304);
            assert_eq!(req, 0x1122_3344_5566_7788);
            assert_eq!(img, 9);
        }

        #[test]
        fn rsp_v3_golden() {
            let frame = encode_rsp_v3(OP_ROUTE, 0xAABBCCDD, STATUS_DENY);
            const GOLDEN: [u8; 10] = [
                b'P',
                b'O',
                3,
                (2 | 0x80),
                0xDD,
                0xCC,
                0xBB,
                0xAA,
                1, // STATUS_DENY
                0,
            ];
            assert_eq!(frame, GOLDEN);
            let (ver, op, nonce, status) = decode_rsp_v2_or_v3(&frame).unwrap();
            assert_eq!(ver, VERSION_V3);
            assert_eq!(op, OP_ROUTE);
            assert_eq!(nonce, 0xAABBCCDD);
            assert_eq!(status, STATUS_DENY);
        }

        #[test]
        fn route_v2_roundtrip() {
            let mut buf = [0u8; 128];
            let n = encode_route_v2(0x12345678, b"bundlemgrd", b"execd", &mut buf).unwrap();
            let (nonce, req, tgt) = decode_route_v2(&buf[..n]).unwrap();
            assert_eq!(nonce, 0x12345678);
            assert_eq!(req, b"bundlemgrd");
            assert_eq!(tgt, b"execd");
        }

        #[test]
        fn exec_v2_roundtrip() {
            let mut buf = [0u8; 128];
            let n = encode_exec_v2(0x90ABCDEF, b"selftest-client", 2, &mut buf).unwrap();
            let (nonce, req, img) = decode_exec_v2(&buf[..n]).unwrap();
            assert_eq!(nonce, 0x90ABCDEF);
            assert_eq!(req, b"selftest-client");
            assert_eq!(img, 2);
        }

        #[test]
        fn rsp_v2_roundtrip() {
            let frame = encode_rsp_v2(OP_ROUTE, 0xAABBCCDD, STATUS_DENY);
            let (op, nonce, status) = decode_rsp_v2(&frame).unwrap();
            assert_eq!(op, OP_ROUTE);
            assert_eq!(nonce, 0xAABBCCDD);
            assert_eq!(status, STATUS_DENY);
        }

        #[test]
        fn abi_profile_v2_roundtrip() {
            let mut req = [0u8; 32];
            let n =
                encode_abi_profile_get_v2(0x0102_0304, 0x1122_3344_5566_7788, &mut req).unwrap();
            let (nonce, subject_id) = decode_abi_profile_get_v2(&req[..n]).unwrap();
            assert_eq!(nonce, 0x0102_0304);
            assert_eq!(subject_id, 0x1122_3344_5566_7788);

            let profile = [1u8, 2, 3, 4];
            let mut rsp = [0u8; 64];
            let m =
                encode_abi_profile_rsp_v2(0x1122_3344, STATUS_ALLOW, &profile, &mut rsp).unwrap();
            let (rsp_nonce, status, got_profile) = decode_abi_profile_rsp_v2(&rsp[..m]).unwrap();
            assert_eq!(rsp_nonce, 0x1122_3344);
            assert_eq!(status, STATUS_ALLOW);
            assert_eq!(got_profile, &profile);
        }
    }
}

/// Computes a stable service identifier from the UTF-8 service name bytes.
///
/// This is the userspace mirror of the kernel's `BootstrapInfo.service_id` derivation.
pub fn service_id_from_name(name: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325u64;
    for &b in name {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3u64);
    }
    h
}

impl MsgHeader {
    /// Creates a new header with the provided fields.
    pub const fn new(src: u32, dst: u32, ty: u16, flags: u16, len: u32) -> Self {
        Self { src, dst, ty, flags, len }
    }

    /// Serialises the header to a little-endian byte array.
    pub fn to_le_bytes(&self) -> [u8; 16] {
        let mut buf = [0_u8; 16];
        buf[0..4].copy_from_slice(&self.src.to_le_bytes());
        buf[4..8].copy_from_slice(&self.dst.to_le_bytes());
        buf[8..10].copy_from_slice(&self.ty.to_le_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_le_bytes());
        buf[12..16].copy_from_slice(&self.len.to_le_bytes());
        buf
    }

    /// Deserialises a little-endian byte array into a header.
    pub fn from_le_bytes(bytes: [u8; 16]) -> Self {
        let src = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let dst = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let ty = u16::from_le_bytes([bytes[8], bytes[9]]);
        let flags = u16::from_le_bytes([bytes[10], bytes[11]]);
        let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        Self { src, dst, ty, flags, len }
    }
}

// ——— IPC v1 syscalls (OS build) ———

/// Syscall flags for IPC v1 operations.
#[cfg(nexus_env = "os")]
pub const IPC_SYS_NONBLOCK: u32 = 1 << 0;
/// Permit payload truncation on receive.
#[cfg(nexus_env = "os")]
pub const IPC_SYS_TRUNCATE: u32 = 1 << 1;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn decode_ipc_send(value: usize) -> Result<usize> {
    if (value as isize) < 0 {
        match -(value as isize) as usize {
            1 => Err(IpcError::PermissionDenied), // EPERM
            3 => Err(IpcError::NoSuchEndpoint),   // ESRCH
            11 => Err(IpcError::QueueFull),       // EAGAIN
            28 => Err(IpcError::NoSpace),         // ENOSPC
            110 => Err(IpcError::TimedOut),       // ETIMEDOUT
            _ => Err(IpcError::Unsupported),
        }
    } else {
        Ok(value)
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn decode_ipc_recv(value: usize) -> Result<usize> {
    if (value as isize) < 0 {
        match -(value as isize) as usize {
            1 => Err(IpcError::PermissionDenied), // EPERM
            3 => Err(IpcError::NoSuchEndpoint),   // ESRCH
            11 => Err(IpcError::QueueEmpty),      // EAGAIN
            28 => Err(IpcError::NoSpace),         // ENOSPC
            110 => Err(IpcError::TimedOut),       // ETIMEDOUT
            _ => Err(IpcError::Unsupported),
        }
    } else {
        Ok(value)
    }
}

/// Sends an IPC v1 message to the endpoint referenced by `slot` (payload copy-in).
///
/// `sys_flags` uses [`IPC_SYS_NONBLOCK`]. When `sys_flags` does not include NONBLOCK, the
/// kernel may block until the queue has capacity or the optional `deadline_ns` expires.
///
/// `deadline_ns=0` means “no deadline”.
#[cfg(nexus_env = "os")]
pub fn ipc_send_v1(
    slot: Cap,
    header: &MsgHeader,
    payload: &[u8],
    sys_flags: u32,
    deadline_ns: u64,
) -> Result<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_SEND_V1: usize = 14;
        let header_ptr = header as *const MsgHeader as usize;
        let payload_ptr = payload.as_ptr() as usize;
        let payload_len = payload.len();
        let sys_flags = sys_flags as usize;
        let deadline_ns = deadline_ns as usize;
        let raw = unsafe {
            ecall6(
                SYSCALL_IPC_SEND_V1,
                slot as usize,
                header_ptr,
                payload_ptr,
                payload_len,
                sys_flags,
                deadline_ns,
            )
        };
        decode_ipc_send(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, header, payload, sys_flags, deadline_ns);
        Err(IpcError::Unsupported)
    }
}

/// Convenience helper: non-blocking send with no deadline.
#[cfg(nexus_env = "os")]
pub fn ipc_send_v1_nb(slot: Cap, header: &MsgHeader, payload: &[u8]) -> Result<usize> {
    ipc_send_v1(slot, header, payload, IPC_SYS_NONBLOCK, 0)
}

/// Receives an IPC v1 message from the endpoint referenced by `slot` (payload copy-out).
///
/// Returns the number of bytes written into `payload_out`.
///
/// `sys_flags` uses [`IPC_SYS_NONBLOCK`] and [`IPC_SYS_TRUNCATE`]. When `sys_flags` does not
/// include NONBLOCK, the kernel may block until a message arrives or the optional
/// `deadline_ns` expires.
///
/// `deadline_ns=0` means “no deadline”.
#[cfg(nexus_env = "os")]
pub fn ipc_recv_v1(
    slot: Cap,
    header_out: &mut MsgHeader,
    payload_out: &mut [u8],
    sys_flags: u32,
    deadline_ns: u64,
) -> Result<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_RECV_V1: usize = 18;
        let header_out_ptr = header_out as *mut MsgHeader as usize;
        let payload_out_ptr = payload_out.as_mut_ptr() as usize;
        let payload_out_max = payload_out.len();
        let sys_flags = sys_flags as usize;
        let deadline_ns = deadline_ns as usize;
        let raw = unsafe {
            ecall6(
                SYSCALL_IPC_RECV_V1,
                slot as usize,
                header_out_ptr,
                payload_out_ptr,
                payload_out_max,
                sys_flags,
                deadline_ns,
            )
        };
        decode_ipc_recv(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, header_out, payload_out, sys_flags, deadline_ns);
        Err(IpcError::Unsupported)
    }
}

/// IPC recv v2 descriptor (extensible ABI for recv-side metadata).
///
/// This struct is part of the **kernel/userspace syscall ABI** and is therefore treated as
/// layout-stable. Host builds keep the definition available so we can run layout tests without
/// needing an OS test runner.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IpcRecvV2Desc {
    /// Descriptor magic ('N''X''I''2').
    pub magic: u32,
    /// Descriptor version.
    pub version: u32,
    /// Receive endpoint capability slot.
    pub slot: u32,
    /// Reserved padding.
    pub _pad0: u32,
    /// User pointer to `MsgHeader` to be written by the kernel.
    pub header_out_ptr: u64,
    /// User pointer to payload buffer to be written by the kernel.
    pub payload_out_ptr: u64,
    /// Maximum payload bytes the kernel may write.
    pub payload_out_max: u64,
    /// User pointer to `u64` where the kernel writes `sender_service_id`.
    pub sender_service_id_out_ptr: u64,
    /// Syscall flags (NONBLOCK/TRUNCATE).
    pub sys_flags: u32,
    /// Reserved padding.
    pub _pad1: u32,
    /// Deadline in nanoseconds (`0` means no deadline).
    pub deadline_ns: u64,
}

/// `IpcRecvV2Desc` magic (`'N''X''I''2'`).
pub const IPC_RECV_V2_DESC_MAGIC: u32 = u32::from_be_bytes(*b"NXI2");
/// `IpcRecvV2Desc` version.
pub const IPC_RECV_V2_DESC_VERSION: u32 = 1;

/// Receives an IPC message and additionally returns the sender's kernel-derived service identity.
///
/// This is a descriptor-based syscall (v2) so we can extend metadata without being limited by
/// the register argument count.
#[cfg(nexus_env = "os")]
pub fn ipc_recv_v2(
    slot: Cap,
    header_out: &mut MsgHeader,
    payload_out: &mut [u8],
    sender_service_id_out: &mut u64,
    sys_flags: u32,
    deadline_ns: u64,
) -> Result<usize> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_RECV_V2: usize = 26;
        let desc = IpcRecvV2Desc {
            magic: IPC_RECV_V2_DESC_MAGIC,
            version: IPC_RECV_V2_DESC_VERSION,
            slot: slot as u32,
            _pad0: 0,
            header_out_ptr: header_out as *mut MsgHeader as u64,
            payload_out_ptr: payload_out.as_mut_ptr() as u64,
            payload_out_max: payload_out.len() as u64,
            sender_service_id_out_ptr: sender_service_id_out as *mut u64 as u64,
            sys_flags,
            _pad1: 0,
            deadline_ns,
        };
        let raw = unsafe { ecall1(SYSCALL_IPC_RECV_V2, &desc as *const _ as usize) };
        decode_ipc_recv(raw)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (slot, header_out, payload_out, sender_service_id_out, sys_flags, deadline_ns);
        Err(IpcError::Unsupported)
    }
}

/// Convenience helper: non-blocking receive (optionally truncating) with no deadline.
#[cfg(nexus_env = "os")]
pub fn ipc_recv_v1_nb(
    slot: Cap,
    header_out: &mut MsgHeader,
    payload_out: &mut [u8],
    truncate: bool,
) -> Result<usize> {
    let mut flags = IPC_SYS_NONBLOCK;
    if truncate {
        flags |= IPC_SYS_TRUNCATE;
    }
    ipc_recv_v1(slot, header_out, payload_out, flags, 0)
}

// ——— Task and capability primitives (OS build) ———

#[cfg(nexus_env = "os")]
bitflags::bitflags! {
    /// Rights mask accepted by capability-transfer syscalls.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Rights: u32 {
        /// Permit the holder to send messages through the endpoint.
        const SEND = 1 << 0;
        /// Permit the holder to receive messages from the endpoint.
        const RECV = 1 << 1;
        /// Permit the holder to map VMOs into its address space.
        const MAP = 1 << 2;
        /// Permit the holder to manage capabilities (reserved for kernel tests).
        const MANAGE = 1 << 3;
    }
}

/// Kernel task identifier returned from [`spawn`].
#[cfg(nexus_env = "os")]
pub type Pid = u32;

/// Capability slot handle returned from [`cap_transfer`].
#[cfg(nexus_env = "os")]
pub type Cap = u32;

/// Handle identifying a virtual memory object (VMO).
#[cfg(nexus_env = "os")]
pub type Handle = u32;

/// Opaque handle referencing a user address space managed by the kernel.
#[cfg(nexus_env = "os")]
pub type AsHandle = u64;

/// Result returned by privileged syscalls that expose kernel operations.
#[cfg(nexus_env = "os")]
pub type SysResult<T> = core::result::Result<T, AbiError>;

/// Errors surfaced when invoking privileged syscalls from userland.
#[cfg(nexus_env = "os")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiError {
    /// Syscall number is not implemented by the kernel build.
    InvalidSyscall,
    /// Kernel rejected the request due to missing rights or invalid slots.
    CapabilityDenied,
    /// Kernel-side IPC machinery reported a routing error.
    IpcFailure,
    /// Kernel rejected process creation.
    SpawnFailed,
    /// Kernel rejected capability transfer.
    TransferFailed,
    /// Caller does not have any children to wait on.
    ChildUnavailable,
    /// Requested process identifier does not belong to the caller.
    NoSuchPid,
    /// Syscall arguments were invalid for the requested operation.
    InvalidArgument,
    /// The operation's deadline elapsed (ETIMEDOUT).
    TimedOut,
    /// The operation would block / resource temporarily unavailable (EAGAIN).
    WouldBlock,
    /// Kernel returned an error code this ABI build does not know. NEVER
    /// treated as success (fail closed) — a Phase C workpool hang traced back
    /// to -ETIMEDOUT being decoded as Ok.
    Unknown,
    /// Operation unsupported on the current build target.
    Unsupported,
}

/// Spawn failure reasons reported by the kernel (RFC-0013).
#[cfg(nexus_env = "os")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnFailReason {
    /// Unknown/unspecified reason.
    Unknown = 0,
    /// Allocation or memory exhaustion.
    OutOfMemory = 1,
    /// Capability table exhausted.
    CapTableFull = 2,
    /// IPC endpoint quota exhausted.
    EndpointQuota = 3,
    /// Address-space map or handle failure.
    MapFailed = 4,
    /// Invalid or malformed payload/arguments.
    InvalidPayload = 5,
    /// Spawn denied by policy (if gating applies).
    DeniedByPolicy = 6,
}

/// Scheduler quality-of-service hint classes (stable wire values).
#[cfg(nexus_env = "os")]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QosClass {
    /// Lowest-priority background work.
    Idle = 0,
    /// Default service-level scheduling class.
    Normal = 1,
    /// Latency-sensitive interactive class.
    Interactive = 2,
    /// Highest-performance burst class.
    PerfBurst = 3,
}

#[cfg(nexus_env = "os")]
impl QosClass {
    /// Decodes a kernel wire value into a typed QoS class.
    pub const fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Normal),
            2 => Some(Self::Interactive),
            3 => Some(Self::PerfBurst),
            _ => None,
        }
    }
}

#[cfg(nexus_env = "os")]
impl SpawnFailReason {
    /// Decodes a reason token into the enum, defaulting to Unknown.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::OutOfMemory,
            2 => Self::CapTableFull,
            3 => Self::EndpointQuota,
            4 => Self::MapFailed,
            5 => Self::InvalidPayload,
            6 => Self::DeniedByPolicy,
            _ => Self::Unknown,
        }
    }
}

#[cfg(nexus_env = "os")]
impl AbiError {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn from_raw(value: usize) -> Option<Self> {
        if (value as isize) >= 0 {
            return None;
        }
        // Kernel returns negative errno values for syscall failures.
        match -(value as isize) as usize {
            38 => Some(Self::InvalidSyscall),   // ENOSYS
            1 => Some(Self::CapabilityDenied),  // EPERM
            22 => Some(Self::InvalidArgument),  // EINVAL
            10 => Some(Self::ChildUnavailable), // ECHILD
            3 => Some(Self::NoSuchPid),         // ESRCH
            12 => Some(Self::SpawnFailed),      // ENOMEM (best-effort mapping)
            28 => Some(Self::SpawnFailed),      // ENOSPC (best-effort mapping)
            110 => Some(Self::TimedOut),        // ETIMEDOUT
            11 => Some(Self::WouldBlock),       // EAGAIN
            // Fail closed: an unknown NEGATIVE code is an error, never a
            // success value (Phase C: -ETIMEDOUT used to decode as Ok and
            // turned every fence/waitset timeout into a silent pseudo-Ok).
            _ => Some(Self::Unknown),
        }
    }
}

// ——— Syscall wrappers (OS build) ———

/// Cooperative yield hint to the scheduler.
#[cfg(nexus_env = "os")]
pub fn yield_() -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_YIELD: usize = 0;
        let raw = unsafe {
            // SAFETY: performs a kernel ecall with no arguments; return value is decoded below.
            ecall0(SYSCALL_YIELD)
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Returns the current monotonic time in nanoseconds (kernel timer).
#[cfg(nexus_env = "os")]
pub fn nsec() -> SysResult<u64> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_NSEC: usize = 1;
        let raw = unsafe { ecall0(SYSCALL_NSEC) };
        decode_syscall(raw).map(|v| v as u64)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// True when the kernel resolved an INTERACTIVE boot (`SYSCALL_BOOT_MODE` → 1), so a U-mode service
/// should fold its boot markers into a `<service> N/N` verdict. Proof/unknown and host return
/// `false` (raw markers, keeping `verify-uart` deterministic). Lets every service share the kernel's
/// fw_cfg-derived mode without mapping fw_cfg itself — the keystone for per-service verdict folding.
#[must_use]
pub fn boot_should_fold_verdicts() -> bool {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_BOOT_MODE: usize = 45;
        let raw = unsafe { ecall0(SYSCALL_BOOT_MODE) };
        decode_syscall(raw).map(|v| v == 1).unwrap_or(false)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        false
    }
}

// ── Per-process service verdict (alloc-free) ─────────────────────────────────────────────────
// In an interactive boot a service folds its routine boot markers into one `[ts] OK <service> N/N
// <ms>` grid line (the same form the kernel emits for `kself`). Fold mode is set once at service
// bootstrap from [`boot_should_fold_verdicts`]; proof boots never fold, so `verify-uart` still sees
// every raw marker. Counters only — no heap, no replay buffer. A FAILED marker is counted and
// printed live; routine markers are suppressed. The flush is paired with the tally so nothing is
// dropped without a verdict; after the flush, folding stops (later runtime markers print raw).
// Lives in nexus-abi (the universal dep) so any service can use it without a new dependency.
static SVC_FOLD: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static SVC_TALLY: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static SVC_FAILS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static SVC_FIRST_NS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
// Per-process opt-in for AUTO-folding `debug_println` markers. Only an ARMED process folds its
// debug_println lines into its verdict — so a folding-but-never-flushing process (init, the
// selftest observer) keeps printing raw and never loses a line. Services whose markers go through
// a custom funnel (e.g. keystored's emit_line → service_marker) need NOT arm.
static SVC_ARMED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
// Per-group EXPAND override (the configurable grid): when set, THIS process does NOT fold — it prints
// every marker raw and emits no verdict line, so you can focus on one group's full detail while every
// other group stays compactly folded. Set from config (`NEXUS_LOG_EXPAND=<group>`) at bootstrap.
// Nothing is hidden: folding is just the default view; expand recalls the raw stream per group.
static SVC_EXPAND: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
// Persistent interactive-fold policy: like SVC_FOLD but NEVER cleared at the `ready` flush, so
// post-`ready` runtime TRACE lines (IPC rx/tx, cap moves, audit echoes) can still fold in the
// interactive view even though the verdict is already emitted. Set once at bootstrap (set_verdict_fold).
static SVC_FOLD_MODE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Opt THIS process out of folding (the configurable per-group EXPAND): all its markers print raw and
/// no verdict line is emitted, while every other group stays folded. Call at bootstrap when this
/// service is in the config's expand set. Overrides fold/arm.
pub fn set_verdict_expand(on: bool) {
    use core::sync::atomic::Ordering;
    SVC_EXPAND.store(on, Ordering::Relaxed);
}

/// Enable per-process verdict folding. Call once at service bootstrap with the kernel boot mode.
/// When enabling, also stamps the init-start time so the verdict's `<ms>` measures bootstrap→ready
/// (the real service init duration), not just the span between the first folded marker and flush.
pub fn set_verdict_fold(on: bool) {
    use core::sync::atomic::Ordering;
    SVC_FOLD.store(on, Ordering::Relaxed);
    // Persistent twin (survives the flush) so post-`ready` runtime traces keep folding via service_trace().
    SVC_FOLD_MODE.store(on, Ordering::Relaxed);
    if on {
        SVC_FIRST_NS.store(span_now_ns(), Ordering::Relaxed);
    }
}

/// Arm AUTO-folding of this process's `debug_println` markers into its verdict (call once at the
/// service's `os_entry`, for services whose markers are scattered `debug_println` rather than a
/// single funnel — gpud, windowd). A no-op effect in proof boots (folding still gated on the mode).
/// Pair with [`service_verdict_flush`] at the service's ready point.
pub fn service_verdict_arm() {
    use core::sync::atomic::Ordering;
    SVC_ARMED.store(true, Ordering::Relaxed);
}

// Only the `nexus_env = "os"` `debug_println` reads this (the host build's println never folds), so
// gate it to its sole caller to keep host clippy of nexus-abi consumers warning-free.
#[cfg(nexus_env = "os")]
#[inline]
fn svc_armed() -> bool {
    SVC_ARMED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Tally one marker, auto-detecting failure from its text (contains `err`/`FAIL`/`denied`).
/// Returns `true` when the caller should SUPPRESS the line (routine marker folded). The one-call
/// convenience a service's marker funnel uses: `if nexus_abi::service_marker(msg) { return; }`.
#[must_use]
pub fn service_marker(line: &[u8]) -> bool {
    service_marker_tally(marker_is_failure(line))
}

/// Shared failure heuristic for service markers (a failure is counted + always printed live).
fn marker_is_failure(b: &[u8]) -> bool {
    fn has(h: &[u8], n: &[u8]) -> bool {
        n.len() <= h.len() && h.windows(n.len()).any(|w| w == n)
    }
    // RFC-0068: match `err` only as a TOKEN (`error`, ` err`, `err=`) — never as a bare substring,
    // which false-flagged routine lines like `audit emit deferred` / `interrupt` as failures and so
    // forced them to print raw instead of folding. The real failure markers all use one of these
    // token forms (`recv error`, `registry recv err`, `err={err}`).
    has(b, b"error")
        || has(b, b" err")
        || has(b, b"err=")
        || has(b, b"FAIL")
        || has(b, b"denied")
}

/// Tally one of this service's markers; returns `true` when the caller should SUPPRESS the line
/// (routine marker folded into the verdict). Only suppresses while folding; a `failed` marker is
/// counted as a failure and never suppressed.
#[must_use]
pub fn service_marker_tally(failed: bool) -> bool {
    use core::sync::atomic::Ordering;
    // Expanded group, or not folding (proof) → never suppress: print raw.
    if SVC_EXPAND.load(Ordering::Relaxed) || !SVC_FOLD.load(Ordering::Relaxed) {
        return false;
    }
    if SVC_FIRST_NS.load(Ordering::Relaxed) == 0 {
        SVC_FIRST_NS.store(span_now_ns(), Ordering::Relaxed);
    }
    SVC_TALLY.fetch_add(1, Ordering::Relaxed);
    if failed {
        SVC_FAILS.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    true
}

/// Suppress one routine *runtime* TRACE line — post-`ready` plumbing a service emits forever (IPC
/// rx/tx ops, cap moves, audit echoes already journaled in logd). Returns `true` when the caller
/// should SUPPRESS the line. Unlike [`service_marker`] this is independent of the per-process verdict
/// (already flushed at `ready`): it just hides recall-only detail in the folded interactive view.
/// Always prints raw in proof boots (fold mode off) and when this group is expanded
/// (`NEXUS_LOG_EXPAND=<svc>` → [`set_verdict_expand`]); nothing is ever lost. The shared SSOT every
/// service's `emit_trace` funnel uses: `if nexus_abi::service_trace() { return; }`.
#[must_use]
pub fn service_trace() -> bool {
    use core::sync::atomic::Ordering;
    SVC_FOLD_MODE.load(Ordering::Relaxed) && !SVC_EXPAND.load(Ordering::Relaxed)
}

/// Combined funnel gate for a service's `emit_line`: PRE-`ready` markers fold into the per-process
/// verdict (tally), POST-`ready` routine markers fold into recall-only detail, and failures/proof
/// boots always print. Returns `true` when the caller should SUPPRESS the line. The drop-in upgrade
/// of [`service_marker`] for funnel services that also emit runtime markers after `ready`:
/// `if nexus_abi::service_line(msg.as_bytes()) { return; }`.
#[must_use]
pub fn service_line(line: &[u8]) -> bool {
    service_marker(line) || (!marker_is_failure(line) && service_trace())
}

/// Emit this service's verdict as one atomic grid line, then stop folding (later runtime markers
/// print raw). No-op when not folding or nothing was tallied. Pairs with [`service_marker_tally`]
/// so folded markers are never lost without a verdict.
pub fn service_verdict_flush(service: &str) {
    use core::sync::atomic::Ordering;
    // Expanded group prints raw detail (no verdict line); proof boots don't fold either.
    if SVC_EXPAND.load(Ordering::Relaxed) || !SVC_FOLD.load(Ordering::Relaxed) {
        return;
    }
    let total = SVC_TALLY.load(Ordering::Relaxed);
    if total != 0 {
        let fails = SVC_FAILS.load(Ordering::Relaxed);
        let first = SVC_FIRST_NS.load(Ordering::Relaxed);
        let now = span_now_ns();
        // RFC-0068: the verdict math (passed/total, ms, OK/WARN-slow/ERROR with the soft-real-time
        // SLOW budget) lives once in nexus-event; this is just the per-process atomic feeder. `first
        // == 0` is the unset sentinel (a real marker's nsec is never 0 here).
        let started_at = (first != 0).then_some(first);
        let v = nexus_event::verdict_from(total, fails, started_at, now);
        svc_emit_verdict_line(now, service, v);
    }
    SVC_FOLD.store(false, Ordering::Relaxed);
    SVC_ARMED.store(false, Ordering::Relaxed);
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn svc_emit_verdict_line(now: u64, service: &str, v: nexus_event::Verdict) {
    // RFC-0068: the grid-line FORMAT is the shared SSOT in nexus-event (same renderer the kernel
    // GROUP flush uses) — this just owns the fixed buffer + the one atomic console write.
    let mut buf = [0u8; 96];
    let n = nexus_event::render_verdict_line(&mut buf, now, service, v);
    let _ = debug_write(&buf[..n]);
}
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn svc_emit_verdict_line(_now: u64, _service: &str, _v: nexus_event::Verdict) {}

/// Current monotonic nanoseconds, or 0 where unavailable (host). Internal to [`Span`].
#[cfg(nexus_env = "os")]
fn span_now_ns() -> u64 {
    nsec().unwrap_or(0)
}
#[cfg(not(nexus_env = "os"))]
fn span_now_ns() -> u64 {
    0
}

/// A monotonic timing span over the kernel clock, for boot/section instrumentation (the
/// signpost primitive). Reading is one cheap syscall; on host (no clock) it degrades to zero
/// duration so the same instrumentation compiles and runs in host tests.
pub struct Span {
    start_ns: u64,
}

impl Span {
    /// Begins a span at the current monotonic time.
    pub fn begin() -> Self {
        Self { start_ns: span_now_ns() }
    }

    /// Nanoseconds elapsed since [`begin`](Self::begin).
    pub fn elapsed_ns(&self) -> u64 {
        span_now_ns().saturating_sub(self.start_ns)
    }

    /// Whole milliseconds elapsed since [`begin`](Self::begin).
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ns() / 1_000_000
    }

    /// Whole microseconds elapsed since [`begin`](Self::begin).
    pub fn elapsed_us(&self) -> u64 {
        self.elapsed_ns() / 1_000
    }
}

/// Creates a kernel timer capability bound to `notify_ep_cap`.
///
/// `notify_ep_cap` must reference an endpoint capability in the caller's cap table.
/// `interval_ns` configures periodic mode when non-zero (0 = one-shot).
#[cfg(nexus_env = "os")]
pub fn timer_create(notify_ep_cap: Cap, interval_ns: u64) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TIMER_CREATE: usize = 33;
        let raw =
            unsafe { ecall2(SYSCALL_TIMER_CREATE, notify_ep_cap as usize, interval_ns as usize) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (notify_ep_cap, interval_ns);
        Err(AbiError::Unsupported)
    }
}

/// Arms a timer capability with an absolute monotonic `deadline_ns`.
#[cfg(nexus_env = "os")]
pub fn timer_set(timer_cap: Cap, deadline_ns: u64) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TIMER_SET: usize = 34;
        let raw = unsafe { ecall2(SYSCALL_TIMER_SET, timer_cap as usize, deadline_ns as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (timer_cap, deadline_ns);
        Err(AbiError::Unsupported)
    }
}

/// Disarms a previously armed timer capability.
#[cfg(nexus_env = "os")]
pub fn timer_cancel(timer_cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TIMER_CANCEL: usize = 35;
        let raw = unsafe { ecall1(SYSCALL_TIMER_CANCEL, timer_cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = timer_cap;
        Err(AbiError::Unsupported)
    }
}

/// Creates an empty **waitset** capability (RFC-0033). A waitset lets a task block
/// on MULTIPLE endpoints at once (commands + a timer-notify + a fence-notify) and
/// wake on the first ready — the first-class replacement for using a recv timeout
/// as a clock. Add members with [`waitset_add`], block with [`waitset_wait`].
#[cfg(nexus_env = "os")]
pub fn waitset_create() -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAITSET_CREATE: usize = 38;
        let raw = unsafe { ecall0(SYSCALL_WAITSET_CREATE) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Adds an endpoint (RECV right required) as a member of `waitset_cap`. Bounded:
/// over the member limit rejects with `ResourceExhausted`; a non-endpoint cap
/// rejects with `InvalidArgument`. A timer- or fence-notify endpoint is added the
/// same way, so one waitset unifies command, timer, and completion waits.
#[cfg(nexus_env = "os")]
pub fn waitset_add(waitset_cap: Cap, endpoint_cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAITSET_ADD: usize = 39;
        let raw =
            unsafe { ecall2(SYSCALL_WAITSET_ADD, waitset_cap as usize, endpoint_cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (waitset_cap, endpoint_cap);
        Err(AbiError::Unsupported)
    }
}

/// Blocks until any member endpoint of `waitset_cap` has a pending message, then
/// returns that member's **slot index** (the order it was added). The caller then
/// `ipc_recv`s that endpoint. `deadline_ns == 0` blocks indefinitely (pacing comes
/// from a timer member's fixed deadline, not from this call — so re-entry never
/// resets a clock); a non-zero deadline returns `TimedOut` when it elapses.
#[cfg(nexus_env = "os")]
pub fn waitset_wait(waitset_cap: Cap, deadline_ns: u64) -> SysResult<u32> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAITSET_WAIT: usize = 40;
        let raw =
            unsafe { ecall2(SYSCALL_WAITSET_WAIT, waitset_cap as usize, deadline_ns as usize) };
        decode_syscall(raw).map(|slot| slot as u32)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (waitset_cap, deadline_ns);
        Err(AbiError::Unsupported)
    }
}

/// Creates a timeline **fence** capability (RFC-0033). A fence holds a monotonic `u64`
/// value: producers advance it with [`fence_signal`], consumers block for a target with
/// [`fence_wait`]. It is the completion/ordering primitive for the DriverKit submit ring
/// (a producer signals a sequence number; consumers wait for it).
#[cfg(nexus_env = "os")]
pub fn fence_create() -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_FENCE_CREATE: usize = 41;
        let raw = unsafe { ecall0(SYSCALL_FENCE_CREATE) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Advances `fence_cap` monotonically to at least `value` (a lower value is a no-op) and
/// wakes every waiter the new value now satisfies.
#[cfg(nexus_env = "os")]
pub fn fence_signal(fence_cap: Cap, value: u64) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_FENCE_SIGNAL: usize = 42;
        let raw = unsafe { ecall2(SYSCALL_FENCE_SIGNAL, fence_cap as usize, value as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (fence_cap, value);
        Err(AbiError::Unsupported)
    }
}

/// Blocks until `fence_cap`'s value reaches `target`. `deadline_ns == 0` blocks
/// indefinitely; a non-zero deadline returns `TimedOut` when it elapses. Unlike a
/// recv-timeout clock, the deadline is a fixed wall-clock cap, so re-entry never resets it.
#[cfg(nexus_env = "os")]
pub fn fence_wait(fence_cap: Cap, target: u64, deadline_ns: u64) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_FENCE_WAIT: usize = 43;
        let raw = unsafe {
            ecall3(SYSCALL_FENCE_WAIT, fence_cap as usize, target as usize, deadline_ns as usize)
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (fence_cap, target, deadline_ns);
        Err(AbiError::Unsupported)
    }
}

/// Binds an external interrupt source (PLIC) to an endpoint the caller owns, so
/// the kernel routes that device IRQ to `endpoint_cap` and wakes a blocked
/// receiver — the reactive alternative to polling the device. The driver then
/// blocks on `recv(endpoint)` and calls [`irq_complete`] after servicing it.
#[cfg(nexus_env = "os")]
pub fn irq_bind(irq: u32, endpoint_cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IRQ_BIND: usize = 36;
        let raw = unsafe { ecall2(SYSCALL_IRQ_BIND, irq as usize, endpoint_cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (irq, endpoint_cap);
        Err(AbiError::Unsupported)
    }
}

/// Acknowledges a delivered IRQ so the PLIC can re-arm it. Call after the device
/// has been serviced (its interrupt condition cleared, e.g. virtqueue drained).
#[cfg(nexus_env = "os")]
pub fn irq_complete(irq: u32) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IRQ_COMPLETE: usize = 37;
        let raw = unsafe { ecall1(SYSCALL_IRQ_COMPLETE, irq as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = irq;
        Err(AbiError::Unsupported)
    }
}

/// Returns the current task PID.
#[cfg(nexus_env = "os")]
pub fn pid() -> SysResult<u32> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_GETPID: usize = 25;
        let raw = unsafe { ecall0(SYSCALL_GETPID) };
        decode_syscall(raw).map(|v| v as u32)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Returns the current task's scheduler QoS hint.
#[cfg(nexus_env = "os")]
#[must_use = "qos get result must be handled"]
pub fn task_qos_get() -> SysResult<QosClass> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_QOS: usize = 15;
        const TASK_QOS_OP_GET_SELF: usize = 0;
        let raw = unsafe { ecall3(SYSCALL_TASK_QOS, TASK_QOS_OP_GET_SELF, 0, 0) };
        decode_syscall(raw)
            .and_then(|value| QosClass::from_u8(value as u8).ok_or(AbiError::InvalidArgument))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Sets the current task's scheduler QoS hint.
///
/// Policy:
/// - equal/lower transitions are allowed for self;
/// - upward transitions require the privileged set-for-target path.
#[cfg(nexus_env = "os")]
#[must_use = "qos set-self result must be handled"]
pub fn task_qos_set_self(qos: QosClass) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_QOS: usize = 15;
        const TASK_QOS_OP_SET: usize = 1;
        let target = pid()? as usize;
        let raw = unsafe { ecall3(SYSCALL_TASK_QOS, TASK_QOS_OP_SET, target, qos as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = qos;
        Err(AbiError::Unsupported)
    }
}

/// B (TASK-0042): scheduling-attribute ops via SYSCALL_SCHED=46.
/// target 0 = self; cross-task requires the QoS-admin capability.
#[cfg(nexus_env = "os")]
pub mod sched {
    use super::*;

    const SYSCALL_SCHED: usize = 48;
    const OP_GET_AFFINITY: usize = 0;
    const OP_SET_AFFINITY: usize = 1;
    const OP_GET_SHARES: usize = 2;
    const OP_SET_SHARES: usize = 3;

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    fn op(op: usize, target: usize, value: usize) -> SysResult<usize> {
        // SAFETY: plain syscall; the kernel validates every argument.
        let raw = unsafe { ecall3(SYSCALL_SCHED, op, target, value) };
        decode_syscall(raw)
    }

    /// Returns the caller's CPU affinity mask (bit N = may run on CPU N).
    #[must_use = "affinity get result must be handled"]
    pub fn get_affinity() -> SysResult<usize> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_GET_AFFINITY, 0, 0)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            Err(AbiError::Unsupported)
        }
    }

    /// Sets the caller's CPU affinity mask. The kernel validates: non-empty,
    /// within the CPU ceiling, intersecting the online set.
    #[must_use = "affinity set result must be handled"]
    pub fn set_affinity(mask: usize) -> SysResult<()> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_SET_AFFINITY, 0, mask).map(|_| ())
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = mask;
            Err(AbiError::Unsupported)
        }
    }

    /// Returns the caller's scheduling shares [1, 1000].
    #[must_use = "shares get result must be handled"]
    pub fn get_shares() -> SysResult<usize> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_GET_SHARES, 0, 0)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            Err(AbiError::Unsupported)
        }
    }

    /// Sets the caller's scheduling shares; the kernel clamps to [1, 1000]
    /// and returns the applied value.
    #[must_use = "shares set result must be handled"]
    pub fn set_shares(shares: usize) -> SysResult<usize> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            op(OP_SET_SHARES, 0, shares)
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = shares;
            Err(AbiError::Unsupported)
        }
    }

    /// Cross-task affinity (B4: execd applies declarative sched recipes).
    /// Requires QoS-admin standing in the kernel (execd/policyd).
    #[must_use = "sched outcomes must be handled"]
    pub fn set_affinity_for(pid: u32, mask: usize) -> SysResult<()> {
        op(OP_SET_AFFINITY, pid as usize, mask).map(|_| ())
    }

    /// Cross-task shares (B4).
    #[must_use = "sched outcomes must be handled"]
    pub fn set_shares_for(pid: u32, shares: usize) -> SysResult<()> {
        op(OP_SET_SHARES, pid as usize, shares).map(|_| ())
    }
}

/// Sets another task's scheduler QoS hint (privileged path).
#[cfg(nexus_env = "os")]
#[must_use = "qos set-for-target result must be handled"]
pub fn task_qos_set_for(target: Pid, qos: QosClass) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_QOS: usize = 15;
        const TASK_QOS_OP_SET: usize = 1;
        let raw =
            unsafe { ecall3(SYSCALL_TASK_QOS, TASK_QOS_OP_SET, target as usize, qos as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (target, qos);
        Err(AbiError::Unsupported)
    }
}

/// Spawns a new task using the provided entry point, stack, bootstrap endpoint, and GP value.
#[cfg(nexus_env = "os")]
pub fn spawn(
    entry_pc: u64,
    stack_sp: u64,
    asid: u64,
    bootstrap_ep: u32,
    global_pointer: u64,
) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SPAWN: usize = 7;
        let raw = unsafe {
            // SAFETY: the syscall interface expects raw register arguments and returns the new PID
            // or a sentinel error code; all inputs are forwarded as provided by the caller.
            ecall5(
                SYSCALL_SPAWN,
                entry_pc as usize,
                stack_sp as usize,
                asid as usize,
                bootstrap_ep as usize,
                global_pointer as usize,
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// C (Phase C): returns the caller's own address-space handle (raw).
#[cfg(nexus_env = "os")]
#[must_use = "as_self result must be handled"]
pub fn as_self() -> SysResult<u32> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_AS_SELF: usize = 49;
        let raw = unsafe { ecall0(SYSCALL_AS_SELF) };
        decode_syscall(raw).map(|v| v as u32)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// C (Phase C): same-address-space threads for COMPUTE work.
///
/// Contract (v1, TASK-0276 policy):
/// - A thread shares the parent's address space but has an EMPTY capability
///   table — it cannot do capability IPC. Threads are for computation; the
///   owning task keeps all service communication single-threaded.
/// - The caller provides the stack (no guard page in v1 — document your
///   sizes; the kernel-side spawn validates alignment only).
/// - No TLS in v1: `tp` is free for a user-defined context pointer.
/// - On return from `entry`, the thread exits with status 0 (trampoline).
///   The parent reaps it via `wait(pid)`.
#[cfg(nexus_env = "os")]
pub mod thread {
    use super::*;

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    core::arch::global_asm!(
        r#"
        .section .text.__nexus_thread_trampoline, "ax", @progbits
        .globl __nexus_thread_trampoline
        .align 2
    __nexus_thread_trampoline:
        /* stack top layout (set up by spawn_thread): [entry, arg] */
        ld    t0, 0(sp)
        ld    a0, 8(sp)
        addi  sp, sp, 16
        jalr  ra, t0, 0
        /* entry returned: exit(0) */
        li    a7, 11
        li    a0, 0
        ecall
    1:  j 1b
    "#
    );

    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    extern "C" {
        fn __nexus_thread_trampoline();
    }

    /// Spawns a compute thread into the caller's address space, running
    /// `entry(arg)` on the provided stack. Returns the thread's task id;
    /// reap it with [`super::wait`] after it exits.
    #[must_use = "thread spawn result must be handled"]
    pub fn spawn_thread(
        entry: extern "C" fn(usize),
        arg: usize,
        stack: &mut [u8],
    ) -> SysResult<Pid> {
        let pid = spawn_thread_suspended(entry, arg, stack)?;
        task_resume(pid)?;
        Ok(pid)
    }

    /// Like [`spawn_thread`] but leaves the thread SUSPENDED so the parent
    /// can transfer capabilities (e.g. fence caps for a workpool) into the
    /// thread's empty cap table before releasing it via [`super::task_resume`].
    #[must_use = "thread spawn result must be handled"]
    pub fn spawn_thread_suspended(
        entry: extern "C" fn(usize),
        arg: usize,
        stack: &mut [u8],
    ) -> SysResult<Pid> {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            if stack.len() < 1024 {
                return Err(AbiError::InvalidArgument);
            }
            let top = (stack.as_ptr() as usize + stack.len()) & !15usize;
            let sp = top - 16;
            // SAFETY: sp/sp+8 lie inside the caller-provided stack slice.
            unsafe {
                core::ptr::write(sp as *mut usize, entry as usize);
                core::ptr::write((sp + 8) as *mut usize, arg);
            }
            let gp: usize;
            // SAFETY: reading gp is side-effect free; the thread shares our
            // address space and must use the same global pointer.
            unsafe {
                core::arch::asm!("mv {g}, gp", g = out(reg) gp, options(nomem, nostack, preserves_flags));
            }
            let handle = as_self()?;
            // Same-AS spawns start suspended by kernel contract.
            spawn(
                __nexus_thread_trampoline as usize as u64,
                sp as u64,
                handle as u64,
                0,
                gp as u64,
            )
        }
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        {
            let _ = (entry, arg, stack);
            Err(AbiError::Unsupported)
        }
    }
}

/// Resumes a suspended task (enqueues into scheduler). Only callable by the parent.
/// Returns `Ok(())` on success, `Err(InvalidArgument)` if the task is not suspended.
#[cfg(nexus_env = "os")]
pub fn task_resume(pid: Pid) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_TASK_RESUME: usize = 32;
        let raw = unsafe { ecall1(SYSCALL_TASK_RESUME, pid as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = pid;
        Err(AbiError::Unsupported)
    }
}

/// Returns the last spawn failure reason for the current task (RFC-0013).
#[cfg(nexus_env = "os")]
pub fn spawn_last_error() -> SysResult<SpawnFailReason> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_SPAWN_LAST_ERROR: usize = 29;
        let raw = unsafe { ecall0(SYSCALL_SPAWN_LAST_ERROR) };
        decode_syscall(raw).map(|v| SpawnFailReason::from_u8(v as u8))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Loads and spawns a process from an ELF blob using the kernel exec loader.
#[cfg(nexus_env = "os")]
pub fn exec(elf: &[u8], stack_pages: usize, global_pointer: u64) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_EXEC: usize = 13;
        if stack_pages == 0 || elf.is_empty() {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe {
            ecall4(
                SYSCALL_EXEC,
                elf.as_ptr() as usize,
                elf.len(),
                stack_pages,
                global_pointer as usize,
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (elf, stack_pages, global_pointer);
        Err(AbiError::Unsupported)
    }
}

/// Loads and spawns a process from an ELF blob using the kernel exec loader (v2).
///
/// v2 additionally provides a per-service name string that the kernel copies into a read-only
/// mapping in the child address space (RFC-0004 provenance floor).
#[cfg(nexus_env = "os")]
pub fn exec_v2(
    elf: &[u8],
    stack_pages: usize,
    global_pointer: u64,
    service_name: &str,
) -> SysResult<Pid> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_EXEC_V2: usize = 17;
        if stack_pages == 0 || elf.is_empty() {
            return Err(AbiError::InvalidArgument);
        }
        // Keep the ABI bounded (kernel enforces too).
        if service_name.len() > 64 {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe {
            ecall6(
                SYSCALL_EXEC_V2,
                elf.as_ptr() as usize,
                elf.len(),
                stack_pages,
                global_pointer as usize,
                service_name.as_ptr() as usize,
                service_name.len(),
            )
        };
        decode_syscall(raw).map(|pid| pid as Pid)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (elf, stack_pages, global_pointer, service_name);
        Err(AbiError::Unsupported)
    }
}

/// Terminates the current task with the provided exit `status`.
#[cfg(nexus_env = "os")]
pub fn exit(status: i32) -> ! {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_EXIT: usize = 11;
        let _ = ecall1(SYSCALL_EXIT, status as usize);
        core::hint::spin_loop();
        loop {
            core::hint::spin_loop();
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = status;
        loop {
            core::hint::spin_loop();
        }
    }
}

/// Waits for the child identified by `pid` (or any child when `pid <= 0`).
#[cfg(nexus_env = "os")]
pub fn wait(pid: i32) -> SysResult<(Pid, i32)> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_WAIT: usize = 12;
        let (raw_pid, raw_status) = unsafe { ecall1_pair(SYSCALL_WAIT, pid as usize) };
        let pid = decode_syscall(raw_pid)?;
        Ok((pid as Pid, raw_status as i32))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = pid;
        Err(AbiError::Unsupported)
    }
}

/// Transfers a capability from the current task to `dst_task` with intersected `rights`.
#[cfg(nexus_env = "os")]
pub fn cap_transfer(dst_task: Pid, cap: Cap, rights: Rights) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_TRANSFER: usize = 8;
        let raw = unsafe {
            // SAFETY: forwards raw arguments expected by the kernel capability transfer ABI.
            ecall3(SYSCALL_CAP_TRANSFER, dst_task as usize, cap as usize, rights.bits() as usize)
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Transfers a capability from the current task to `dst_task` into `dst_slot`.
#[cfg(nexus_env = "os")]
pub fn cap_transfer_to_slot(
    dst_task: Pid,
    cap: Cap,
    rights: Rights,
    dst_slot: Cap,
) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_TRANSFER_TO: usize = 31;
        let raw = unsafe {
            ecall4(
                SYSCALL_CAP_TRANSFER_TO,
                dst_task as usize,
                cap as usize,
                rights.bits() as usize,
                dst_slot as usize,
            )
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (dst_task, cap, rights, dst_slot);
        Err(AbiError::Unsupported)
    }
}

/// Creates a new kernel IPC endpoint and returns a capability slot for it.
///
/// Bring-up rule: this syscall is currently restricted to init-lite (the direct child of the
/// bootstrap task, parent PID 0), acting as the temporary endpoint factory (RFC-0005 Phase 2
/// hardening).
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_create(queue_depth: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CREATE: usize = 19;
        if queue_depth == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe { ecall1(SYSCALL_IPC_ENDPOINT_CREATE, queue_depth) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = queue_depth;
        Err(AbiError::Unsupported)
    }
}

/// Creates a new kernel IPC endpoint using an endpoint-factory capability slot.
///
/// This is the hardened replacement for `ipc_endpoint_create()` (v1). The caller must hold a
/// `CapabilityKind::EndpointFactory` capability with `Rights::MANAGE` in `factory_cap`.
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_create_v2(factory_cap: Cap, queue_depth: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CREATE_V2: usize = 22;
        if queue_depth == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let raw =
            unsafe { ecall3(SYSCALL_IPC_ENDPOINT_CREATE_V2, factory_cap as usize, queue_depth, 0) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (factory_cap, queue_depth);
        Err(AbiError::Unsupported)
    }
}

/// Creates a new kernel IPC endpoint and assigns ownership to `owner_pid`.
///
/// This is a bootstrap helper used by init-lite so endpoints created during bring-up can be owned
/// by the target service (close-on-exit semantics), while init-lite retains the creator capability
/// for rights-filtered distribution.
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_create_for(
    factory_cap: Cap,
    owner_pid: u32,
    queue_depth: usize,
) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CREATE_FOR: usize = 23;
        if queue_depth == 0 {
            return Err(AbiError::InvalidArgument);
        }
        let raw = unsafe {
            ecall3(
                SYSCALL_IPC_ENDPOINT_CREATE_FOR,
                factory_cap as usize,
                owner_pid as usize,
                queue_depth,
            )
        };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (factory_cap, owner_pid, queue_depth);
        Err(AbiError::Unsupported)
    }
}

/// Closes an IPC endpoint referenced by `cap` (slot id) if the capability includes `Rights::MANAGE`.
///
/// This is a *global close* (revocation-by-close): once closed, subsequent IPC operations on the
/// endpoint fail deterministically (`NoSuchEndpoint`).
#[cfg(nexus_env = "os")]
pub fn ipc_endpoint_close(cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_IPC_ENDPOINT_CLOSE: usize = 21;
        let raw = unsafe { ecall1(SYSCALL_IPC_ENDPOINT_CLOSE, cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = cap;
        Err(AbiError::Unsupported)
    }
}

/// Drops the caller's reference to the capability slot identified by `cap`.
#[cfg(nexus_env = "os")]
pub fn cap_close(cap: Cap) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_CLOSE: usize = 20;
        let raw = unsafe { ecall1(SYSCALL_CAP_CLOSE, cap as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = cap;
        Err(AbiError::Unsupported)
    }
}

/// Clones a capability slot locally.
///
/// Returns the newly allocated slot in the caller. This is a local duplicate only (no transfer).
#[cfg(nexus_env = "os")]
pub fn cap_clone(cap: Cap) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_CLONE: usize = 24;
        let raw = unsafe { ecall1(SYSCALL_CAP_CLONE, cap as usize) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = cap;
        Err(AbiError::Unsupported)
    }
}

/// Drops the caller's reference to an address space handle.
#[cfg(nexus_env = "os")]
pub fn as_destroy(handle: AsHandle) -> SysResult<()> {
    let _ = handle;
    Err(AbiError::Unsupported)
}

/// Allocates a new address space and returns its opaque handle.
#[cfg(nexus_env = "os")]
pub fn as_create() -> SysResult<AsHandle> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_AS_CREATE: usize = 9;
        let raw = unsafe { ecall0(SYSCALL_AS_CREATE) };
        decode_syscall(raw).map(|handle| handle as AsHandle)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

/// Maps a VMO into the target address space referenced by `as_handle`.
#[cfg(nexus_env = "os")]
pub fn as_map(
    as_handle: AsHandle,
    vmo: Handle,
    va: u64,
    len: u64,
    prot: u32,
    flags: u32,
) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_AS_MAP: usize = 10;
        if va > usize::MAX as u64 || len > usize::MAX as u64 {
            return Err(AbiError::Unsupported);
        }
        let raw = unsafe {
            ecall6(
                SYSCALL_AS_MAP,
                as_handle as usize,
                vmo as usize,
                va as usize,
                len as usize,
                prot as usize,
                flags as usize,
            )
        };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(AbiError::Unsupported)
    }
}

// ——— VMO userland wrappers (OS build) ———

/// Creates a new contiguous VMO of `len` bytes and returns a handle to it.
///
/// The initial implementation is a placeholder; the kernel syscall path will
/// be wired in a subsequent change.
#[cfg(nexus_env = "os")]
pub fn vmo_create(_len: usize) -> Result<Handle> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_CREATE: usize = 5;
        let slot = usize::MAX;
        let len = _len;
        let raw = ecall3(SYSCALL_VMO_CREATE, slot, len, 0);
        match decode_syscall(raw) {
            Ok(slot) => Ok(slot as Handle),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Writes `bytes` into the VMO starting at `offset` bytes from the base.
#[cfg(nexus_env = "os")]
pub fn vmo_write(_handle: Handle, _offset: usize, _bytes: &[u8]) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_WRITE: usize = 6;
        let len = _bytes.len();
        let ptr = _bytes.as_ptr() as usize;
        let raw = ecall4(SYSCALL_VMO_WRITE, _handle as usize, _offset, ptr, len);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Reads bytes out of the VMO starting at `offset` into `buf` — the mirror of
/// [`vmo_write`] (syscall 47). The ADR-0042 compositor damage-blit is the
/// first consumer: windowd reads app-surface pixels through this (userspace
/// has no VMO mapping path).
#[cfg(nexus_env = "os")]
pub fn vmo_read(_handle: Handle, _offset: usize, _buf: &mut [u8]) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_VMO_READ: usize = 47;
        let len = _buf.len();
        let ptr = _buf.as_mut_ptr() as usize;
        let raw = ecall4(SYSCALL_VMO_READ, _handle as usize, _offset, ptr, len);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Maps the VMO into the caller's address space at virtual address `va` with
/// the requested flags. The mapping is read-only in the initial path.
#[cfg(nexus_env = "os")]
pub fn vmo_map(_handle: Handle, _va: usize, _flags: u32) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_MAP: usize = 4;
        // Offset=0 for the minimal path; flags passed as fourth arg.
        let raw = ecall4(SYSCALL_MAP, _handle as usize, _va, 0, _flags as usize);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        Err(IpcError::Unsupported)
    }
}

/// Page-table leaf flags for user mappings (Sv39).
///
/// These constants match `source/kernel/neuron/src/mm/page_table.rs` `PageFlags` bits.
#[cfg(nexus_env = "os")]
pub mod page_flags {
    /// Entry is valid.
    pub const VALID: u32 = 1 << 0;
    /// Readable.
    pub const READ: u32 = 1 << 1;
    /// Writable.
    pub const WRITE: u32 = 1 << 2;
    /// Executable.
    pub const EXECUTE: u32 = 1 << 3;
    /// User accessible.
    pub const USER: u32 = 1 << 4;
}

/// Maps one page of a VMO into the caller's address space at virtual address `va`.
///
/// - `va` must be 4096-byte aligned.
/// - `offset` is a byte offset into the VMO (page-aligned by the kernel).
/// - `flags` uses `page_flags::*` bits.
#[cfg(nexus_env = "os")]
pub fn vmo_map_page(_handle: Handle, _va: usize, _offset: usize, _flags: u32) -> Result<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    unsafe {
        const SYSCALL_MAP: usize = 4;
        let raw = ecall4(SYSCALL_MAP, _handle as usize, _va, _offset, _flags as usize);
        match decode_syscall(raw) {
            Ok(_) => Ok(()),
            Err(_) => Err(IpcError::Unsupported),
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _va, _offset, _flags);
        Err(IpcError::Unsupported)
    }
}

/// Like [`vmo_map_page`] but returns the raw syscall error (`AbiError`) for diagnostics.
#[cfg(nexus_env = "os")]
pub fn vmo_map_page_sys(_handle: Handle, _va: usize, _offset: usize, _flags: u32) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_MAP: usize = 4;
        let raw = unsafe { ecall4(SYSCALL_MAP, _handle as usize, _va, _offset, _flags as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _va, _offset, _flags);
        Err(AbiError::Unsupported)
    }
}

/// Maps a device MMIO window capability into the caller's address space at virtual address `va`.
///
/// Security invariants (enforced by kernel):
/// - mapping is USER + RW
/// - mapping is never executable
/// - mapping is bounded to the capability window
#[cfg(nexus_env = "os")]
pub fn mmio_map(_handle: Handle, _va: usize, _offset: usize) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_MMIO_MAP: usize = 27;
        let raw = unsafe { ecall3(SYSCALL_MMIO_MAP, _handle as usize, _va, _offset) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_handle, _va, _offset);
        Err(AbiError::Unsupported)
    }
}

/// Information about an address-bearing capability (VMO or device MMIO window).
#[cfg(nexus_env = "os")]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapQuery {
    /// 1 = VMO, 2 = DeviceMmio.
    pub kind_tag: u32,
    /// Reserved for future expansion (must be zero).
    pub reserved: u32,
    /// Physical base address for the capability's window.
    pub base: u64,
    /// Length in bytes of the capability's window.
    pub len: u64,
}

/// Queries a capability slot and writes the result into `out`.
#[cfg(nexus_env = "os")]
pub fn cap_query(_cap: Cap, _out: &mut CapQuery) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_CAP_QUERY: usize = 28;
        let out_ptr = (_out as *mut CapQuery) as usize;
        let raw = unsafe { ecall2(SYSCALL_CAP_QUERY, _cap as usize, out_ptr) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_cap, _out);
        Err(AbiError::Unsupported)
    }
}

/// Creates a DeviceMmio capability in the caller's cap table (init-only).
///
/// If `slot_raw` is `usize::MAX`, the kernel allocates a fresh slot; otherwise, the cap is placed
/// into the requested slot (must be empty).
#[cfg(nexus_env = "os")]
pub fn device_mmio_cap_create(_base: usize, _len: usize, _slot_raw: usize) -> SysResult<Cap> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_DEVICE_CAP_CREATE: usize = 30;
        let raw = unsafe { ecall3(SYSCALL_DEVICE_CAP_CREATE, _base, _len, _slot_raw) };
        decode_syscall(raw).map(|slot| slot as Cap)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (_base, _len, _slot_raw);
        Err(AbiError::Unsupported)
    }
}

/// Releases a sole-owned VMO back to the kernel arena (task #124).
///
/// For self-created, never-shared one-shot VMOs (staging buffers, the boot-splash
/// backing). The kernel refuses while any other capability in the system still
/// references the range. The caller must not touch the memory afterwards —
/// including through mappings it made with `vmo_map_page`.
#[cfg(nexus_env = "os")]
pub fn vmo_destroy(handle: Handle) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_VMO_DESTROY: usize = 46;
        let raw = unsafe { ecall1(SYSCALL_VMO_DESTROY, handle as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = handle;
        Err(AbiError::Unsupported)
    }
}

// ——— Debug print helpers (OS build) ———

/// Writes a single byte to the kernel UART from userspace for debugging.
#[cfg(nexus_env = "os")]
pub fn debug_putc(byte: u8) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_DEBUG_PUTC: usize = 16;
        let raw = unsafe { ecall1(SYSCALL_DEBUG_PUTC, byte as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = byte;
        Err(AbiError::Unsupported)
    }
}

/// Writes a byte slice to the kernel UART for debugging. The whole slice is emitted
/// atomically by the kernel under the UART lock (one syscall), so it cannot interleave
/// mid-slice with the kernel or another process.
#[cfg(nexus_env = "os")]
pub fn debug_write(bytes: &[u8]) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if bytes.is_empty() {
            return Ok(());
        }
        const SYSCALL_DEBUG_WRITE: usize = 44;
        let raw = unsafe { ecall2(SYSCALL_DEBUG_WRITE, bytes.as_ptr() as usize, bytes.len()) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        for &b in bytes {
            debug_putc(b)?;
        }
        Ok(())
    }
}

/// Writes a line (with trailing '\n') to the kernel UART for debugging. The content and the
/// newline go out in a single atomic [`debug_write`] (via a bounded stack buffer) so a line
/// is never split across two console writes; very long lines fall back to content + newline.
#[cfg(nexus_env = "os")]
pub fn debug_println(s: &str) -> SysResult<()> {
    // Verdict folding: an ARMED service folds its routine `debug_println` lines in interactive boots.
    // `service_line` folds BOTH phases: pre-`ready` markers tally into the `<service> N/N` verdict,
    // and post-`ready` runtime traces (IPC/present/chain echoes) fold into recall-only detail
    // (`NEXUS_LOG_EXPAND=<svc>`). FAIL lines and proof boots always print; only armed processes fold,
    // so init/the observer keep printing raw and never lose a line.
    if svc_armed() && service_line(s.as_bytes()) {
        return Ok(());
    }
    // RFC-0068: in interactive boots, give the raw (unfolded) marker the SAME `[    S.uuuuuu]  ` grid
    // timestamp as the verdict lines, so the `build/logs` boot timeline shows WHEN each marker fired
    // (this is how the gpud/windowd/hidrawd display+input chain becomes legible). Proof boots stay
    // unprefixed — `verify-uart` + the nexus-evidence canonicalizer see the bare line (its `[ts=…ms]`
    // parser only triggers on a `[ts=` prefix, but keeping proof output bare avoids any ambiguity).
    if boot_should_fold_verdicts() {
        let now = nsec().unwrap_or(0);
        let mut buf = [0u8; 640];
        let n = nexus_event::render_marker_ts(&mut buf, now, s);
        return debug_write(&buf[..n]);
    }
    const LINE_CAP: usize = 512;
    let bytes = s.as_bytes();
    if bytes.len() < LINE_CAP {
        let mut buf = [0u8; LINE_CAP];
        buf[..bytes.len()].copy_from_slice(bytes);
        buf[bytes.len()] = b'\n';
        debug_write(&buf[..bytes.len() + 1])
    } else {
        debug_write(bytes)?;
        debug_write(b"\n")
    }
}

/// Write the grid TIMESTAMP PREFIX `[    S.uuuuuu]  ` to the console (RFC-0068), for emitters that
/// build a marker line from `debug_putc` fragments (e.g. `abilitymgr`) rather than one `debug_println`.
/// Call it once at the START of the marker line. No-op in proof boots (keeps the evidence line bare).
#[cfg(nexus_env = "os")]
pub fn debug_ts_prefix() {
    if !boot_should_fold_verdicts() {
        return;
    }
    let now = nsec().unwrap_or(0);
    let mut buf = [0u8; 24];
    let n = nexus_event::render_ts_prefix(&mut buf, now);
    let _ = debug_write(&buf[..n]);
}

/// Emit a routine bring-up/runtime trace line that FOLDS in the interactive grid view (recall with
/// `NEXUS_LOG_EXPAND=<svc>`) and prints raw in proof boots (so `verify-uart` still sees it). The
/// shared funnel — via [`service_trace`] — for services that emit scattered `debug_println` markers
/// (gpud, dsoftbusd) rather than routing through a single `emit_line`. Unlike [`debug_trace`] (gated
/// on an explicit verbosity flag) this follows the per-process verdict-fold policy. NEVER use it for
/// SELFTEST markers or failures — those must always reach the observer + verify-uart.
#[cfg(nexus_env = "os")]
pub fn trace_line(s: &str) -> SysResult<()> {
    // Failure safety net: a marker the heuristic flags (err/FAIL/denied) is never folded, even if a
    // caller routes it here by mistake. Routine lines fold in interactive, raw in proof.
    if !marker_is_failure(s.as_bytes()) && service_trace() {
        return Ok(());
    }
    debug_println(s)
}

/// Runtime gate for developer trace breadcrumbs (see [`debug_trace`]). Off by default so
/// high-frequency dev lines stay silent in a normal boot; flipped on by the boot-time
/// verbosity knob for a focused debug session.
#[cfg(nexus_env = "os")]
static DEBUG_TRACE_ON: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Enable or disable developer trace breadcrumbs emitted via [`debug_trace`].
#[cfg(nexus_env = "os")]
pub fn set_debug_trace(on: bool) {
    DEBUG_TRACE_ON.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// Developer trace breadcrumb. Routes to [`debug_println`] only when tracing is enabled, so
/// these lines are silent by default but one runtime flag away — never use it for markers or
/// errors, which must always emit.
#[cfg(nexus_env = "os")]
pub fn debug_trace(s: &str) -> SysResult<()> {
    if DEBUG_TRACE_ON.load(core::sync::atomic::Ordering::Relaxed) {
        debug_println(s)
    } else {
        Ok(())
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn decode_syscall(value: usize) -> SysResult<usize> {
    if let Some(err) = AbiError::from_raw(value) {
        Err(err)
    } else {
        Ok(value)
    }
}

// ——— Architecture-specific ecall helpers (riscv64, OS) ———
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall0(n: usize) -> usize {
    let mut r7 = n;
    let r0: usize;
    core::arch::asm!(
        "ecall",
        inout("a7") r7,
        lateout("a0") r0,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall1(n: usize, a0: usize) -> usize {
    let mut r0 = a0;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall1_pair(n: usize, a0: usize) -> (usize, usize) {
    let mut r0 = a0;
    let mut r7 = n;
    let mut r1: usize;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        lateout("a1") r1,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    (r0, r1)
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall2(n: usize, a0: usize, a1: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall3(n: usize, a0: usize, a1: usize, a2: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall4(n: usize, a0: usize, a1: usize, a2: usize, a3: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r3 = a3;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a3") r3,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall5(n: usize, a0: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r3 = a3;
    let mut r4 = a4;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a3") r3,
        inout("a4") r4,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
#[allow(unused_assignments)]
#[inline(always)]
unsafe fn ecall6(
    n: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> usize {
    let mut r0 = a0;
    let mut r1 = a1;
    let mut r2 = a2;
    let mut r3 = a3;
    let mut r4 = a4;
    let mut r5 = a5;
    let mut r7 = n;
    core::arch::asm!(
        "ecall",
        inout("a0") r0,
        inout("a1") r1,
        inout("a2") r2,
        inout("a3") r3,
        inout("a4") r4,
        inout("a5") r5,
        inout("a7") r7,
        clobber_abi("C"),
        options(nostack)
    );
    r0
}

// Fast capability slot probing for early boot validation (OS only).
#[cfg(nexus_env = "os")]
pub mod slot_probe;

/// Deterministic userspace ABI syscall filter profile helpers.
pub mod abi_filter;

#[cfg(test)]
mod tests {
    use super::{IpcRecvV2Desc, MsgHeader};
    use core::mem::{align_of, size_of};

    #[test]
    fn header_layout() {
        assert_eq!(size_of::<MsgHeader>(), 16);
        assert_eq!(align_of::<MsgHeader>(), 4);
    }

    #[test]
    fn header_golden_vector() {
        // Inline golden vector (LE):
        // src=0x01020304, dst=0x11223344, ty=0x5566, flags=0x7788, len=0x99aabbcc
        const VECTOR: [u8; 16] = [
            0x04, 0x03, 0x02, 0x01, 0x44, 0x33, 0x22, 0x11, 0x66, 0x55, 0x88, 0x77, 0xCC, 0xBB,
            0xAA, 0x99,
        ];
        let header = MsgHeader::new(0x0102_0304, 0x1122_3344, 0x5566, 0x7788, 0x99aa_bbcc);
        assert_eq!(
            header.to_le_bytes(),
            VECTOR,
            "golden vector out of date; expected bytes: {:02x?}",
            header.to_le_bytes()
        );
        assert_eq!(MsgHeader::from_le_bytes(VECTOR), header);
    }

    #[test]
    fn round_trip() {
        let header = MsgHeader::new(1, 2, 3, 4, 5);
        assert_eq!(header, MsgHeader::from_le_bytes(header.to_le_bytes()));
    }

    #[test]
    fn recv_v2_desc_layout() {
        use core::mem::offset_of;

        assert_eq!(size_of::<IpcRecvV2Desc>(), 64);
        assert_eq!(align_of::<IpcRecvV2Desc>(), 8);

        // Offsets are part of the descriptor ABI contract.
        assert_eq!(offset_of!(IpcRecvV2Desc, magic), 0);
        assert_eq!(offset_of!(IpcRecvV2Desc, version), 4);
        assert_eq!(offset_of!(IpcRecvV2Desc, slot), 8);
        assert_eq!(offset_of!(IpcRecvV2Desc, _pad0), 12);
        assert_eq!(offset_of!(IpcRecvV2Desc, header_out_ptr), 16);
        assert_eq!(offset_of!(IpcRecvV2Desc, payload_out_ptr), 24);
        assert_eq!(offset_of!(IpcRecvV2Desc, payload_out_max), 32);
        assert_eq!(offset_of!(IpcRecvV2Desc, sender_service_id_out_ptr), 40);
        assert_eq!(offset_of!(IpcRecvV2Desc, sys_flags), 48);
        assert_eq!(offset_of!(IpcRecvV2Desc, _pad1), 52);
        assert_eq!(offset_of!(IpcRecvV2Desc, deadline_ns), 56);
    }
}
