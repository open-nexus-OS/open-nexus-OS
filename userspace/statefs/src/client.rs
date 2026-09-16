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
// The OS target has exactly ONE client path: the nonce-filtered one below.
// A service that enabled `ipc-client` but not `os-lite` silently compiled the
// host-style blocking path, which returns whatever frame arrives on the reply
// slot — keystored did (TASK-0324 P0): a stale frame after a slow PUT decoded
// as `Corrupted` and failed the device-key persist about once in ten boots.
#[cfg(all(nexus_env = "os", not(feature = "os-lite")))]
compile_error!("statefs: the OS build needs the `os-lite` feature (nonce-filtered reply path)");
// That refusal made the second, host-style `send_and_recv_raw` arm unreachable in every
// configuration this tree builds — it could only be selected where the line above already
// fails the build. TASK-0054C P2-c deleted it: ONE client path, not one plus a trap.

/// Client for statefsd IPC operations.
/// How long a client keeps retrying a quiesced store (fsck windows are short).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
const BUSY_RETRY_BUDGET_NS: u64 = 2_000_000_000;

pub struct StatefsClient {
    client: KernelClient,
    /// The caller's reply inbox. NOT optional (TASK-0054C P2-e): a statefs client without one
    /// used to fall back to statefsd's shared response endpoint, and that branch was dead —
    /// both `new()` callers run inside execd, which always has an `@reply` inbox. An
    /// unresolvable inbox is now a construction failure, so the fallback cannot come back.
    reply: KernelClient,
}

impl StatefsClient {
    /// Create a new client targeting `statefsd`. Fails when this service has no `@reply`
    /// inbox: without one there is nowhere for statefsd to answer that only this caller reads.
    pub fn new() -> Result<Self, StatefsError> {
        let client = KernelClient::new_for("statefsd").map_err(|_| StatefsError::IoError)?;
        let reply = KernelClient::new_for("@reply").map_err(|_| StatefsError::IoError)?;
        Ok(Self { client, reply })
    }

    /// Create a new client from pre-routed kernel IPC endpoints.
    pub fn from_clients(client: KernelClient, reply: KernelClient) -> Self {
        Self { client, reply }
    }

    /// Put a value into statefs.
    pub fn put(&self, key: &str, value: &[u8]) -> Result<(), StatefsError> {
        self.put_status(key, value).map_err(|(err, _)| err)
    }

    /// Put, returning the raw wire status on refusal (`Err((err, status))`)
    /// — a diagnostic-grade variant for callers that must name WHICH status
    /// the store answered (the error enum folds several statuses together).
    pub fn put_status(&self, key: &str, value: &[u8]) -> Result<(), (StatefsError, u8)> {
        let frame = protocol::encode_put_request(key, value).map_err(|e| (e, 0xff))?;
        self.send_and_recv_status(frame, protocol::OP_PUT)
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

    /// Begin a journal-v2 transaction (TASK-0026 wire ops; TASK-0049C
    /// exposes them on the shared client so multi-key writers get
    /// both-or-neither without hand-rolled frames).
    pub fn txn_begin(&self) -> Result<u64, StatefsError> {
        let rsp = self.send_and_recv_raw(
            protocol::txn::encode_txn_begin_request(),
            protocol::txn::OP_TXN_BEGIN,
        )?;
        let (status, txn_id) = protocol::txn::decode_txn_begin_response(&rsp)?;
        if status == protocol::STATUS_OK {
            Ok(txn_id)
        } else {
            Err(protocol::error_from_status(status))
        }
    }

    /// Stage one key write inside an open transaction.
    pub fn txn_put(&self, txn_id: u64, key: &str, value: &[u8]) -> Result<(), StatefsError> {
        let frame = protocol::txn::encode_txn_put_request(txn_id, key, value)?;
        self.send_and_recv(frame, protocol::txn::OP_TXN_PUT)
    }

    /// Commit: all staged writes become visible atomically (2PC replay
    /// drops a torn transaction whole — proven TASK-0026 semantics).
    pub fn txn_commit(&self, txn_id: u64) -> Result<(), StatefsError> {
        self.send_and_recv(
            protocol::txn::encode_txn_commit_request(txn_id),
            protocol::txn::OP_TXN_COMMIT,
        )
    }

    /// Abort: discard all staged writes.
    pub fn txn_abort(&self, txn_id: u64) -> Result<(), StatefsError> {
        self.send_and_recv(
            protocol::txn::encode_txn_abort_request(txn_id),
            protocol::txn::OP_TXN_ABORT,
        )
    }

    fn send_and_recv(&self, frame: Vec<u8>, op: u8) -> Result<(), StatefsError> {
        self.send_and_recv_status(frame, op).map_err(|(err, _)| err)
    }

    /// Like [`Self::send_and_recv`] but keeps the raw refusal status.
    fn send_and_recv_status(&self, frame: Vec<u8>, op: u8) -> Result<(), (StatefsError, u8)> {
        // `STATUS_BUSY` = the store is quiesced (fsck window, TASK-0051):
        // retry the whole exchange with yields inside ONE bounded op budget
        // instead of surfacing a transient as an error to every writer.
        #[cfg(all(nexus_env = "os", feature = "os-lite"))]
        let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(BUSY_RETRY_BUDGET_NS);
        loop {
            let rsp = self.send_and_recv_raw(frame.clone(), op).map_err(|e| (e, 0xfe))?;
            let status = protocol::decode_status_response(op, &rsp).map_err(|e| (e, 0xfd))?;
            if status == protocol::STATUS_OK {
                return Ok(());
            }
            #[cfg(all(nexus_env = "os", feature = "os-lite"))]
            if status == protocol::STATUS_BUSY && nexus_abi::nsec().unwrap_or(u64::MAX) < deadline {
                let _ = nexus_abi::yield_();
                continue;
            }
            return Err((protocol::error_from_status(status), status));
        }
    }

    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    fn send_and_recv_raw(&self, frame: Vec<u8>, expected_op: u8) -> Result<Vec<u8>, StatefsError> {
        // Nonce correlation for shared reply inboxes (RFC-0019): upgrade requests to SF v2
        // (explicit nonce field) and require it in the reply.
        static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if frame.len() < 4
            || frame[0] != protocol::MAGIC0
            || frame[1] != protocol::MAGIC1
            || frame[2] != protocol::VERSION
        {
            return Err(StatefsError::IoError);
        }
        // Upgrade v1 request frame to v2 by inserting nonce after the 4-byte header.
        let mut v2 = Vec::with_capacity(frame.len() + 8);
        v2.extend_from_slice(&frame[..4]);
        v2[2] = protocol::VERSION_V2;
        v2.extend_from_slice(&nonce.to_le_bytes());
        v2.extend_from_slice(&frame[4..]);

        // The answer carries this op AND this nonce; anything else on the inbox belongs to
        // another exchange and is dropped (TASK-0054C P2-d).
        let matches = |rsp: &[u8]| -> Option<usize> {
            (rsp.len() >= 13
                && rsp[0] == protocol::MAGIC0
                && rsp[1] == protocol::MAGIC1
                && rsp[2] == protocol::VERSION_V2
                && rsp[3] == (expected_op | 0x80)
                && rsp[5..13] == nonce.to_le_bytes())
            .then(|| rsp.len())
        };
        let mut buf = [0u8; 4096];
        // No clock (TASK-0324 P7-d): queue space, then statefsd's answer (or its death). The
        // request moves a SEND clone of OUR inbox and the wait is EOF-opted, so a statefsd that
        // dies mid-exchange wakes us.
        let (reply_send, reply_recv) = self.reply.slots();
        let n = nexus_ipc::exchange::call_matching(
            self.client.slots().0,
            nexus_ipc::SlotPair::new(reply_send, reply_recv),
            &v2,
            &mut buf,
            matches,
        )
        .map_err(|_| StatefsError::IoError)?;
        Ok(buf[..n].to_vec())
    }
}
