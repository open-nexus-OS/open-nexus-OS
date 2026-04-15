// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: no_std + alloc transport-neutral core contract helpers for TASK-0022
//! OWNERS: @runtime
//! STATUS: Functional (phase-1 borrow-view contract + reject invariants; handle-first bulk path remains follow-up scope)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: userspace/dsoftbus/tests/core_contract_rejects.rs (8 host tests)
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

pub const REJECT_INVALID_STATE_TRANSITION: &str = "core.reject.invalid_state_transition";
pub const REJECT_NONCE_MISMATCH_OR_STALE_REPLY: &str = "core.reject.nonce_mismatch_or_stale_reply";
pub const REJECT_OVERSIZE_FRAME_OR_RECORD: &str = "core.reject.oversize_frame_or_record";
pub const REJECT_UNAUTHENTICATED_MESSAGE_PATH: &str = "core.reject.unauthenticated_message_path";
pub const REJECT_PAYLOAD_IDENTITY_SPOOF_VS_SENDER_SERVICE_ID: &str =
    "core.reject.payload_identity_spoof_vs_sender_service_id";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreReject {
    label: &'static str,
}

impl CoreReject {
    const fn new(label: &'static str) -> Self {
        Self { label }
    }

    pub const fn label(self) -> &'static str {
        self.label
    }
}

impl fmt::Display for CoreReject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CorrelationNonce(u64);

impl CorrelationNonce {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SenderServiceId<'a>(&'a str);

impl<'a> SenderServiceId<'a> {
    pub const fn new(raw: &'a str) -> Self {
        Self(raw)
    }

    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PayloadIdentityClaim<'a>(&'a str);

impl<'a> PayloadIdentityClaim<'a> {
    pub const fn new(raw: &'a str) -> Self {
        Self(raw)
    }

    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

#[must_use]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedRecord {
    channel: u32,
    bytes: Vec<u8>,
}

impl OwnedRecord {
    pub fn new(channel: u32, bytes: Vec<u8>) -> Self {
        Self { channel, bytes }
    }

    pub const fn channel(&self) -> u32 {
        self.channel
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn borrow(&self) -> BorrowedRecord<'_> {
        BorrowedRecord {
            channel: self.channel,
            bytes: self.bytes.as_slice(),
        }
    }
}

#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BorrowedRecord<'a> {
    channel: u32,
    bytes: &'a [u8],
}

impl<'a> BorrowedRecord<'a> {
    pub const fn channel(&self) -> u32 {
        self.channel
    }

    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

pub trait BorrowedFrameTransport {
    type Error;

    fn send_record(&mut self, channel: u32, payload: &[u8]) -> Result<(), Self::Error>;
    fn recv_record(&mut self) -> Result<Option<OwnedRecord>, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CorrelationWindow {
    authenticated: bool,
    next_outbound_nonce: CorrelationNonce,
    last_accepted_reply: Option<CorrelationNonce>,
}

impl CorrelationWindow {
    pub const fn new_unauthenticated(initial_nonce: u64) -> Self {
        Self {
            authenticated: false,
            next_outbound_nonce: CorrelationNonce::new(initial_nonce),
            last_accepted_reply: None,
        }
    }

    pub const fn new_authenticated(initial_nonce: u64) -> Self {
        Self {
            authenticated: true,
            next_outbound_nonce: CorrelationNonce::new(initial_nonce),
            last_accepted_reply: None,
        }
    }

    pub fn mark_authenticated(&mut self) {
        self.authenticated = true;
    }

    pub fn reserve_outbound_nonce(&mut self) -> Result<CorrelationNonce, CoreReject> {
        self.require_authenticated()?;
        let current = self.next_outbound_nonce;
        self.next_outbound_nonce = CorrelationNonce::new(current.get().wrapping_add(1));
        Ok(current)
    }

    pub fn validate_inbound_reply(
        &mut self,
        expected: CorrelationNonce,
        observed: CorrelationNonce,
    ) -> Result<(), CoreReject> {
        self.require_authenticated()?;
        if observed != expected {
            return Err(CoreReject::new(REJECT_NONCE_MISMATCH_OR_STALE_REPLY));
        }
        if let Some(last) = self.last_accepted_reply {
            if observed.get() <= last.get() {
                return Err(CoreReject::new(REJECT_NONCE_MISMATCH_OR_STALE_REPLY));
            }
        }
        self.last_accepted_reply = Some(observed);
        Ok(())
    }

    fn require_authenticated(&self) -> Result<(), CoreReject> {
        if self.authenticated {
            Ok(())
        } else {
            Err(CoreReject::new(REJECT_UNAUTHENTICATED_MESSAGE_PATH))
        }
    }
}

pub fn reject_invalid_state_transition() -> Result<(), CoreReject> {
    Err(CoreReject::new(REJECT_INVALID_STATE_TRANSITION))
}

pub fn validate_record_bounds(record_len: usize, max_record_len: usize) -> Result<(), CoreReject> {
    if record_len > max_record_len {
        Err(CoreReject::new(REJECT_OVERSIZE_FRAME_OR_RECORD))
    } else {
        Ok(())
    }
}

pub fn validate_payload_identity_spoof_vs_sender_service_id(
    sender_service_id: SenderServiceId<'_>,
    payload_identity: PayloadIdentityClaim<'_>,
) -> Result<(), CoreReject> {
    if sender_service_id.as_str() == payload_identity.as_str() {
        Ok(())
    } else {
        Err(CoreReject::new(
            REJECT_PAYLOAD_IDENTITY_SPOOF_VS_SENDER_SERVICE_ID,
        ))
    }
}
