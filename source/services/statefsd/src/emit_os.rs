// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd UART/audit emit helpers (moved verbatim out of
//!   os_lite.rs for the structure ratchet; behavior unchanged)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: Exercised via the QEMU marker ladder
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use statefs::StatefsError;
use storage::virtio_blk::VirtioBlkDevice;

pub(crate) fn emit_access_denied(path: &str, sender_service_id: u64) {
    let mut buf = [0u8; 160];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"statefsd: access denied path=");
    let _ = push_bytes(&mut buf, &mut len, path.as_bytes());
    let _ = push_bytes(&mut buf, &mut len, b" sender=0x");
    write_hex_u64(&mut buf, &mut len, sender_service_id);
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("statefsd: access denied");
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

/// TASK-0025: audit an envelope-policy denial (never key material, never
/// payload bytes — path + wire status only).
pub(crate) fn emit_envelope_denied(path: &str, status: u8) {
    let mut buf = [0u8; 160];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"statefsd: envelope deny path=");
    let _ = push_bytes(&mut buf, &mut len, path.as_bytes());
    let _ = push_bytes(&mut buf, &mut len, b" status=");
    push_u32(&mut buf, &mut len, status as u32);
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("statefsd: envelope deny");
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

/// TASK-0025: audit a migration-era raw write under an enrolled prefix.
pub(crate) fn emit_envelope_migration(path: &str) {
    let mut buf = [0u8; 160];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"statefsd: envelope migration accept path=");
    let _ = push_bytes(&mut buf, &mut len, path.as_bytes());
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("statefsd: envelope migration accept");
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

/// TASK-0025: audit one write-budget overrun (running warn count included).
pub(crate) fn emit_budget_warn(warn_count: u64, elapsed_ns: u64) {
    let mut buf = [0u8; 96];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"statefsd: write budget exceeded (ns=");
    push_u64(&mut buf, &mut len, elapsed_ns);
    let _ = push_bytes(&mut buf, &mut len, b" warns=");
    push_u64(&mut buf, &mut len, warn_count);
    let _ = push_bytes(&mut buf, &mut len, b")");
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("statefsd: write budget exceeded");
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

/// TASK-0026: one compaction cycle completed AND the rotated journal
/// re-replayed clean (reopen-verified by the caller — never emitted on the
/// engine flip alone). Also audited to logd so the selftest can cross-check
/// that compaction really ran. Marker contract: scripts/qemu-test.sh.
pub(crate) fn emit_compaction_done(generation: u32, entries: usize) {
    let mut buf = [0u8; 96];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"statefsd: compaction done (gen=");
    push_u32(&mut buf, &mut len, generation);
    let _ = push_bytes(&mut buf, &mut len, b", entries=");
    push_u64(&mut buf, &mut len, entries as u64);
    let _ = push_bytes(&mut buf, &mut len, b")");
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("statefsd: compaction done");
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

/// TASK-0026: a compaction cycle ran (or errored) but the post-cycle
/// reopen-verify did not come back clean — loud failure signature, the done
/// marker is withheld.
pub(crate) fn emit_compaction_verify_failed() {
    let msg = "statefsd: compaction verify failed";
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

pub(crate) fn emit_line(message: &str) {
    // RFC-0068: fold routine markers into recall (interactive); failures & proof print raw.
    // One atomic `debug_write` (via `debug_println`, which also owns the verdict
    // folding): the per-byte `debug_putc` fallback tears mid-line against the
    // kernel's locked log records and DROPS the tail — a torn ready marker is a
    // red ladder gate.
    let _ = nexus_abi::debug_println(message);
}

pub(crate) fn emit_statefs_error(err: StatefsError) {
    let msg = match err {
        StatefsError::NotFound => "statefsd: err not-found",
        StatefsError::AccessDenied => "statefsd: err access-denied",
        StatefsError::ValueTooLarge => "statefsd: err value-too-large",
        StatefsError::KeyTooLong => "statefsd: err key-too-long",
        StatefsError::IoError => "statefsd: err io",
        StatefsError::Corrupted => "statefsd: err corrupted",
        StatefsError::InvalidKey => "statefsd: err invalid-key",
        StatefsError::ReplayLimitExceeded => "statefsd: err replay-limit",
        StatefsError::IntegrityViolation => "statefsd: err integrity",
        StatefsError::RollbackDetected => "statefsd: err rollback",
    };
    emit_line(msg);
}

pub(crate) fn emit_ipc_error(err: nexus_ipc::IpcError) {
    let msg = match err {
        nexus_ipc::IpcError::WouldBlock => "statefsd: ipc would-block",
        nexus_ipc::IpcError::Timeout => "statefsd: ipc timeout",
        nexus_ipc::IpcError::Disconnected => "statefsd: ipc disconnected",
        nexus_ipc::IpcError::NoSpace => "statefsd: ipc no-space",
        nexus_ipc::IpcError::Kernel(err) => match err {
            nexus_abi::IpcError::NoSuchEndpoint => "statefsd: ipc no-such-endpoint",
            nexus_abi::IpcError::QueueFull => "statefsd: ipc queue-full",
            nexus_abi::IpcError::QueueEmpty => "statefsd: ipc queue-empty",
            nexus_abi::IpcError::PermissionDenied => "statefsd: ipc permission-denied",
            nexus_abi::IpcError::TimedOut => "statefsd: ipc timed-out",
            nexus_abi::IpcError::NoSpace => "statefsd: ipc no-space",
            nexus_abi::IpcError::PeerClosed => "statefsd: ipc peer-closed",
            nexus_abi::IpcError::Unsupported => "statefsd: ipc unsupported",
        },
        nexus_ipc::IpcError::Unsupported => "statefsd: ipc unsupported",
        _ => "statefsd: ipc other",
    };
    emit_line(msg);
}

pub(crate) fn emit_blk_marker(dev: &VirtioBlkDevice) {
    let ss = dev.sector_size();
    let nsec = dev.capacity_sectors();
    emit_line("blk: virtio-blk up");
    let mut buf = [0u8; 64];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"blk: virtio-blk up (ss=");
    push_u32(&mut buf, &mut len, ss);
    let _ = push_bytes(&mut buf, &mut len, b" nsec=");
    push_u64(&mut buf, &mut len, nsec);
    let _ = push_bytes(&mut buf, &mut len, b")");
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("blk: virtio-blk up");
    emit_line(msg);
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

fn push_u32(buf: &mut [u8], len: &mut usize, value: u32) {
    push_u64(buf, len, value as u64);
}

fn push_u64(buf: &mut [u8], len: &mut usize, mut value: u64) {
    let mut tmp = [0u8; 20];
    let mut pos = 0usize;
    if value == 0 {
        tmp[0] = b'0';
        pos = 1;
    } else {
        while value > 0 && pos < tmp.len() {
            tmp[pos] = b'0' + (value % 10) as u8;
            value /= 10;
            pos += 1;
        }
        tmp[..pos].reverse();
    }
    let _ = push_bytes(buf, len, &tmp[..pos]);
}

fn append_logd_audit(msg: &[u8]) {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    const SCOPE: &[u8] = b"statefsd.audit";

    if msg.len() > 256 || SCOPE.len() > 64 {
        return;
    }

    // init-lite deterministic slots for statefsd:
    // - logd send cap: 0x08
    // - reply inbox: recv=0x05, send=0x06
    let send_slot = 0x08;
    let reply_send_slot = 0x06;
    let _reply_recv_slot = 0x05;
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut frame = [0u8; 512];
    let mut len = 0usize;
    frame[len..len + 4].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    len += 4;
    frame[len] = LEVEL_INFO;
    len += 1;
    frame[len] = SCOPE.len() as u8;
    len += 1;
    frame[len..len + 2].copy_from_slice(&(msg.len() as u16).to_le_bytes());
    len += 2;
    frame[len..len + 2].copy_from_slice(&0u16.to_le_bytes()); // fields_len
    len += 2;
    frame[len..len + SCOPE.len()].copy_from_slice(SCOPE);
    len += SCOPE.len();
    frame[len..len + msg.len()].copy_from_slice(msg);
    len += msg.len();

    let hdr =
        nexus_abi::MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, len as u32);
    let _ = nexus_abi::ipc_send_v1(send_slot, &hdr, &frame[..len], nexus_abi::IPC_SYS_NONBLOCK, 0);
    let _ = _reply_recv_slot;
}
