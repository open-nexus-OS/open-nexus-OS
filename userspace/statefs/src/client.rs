// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS IPC client wrapper (feature = "ipc-client", nexus_env = "os")
//! OWNERS: @runtime
//! STATUS: Functional (OS path)
//! API_STABILITY: Stable (v1.0) — public path is `statefs::client`, do not move
//! TEST_COVERAGE: Exercised via QEMU selftests (statefs persist ladder)
//!
//! PUBLIC API:
//!   - StatefsClient: put/get/delete/list/sync against statefsd over kernel IPC
//!
//! Moved verbatim out of lib.rs (structure ratchet); behavior and API are
//! unchanged. Nonce correlation for shared reply inboxes follows RFC-0019.
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use alloc::string::String;
use alloc::vec::Vec;

use super::protocol;
use super::StatefsError;
use nexus_abi;
use nexus_ipc::KernelClient;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use nexus_ipc::Wait;

/// Client for statefsd IPC operations.
pub struct StatefsClient {
    client: KernelClient,
    reply: Option<KernelClient>,
}

impl StatefsClient {
    /// Create a new client targeting `statefsd`.
    pub fn new() -> Result<Self, StatefsError> {
        let client = KernelClient::new_for("statefsd").map_err(|_| StatefsError::IoError)?;
        let reply = KernelClient::new_for("@reply").ok();
        Ok(Self { client, reply })
    }

    /// Create a new client from pre-routed kernel IPC endpoints.
    pub fn from_clients(client: KernelClient, reply: Option<KernelClient>) -> Self {
        Self { client, reply }
    }

    /// Put a value into statefs.
    pub fn put(&self, key: &str, value: &[u8]) -> Result<(), StatefsError> {
        let frame = protocol::encode_put_request(key, value)?;
        self.send_and_recv(frame, protocol::OP_PUT)?;
        Ok(())
    }

    /// Get a value from statefs.
    pub fn get(&self, key: &str) -> Result<Vec<u8>, StatefsError> {
        let frame = protocol::encode_key_only_request(protocol::OP_GET, key)?;
        let rsp = self.send_and_recv_raw(frame, protocol::OP_GET)?;
        protocol::decode_get_response(&rsp)
    }

    /// Delete a key.
    pub fn delete(&self, key: &str) -> Result<(), StatefsError> {
        let frame = protocol::encode_key_only_request(protocol::OP_DEL, key)?;
        self.send_and_recv(frame, protocol::OP_DEL)?;
        Ok(())
    }

    /// List keys by prefix.
    pub fn list(&self, prefix: &str, limit: u16) -> Result<Vec<String>, StatefsError> {
        let frame = protocol::encode_list_request(prefix, limit)?;
        let rsp = self.send_and_recv_raw(frame, protocol::OP_LIST)?;
        protocol::decode_list_response(&rsp)
    }

    /// Sync statefs.
    pub fn sync(&self) -> Result<(), StatefsError> {
        let frame = protocol::encode_sync_request();
        self.send_and_recv(frame, protocol::OP_SYNC)?;
        Ok(())
    }

    fn send_and_recv(&self, frame: Vec<u8>, op: u8) -> Result<(), StatefsError> {
        let rsp = self.send_and_recv_raw(frame, op)?;
        let status = protocol::decode_status_response(op, &rsp)?;
        if status == protocol::STATUS_OK {
            Ok(())
        } else {
            Err(protocol::error_from_status(status))
        }
    }

    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    fn send_and_recv_raw(&self, frame: Vec<u8>, expected_op: u8) -> Result<Vec<u8>, StatefsError> {
        // OS-lite bring-up: avoid indefinite blocking waits.
        // Use explicit NONBLOCK + bounded retry with `nsec()` deadlines.
        let (send_slot, recv_slot) = if let Some(reply) = &self.reply {
            // Replies land on the shared reply inbox when using CAP_MOVE.
            let (_reply_send, reply_recv) = reply.slots();
            (self.client.slots().0, reply_recv)
        } else {
            self.client.slots()
        };

        let moved = if let Some(reply) = &self.reply {
            let (reply_send_slot, _reply_recv_slot) = reply.slots();
            nexus_abi::cap_clone(reply_send_slot).map_err(|_| StatefsError::IoError)?
        } else {
            0
        };
        let flags = if moved != 0 { nexus_abi::ipc_hdr::CAP_MOVE } else { 0 };
        // Nonce correlation for shared reply inboxes (RFC-0019):
        // upgrade requests to SF v2 (explicit nonce field) and require it in the reply.
        static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let mut frame = frame;
        // Upgrade v1 request frame to v2 by inserting nonce after the 4-byte header.
        if frame.len() < 4
            || frame[0] != protocol::MAGIC0
            || frame[1] != protocol::MAGIC1
            || frame[2] != protocol::VERSION
        {
            return Err(StatefsError::IoError);
        }
        let mut v2 = Vec::with_capacity(frame.len() + 8);
        v2.extend_from_slice(&frame[..4]);
        v2[2] = protocol::VERSION_V2;
        v2.extend_from_slice(&nonce.to_le_bytes());
        v2.extend_from_slice(&frame[4..]);
        frame = v2;
        let hdr = nexus_abi::MsgHeader::new(moved, 0, 0, flags, frame.len() as u32);

        let start = nexus_abi::nsec().map_err(|_| StatefsError::IoError)?;
        let deadline = start.saturating_add(2_000_000_000); // 2s per op (bounded)

        // Send bounded.
        let mut i: usize = 0;
        loop {
            match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    if (i & 0x7f) == 0 {
                        let now = nexus_abi::nsec().map_err(|_| StatefsError::IoError)?;
                        if now >= deadline {
                            return Err(StatefsError::IoError);
                        }
                    }
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return Err(StatefsError::IoError),
            }
            i = i.wrapping_add(1);
        }

        // Recv bounded.
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 4096];
        let mut j: usize = 0;
        loop {
            if (j & 0x7f) == 0 {
                let now = nexus_abi::nsec().map_err(|_| StatefsError::IoError)?;
                if now >= deadline {
                    return Err(StatefsError::IoError);
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
                    // Shared reply inbox: ignore unrelated replies deterministically.
                    if n < 13
                        || buf[0] != protocol::MAGIC0
                        || buf[1] != protocol::MAGIC1
                        || buf[2] != protocol::VERSION_V2
                        || buf[3] != (expected_op | 0x80)
                    {
                        continue;
                    }
                    // Nonce must match.
                    let nn = &buf[5..13];
                    let mut want = [0u8; 8];
                    want.copy_from_slice(&nonce.to_le_bytes());
                    if nn != want {
                        continue;
                    }
                    return Ok(buf[..n].to_vec());
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return Err(StatefsError::IoError),
            }
            j = j.wrapping_add(1);
        }
    }

    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    fn send_and_recv_raw(&self, frame: Vec<u8>, _expected_op: u8) -> Result<Vec<u8>, StatefsError> {
        if let Some(reply) = &self.reply {
            let (reply_send_slot, _reply_recv_slot) = reply.slots();
            let reply_send_clone =
                nexus_abi::cap_clone(reply_send_slot).map_err(|_| StatefsError::IoError)?;
            self.client
                .send_with_cap_move_wait(&frame, reply_send_clone, Wait::Blocking)
                .map_err(|_| StatefsError::IoError)?;
            nexus_ipc::Client::recv(reply, Wait::Blocking).map_err(|_| StatefsError::IoError)
        } else {
            nexus_ipc::Client::send(&self.client, &frame, Wait::Blocking)
                .map_err(|_| StatefsError::IoError)?;
            nexus_ipc::Client::recv(&self.client, Wait::Blocking).map_err(|_| StatefsError::IoError)
        }
    }
}
