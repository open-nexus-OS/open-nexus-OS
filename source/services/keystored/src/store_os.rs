// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: keystored (os-lite) persistence backends — statefsd-backed store
//!   with Integrity envelopes (TASK-0025 step 4) and the memory fallback
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal; moved out of os_stub.rs)
//! TEST_COVERAGE: Codec proofs in tests/state_record_contract.rs (host);
//!   end-to-end via QEMU device-key persist ladder
//!
//! WRITE PATH (Integrity floor, `/state/keystore/*`): every put is a
//! read-modify-write — read the stored record, learn its seq (legacy raw or
//! missing = none), seal with seq = last_seen + 1 (`keystored/state_record`
//! meta), put. One bounded retry on a rollback race; statefsd's replay-fed
//! tracker stays authoritative. Reads accept envelope v1 AND pre-migration
//! legacy raw bytes (deterministic, no panic).
//!
//! SECURITY INVARIANTS:
//! - Never log entropy bytes or private key material
//! - Keys scoped to the kernel-provided `sender_service_id`
//!
//! ADR: docs/adr/0017-service-architecture.md

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;
use statefs::client::StatefsClient;
use statefs::StatefsError;

use crate::os_stub::emit_line;
use crate::state_record;

pub(crate) const MAX_KEY_LEN: usize = 64;

const STATEFS_KEY_PREFIX: &str = "/state/keystore/";

pub(crate) enum KeyStoreBackend {
    Statefs(StatefsStore),
    Memory(BTreeMap<(u64, Vec<u8>), Vec<u8>>),
}

pub(crate) struct KeyStore {
    backend: KeyStoreBackend,
    device_key_bytes: Option<[u8; 32]>,
}

impl KeyStore {
    pub(crate) fn new() -> Self {
        if let Some(mut store) = StatefsStore::new() {
            emit_line("keystored: statefs backend ok");
            let device_key_bytes = store.load_device_key().ok().flatten();
            return Self { backend: KeyStoreBackend::Statefs(store), device_key_bytes };
        }
        emit_line("keystored: memory backend fallback");
        Self { backend: KeyStoreBackend::Memory(BTreeMap::new()), device_key_bytes: None }
    }

    #[cfg(test)]
    pub(crate) fn new_memory() -> Self {
        Self { backend: KeyStoreBackend::Memory(BTreeMap::new()), device_key_bytes: None }
    }

    pub(crate) fn get(
        &mut self,
        sender_service_id: u64,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => match store.get(sender_service_id, key) {
                Ok(value) => Ok(value),
                Err(StatefsError::AccessDenied) => {
                    self.fallback_to_memory_backend();
                    self.get(sender_service_id, key)
                }
                Err(err) => Err(err),
            },
            KeyStoreBackend::Memory(map) => {
                Ok(map.get(&(sender_service_id, key.to_vec())).cloned())
            }
        }
    }

    pub(crate) fn put(
        &mut self,
        sender_service_id: u64,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => match store.put(sender_service_id, key, value) {
                Ok(()) => Ok(()),
                Err(StatefsError::AccessDenied) => {
                    self.fallback_to_memory_backend();
                    self.put(sender_service_id, key, value)
                }
                Err(err) => Err(err),
            },
            KeyStoreBackend::Memory(map) => {
                map.insert((sender_service_id, key.to_vec()), value.to_vec());
                Ok(())
            }
        }
    }

    pub(crate) fn delete(
        &mut self,
        sender_service_id: u64,
        key: &[u8],
    ) -> Result<bool, StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => match store.delete(sender_service_id, key) {
                Ok(deleted) => Ok(deleted),
                Err(StatefsError::AccessDenied) => {
                    self.fallback_to_memory_backend();
                    self.delete(sender_service_id, key)
                }
                Err(err) => Err(err),
            },
            KeyStoreBackend::Memory(map) => {
                Ok(map.remove(&(sender_service_id, key.to_vec())).is_some())
            }
        }
    }

    pub(crate) fn device_key_bytes(&self) -> Option<[u8; 32]> {
        self.device_key_bytes
    }

    /// Reload device key from statefsd (for persistence proof after reboot).
    pub(crate) fn reload_device_key(&mut self) -> Result<Option<[u8; 32]>, StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => match store.load_device_key() {
                Ok(Some(bytes)) => {
                    emit_line("keystored: reload from statefs ok");
                    self.device_key_bytes = Some(bytes);
                    Ok(Some(bytes))
                }
                Ok(None) => {
                    emit_line("keystored: reload from statefs (not found)");
                    Ok(None)
                }
                Err(err) => {
                    emit_line("keystored: reload from statefs err");
                    Err(err)
                }
            },
            KeyStoreBackend::Memory(_) => {
                emit_line("keystored: reload from memory backend");
                Ok(self.device_key_bytes)
            }
        }
    }

    pub(crate) fn set_device_key_bytes(&mut self, bytes: [u8; 32]) -> Result<(), StatefsError> {
        if let KeyStoreBackend::Statefs(store) = &mut self.backend {
            if let Err(err) = store.store_device_key(&bytes) {
                if err == StatefsError::AccessDenied {
                    self.fallback_to_memory_backend();
                } else {
                    return Err(err);
                }
            }
        }
        self.device_key_bytes = Some(bytes);
        Ok(())
    }

    fn fallback_to_memory_backend(&mut self) {
        if !matches!(self.backend, KeyStoreBackend::Memory(_)) {
            emit_line("keystored: statefs access denied fallback");
            self.backend = KeyStoreBackend::Memory(BTreeMap::new());
        }
    }
}

pub(crate) struct StatefsStore {
    client: StatefsClient,
    /// Last seq this writer wrote per path. Statefsd's tracker keeps the
    /// high-water mark across DELETEs, so a re-put must not re-learn its seq
    /// from the (now missing) stored value alone — that sealed seq 1 forever
    /// and every re-put of a deleted scoped key was denied as a rollback
    /// (QEMU regression 2026-08-15: `envelope deny …/6b31 status=10` loop).
    seq_cache: statefs::writer::SeqCache,
}

impl StatefsStore {
    fn new() -> Option<Self> {
        // init-lite deterministic slots for keystored -> statefsd:
        // - send=0x07, reply recv=0x05, reply send=0x06
        const STATEFS_SEND_SLOT: u32 = 0x07;
        const REPLY_RECV_SLOT: u32 = 0x05;
        const REPLY_SEND_SLOT: u32 = 0x06;
        let client = KernelClient::new_with_slots(STATEFS_SEND_SLOT, REPLY_RECV_SLOT).ok()?;
        let reply = KernelClient::new_with_slots(REPLY_SEND_SLOT, REPLY_RECV_SLOT).ok();
        let client = StatefsClient::from_clients(client, reply);
        Some(Self { client, seq_cache: statefs::writer::SeqCache::new() })
    }

    fn get(&mut self, sender_service_id: u64, key: &[u8]) -> Result<Option<Vec<u8>>, StatefsError> {
        let path = self.key_path(sender_service_id, key)?;
        match self.client.get(&path) {
            Ok(value) => {
                // Envelope v1 or legacy raw bytes — both stay readable.
                let (payload, _seq) = state_record::open_scoped(&value)?;
                Ok(Some(payload.to_vec()))
            }
            Err(StatefsError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn put(
        &mut self,
        sender_service_id: u64,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StatefsError> {
        let path = self.key_path(sender_service_id, key)?;
        self.put_integrity(&path, state_record::PURPOSE_SCOPED, value)
    }

    fn delete(&mut self, sender_service_id: u64, key: &[u8]) -> Result<bool, StatefsError> {
        let path = self.key_path(sender_service_id, key)?;
        match self.client.delete(&path) {
            Ok(()) => Ok(true),
            Err(StatefsError::NotFound) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn load_device_key(&mut self) -> Result<Option<[u8; 32]>, StatefsError> {
        match self.client.get(state_record::DEVICE_KEY_PATH) {
            Ok(bytes) => {
                let (seed, _seq) = state_record::open_device_key(&bytes)?;
                Ok(Some(seed))
            }
            Err(StatefsError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn store_device_key(&mut self, key_bytes: &[u8; 32]) -> Result<(), StatefsError> {
        self.put_integrity(
            state_record::DEVICE_KEY_PATH,
            state_record::PURPOSE_DEVICE_KEY,
            key_bytes,
        )
    }

    /// Read-modify-write an Integrity-class record (see module header):
    /// seq = max(stored envelope seq, last seq we wrote) + 1. The cache leg
    /// is what keeps delete -> re-put monotonic (a DELETE removes the stored
    /// value but never statefsd's high-water mark).
    fn put_integrity(
        &mut self,
        path: &str,
        purpose: &str,
        value: &[u8],
    ) -> Result<(), StatefsError> {
        let mut retried = false;
        loop {
            let stored = match self.client.get(path) {
                Ok(bytes) => statefs::writer::open_stored(&bytes)?.seq(),
                Err(StatefsError::NotFound) => None,
                Err(err) => return Err(err),
            };
            let seq = self.seq_cache.next_for(path, stored);
            let ts = nexus_abi::nsec().unwrap_or(0);
            let sealed = statefs::writer::seal_integrity(
                path,
                seq,
                state_record::SUBJECT,
                purpose,
                ts,
                value,
            )?;
            match self.client.put(path, &sealed) {
                Ok(()) => {
                    self.seq_cache.note_written(path, seq);
                    return Ok(());
                }
                Err(StatefsError::RollbackDetected) if !retried => {
                    // Raced statefsd's high-water mark: re-read once, re-seal.
                    retried = true;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn key_path(&self, sender_service_id: u64, key: &[u8]) -> Result<String, StatefsError> {
        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return Err(StatefsError::KeyTooLong);
        }
        let mut path = String::with_capacity(STATEFS_KEY_PREFIX.len() + 16 + 1 + key.len() * 2);
        path.push_str(STATEFS_KEY_PREFIX);
        push_hex_u64(&mut path, sender_service_id);
        path.push('/');
        push_hex_bytes(&mut path, key);
        if path.len() > statefs::MAX_KEY_LEN {
            return Err(StatefsError::KeyTooLong);
        }
        Ok(path)
    }
}

fn push_hex_u64(out: &mut String, value: u64) {
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        out.push(ch as char);
    }
}

fn push_hex_bytes(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        let high = (byte >> 4) & 0xF;
        let low = byte & 0xF;
        out.push(if high < 10 { (b'0' + high) as char } else { (b'a' + (high - 10)) as char });
        out.push(if low < 10 { (b'0' + low) as char } else { (b'a' + (low - 10)) as char });
    }
}
