// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Supervision restart-counter persistence (TASK-0049B PR-B3c,
//! RFC-0087 §2 "counters persist so loops survive reboot"). init bumps a
//! bounded per-service counter at `/state/init/restarts/<svc>` BEFORE
//! resuming a respawned instance (strict ordering: the record is durable
//! before any client can observe the restarted service, so the selftest's
//! read is race-free on the shared response queue). Wire = statefs proto v2
//! with nonces over init's own pre-minted statefsd slots; replies for other
//! nonce owners are never consumed here out of order because the respawn
//! path is the only init-side statefs traffic and it is strictly
//! sequential. Cross-boot AUTO-blocking is deliberately NOT implemented:
//! the in-boot window uses monotonic time, which does not compare across
//! boots — a wall-clock-anchored policy (RFC-0076) is recorded as the
//! follow-up in the 0049B ledger.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU double-boot lane (`SELFTEST: crash-loop persist ok`)
//!   + count assert every boot (`SELFTEST: crash-loop count ok`).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§2)

use crate::bootstrap::helpers::{debug_write_byte, debug_write_bytes, debug_write_hex};
use statefs::protocol as proto;

/// init's statefsd wire (pre-minted pair slots) for supervision persistence.
pub(crate) struct SupervisionPersist {
    send_slot: u32,
    recv_slot: u32,
    nonce: u64,
}

impl SupervisionPersist {
    pub(crate) fn new(slots: Option<(u32, u32)>) -> Option<Self> {
        slots.map(|(send_slot, recv_slot)| Self { send_slot, recv_slot, nonce: 0x4953_0000 })
    }

    /// Reads the persisted restart counter for `svc` (0 when absent).
    pub(crate) fn restart_count(&mut self, svc: &str) -> u32 {
        let mut key = KeyBuf::new(svc);
        let Ok(req) = proto::encode_key_only_request(proto::OP_GET, key.as_str()) else {
            return 0;
        };
        match self.send_recv(&req) {
            Some(rsp) => proto::decode_get_response(&rsp)
                .ok()
                .filter(|v| v.len() == 4)
                .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
                .unwrap_or(0),
            None => 0,
        }
    }

    /// Bumps and durably persists the restart counter for `svc`. Loud on
    /// failure — a supervisor that silently forgets its history would be
    /// the exact silent-degradation class RFC-0087 forbids.
    pub(crate) fn bump_restart_count(&mut self, svc: &str) {
        let next = self.restart_count(svc).saturating_add(1);
        let mut key = KeyBuf::new(svc);
        match self.put_and_sync(key.as_str(), &next.to_le_bytes()) {
            Ok(()) => {
                crate::bootstrap::diag::emit_marker_atomic(
                    &[b"init: supervision persist restarts=0x"],
                    Some(next as u64),
                );
            }
            Err(step) => {
                // Loud + located: the step tag names the first wire stage
                // that failed (encode/roundtrip/status for PUT, then SYNC).
                debug_write_bytes(b"init: FAIL supervision persist step=");
                debug_write_bytes(step.as_bytes());
                debug_write_byte(b'\n');
            }
        }
    }

    fn put_and_sync(&mut self, key: &str, value: &[u8]) -> Result<(), &'static str> {
        let put = proto::encode_put_request(key, value).map_err(|_| "put-encode")?;
        let rsp = self.send_recv(&put).ok_or("put-roundtrip")?;
        let status =
            proto::decode_status_response(proto::OP_PUT, &rsp).map_err(|_| "put-decode")?;
        if status != proto::STATUS_OK {
            debug_write_bytes(b"init: supervision persist put status=0x");
            debug_write_hex(status as usize);
            debug_write_byte(b'\n');
            return Err("put-status");
        }
        let rsp = self.send_recv(&proto::encode_sync_request()).ok_or("sync-roundtrip")?;
        let status =
            proto::decode_status_response(proto::OP_SYNC, &rsp).map_err(|_| "sync-decode")?;
        if status != proto::STATUS_OK {
            return Err("sync-status");
        }
        Ok(())
    }

    /// One nonce-correlated v2 round trip on init's statefsd slots (bounded;
    /// foreign-nonce frames are skipped, never treated as our reply).
    fn send_recv(&mut self, v1_frame: &[u8]) -> Option<heapless_vec::RspBuf> {
        self.nonce = self.nonce.wrapping_add(1);
        let nonce = self.nonce;
        if v1_frame.len() < 4 {
            return None;
        }
        let mut req = [0u8; 160];
        if v1_frame.len() + 8 > req.len() {
            return None;
        }
        req[..4].copy_from_slice(&v1_frame[..4]);
        req[2] = proto::VERSION_V2;
        req[4..12].copy_from_slice(&nonce.to_le_bytes());
        req[12..12 + v1_frame.len() - 4].copy_from_slice(&v1_frame[4..]);
        let req_len = v1_frame.len() + 8;
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
        // Time-based bounds (NOT yield counters): with statefsd busy
        // draining stalled replies, thousands of yields can burn off in
        // microseconds while the store needs real milliseconds — a counter
        // bound starved this exact path in the 0049C bring-up boots.
        let start = nexus_abi::nsec().unwrap_or(0);
        let send_deadline = start.saturating_add(2_000_000_000);
        let mut sent = false;
        loop {
            match nexus_abi::ipc_send_v1(
                self.send_slot,
                &hdr,
                &req[..req_len],
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => {
                    sent = true;
                    break;
                }
                Err(nexus_abi::IpcError::QueueFull) => {
                    if nexus_abi::nsec().unwrap_or(u64::MAX) >= send_deadline {
                        break;
                    }
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return None,
            }
        }
        if !sent {
            debug_write_bytes(b"init: supervision persist send stalled\n");
            return None;
        }
        let recv_deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(2_000_000_000);
        loop {
            if nexus_abi::nsec().unwrap_or(u64::MAX) >= recv_deadline {
                return None;
            }
            let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = heapless_vec::RspBuf::zeroed();
            match nexus_abi::ipc_recv_v1(
                self.recv_slot,
                &mut rh,
                buf.bytes_mut(),
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    buf.set_len(n as usize);
                    let b = buf.bytes();
                    if b.len() >= 13
                        && b[0] == proto::MAGIC0
                        && b[1] == proto::MAGIC1
                        && b[2] == proto::VERSION_V2
                        && u64::from_le_bytes([b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12]])
                            == nonce
                    {
                        return Some(buf);
                    }
                    // Foreign nonce/version: not ours — skip (bounded).
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return None,
            }
        }
    }
}

/// `/state/init/restarts/<svc>` without alloc.
struct KeyBuf {
    buf: [u8; 64],
    len: usize,
}

impl KeyBuf {
    fn new(svc: &str) -> Self {
        const PREFIX: &[u8] = b"/state/init/restarts/";
        let mut buf = [0u8; 64];
        let mut len = 0usize;
        buf[..PREFIX.len()].copy_from_slice(PREFIX);
        len += PREFIX.len();
        let name = svc.as_bytes();
        let take = name.len().min(buf.len() - len);
        buf[len..len + take].copy_from_slice(&name[..take]);
        len += take;
        Self { buf, len }
    }

    fn as_str(&mut self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("/state/init/restarts/invalid")
    }
}

/// Minimal fixed reply buffer (statefs v2 replies for GET(u32)/status fit
/// far below this) — keeps the responder allocation-free.
mod heapless_vec {
    /// Bounded reply frame storage.
    pub(crate) struct RspBuf {
        data: [u8; 96],
        len: usize,
    }

    impl RspBuf {
        pub(crate) fn zeroed() -> Self {
            Self { data: [0; 96], len: 0 }
        }
        pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
            &mut self.data
        }
        pub(crate) fn bytes(&self) -> &[u8] {
            &self.data[..self.len.min(self.data.len())]
        }
        pub(crate) fn set_len(&mut self, len: usize) {
            self.len = len.min(self.data.len());
        }
    }

    impl core::ops::Deref for RspBuf {
        type Target = [u8];
        fn deref(&self) -> &[u8] {
            self.bytes()
        }
    }
}
