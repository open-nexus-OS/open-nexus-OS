// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS backend stubs for DSoftBus-lite (gated on OS networking + sockets facade)
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - OsDiscovery: OS discovery backend (stub)
//!   - OsAuthenticator: OS authenticator (stub)
//!   - OsSession: OS session (stub)
//!   - OsStream: OS stream (stub)
//!
//! CONSTRAINTS:
//!   - Stubs must be honest: return Unsupported/Placeholder errors, never “ok”.
//!   - This backend is gated on `tasks/TASK-0010-device-mmio-access-model.md` via `TASK-0003`.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use std::net::SocketAddr;

use identity::Identity;

use crate::{
    Announcement, AuthError, Discovery, DiscoveryError, Session, SessionError, Stream, StreamError,
};

/// Placeholder discovery backend for the OS configuration.
pub struct OsDiscovery;

impl Discovery for OsDiscovery {
    type Error = DiscoveryError;
    type Stream = std::vec::IntoIter<Announcement>;

    fn announce(&self, _announcement: Announcement) -> Result<(), Self::Error> {
        Err(DiscoveryError::Unsupported)
    }

    fn get(&self, _device: &identity::DeviceId) -> Result<Option<Announcement>, Self::Error> {
        Err(DiscoveryError::Unsupported)
    }

    fn watch(&self) -> Result<Self::Stream, Self::Error> {
        Err(DiscoveryError::Unsupported)
    }
}

/// Placeholder authenticator for the OS configuration.
pub struct OsAuthenticator;

impl crate::Authenticator for OsAuthenticator {
    type Session = OsSession;

    fn bind(_addr: SocketAddr, _identity: Identity) -> Result<Self, AuthError>
    where
        Self: Sized,
    {
        Err(AuthError::Unsupported)
    }

    fn accept(&self) -> Result<Self::Session, AuthError> {
        Err(AuthError::Unsupported)
    }

    fn connect(&self, _announcement: &Announcement) -> Result<Self::Session, AuthError> {
        Err(AuthError::Unsupported)
    }
}

/// Placeholder session object for the OS configuration.
pub struct OsSession;

impl Session for OsSession {
    type Stream = OsStream;

    fn remote_device_id(&self) -> &identity::DeviceId {
        panic!("OsSession is a placeholder: remote_device_id() unsupported")
    }

    fn into_stream(self) -> Result<Self::Stream, SessionError> {
        Err(SessionError::Rejected("OS DSoftBus session is unsupported (placeholder)".into()))
    }
}

/// Placeholder stream object for the OS configuration.
pub struct OsStream;

impl Stream for OsStream {
    fn send(&mut self, _channel: u32, _payload: &[u8]) -> Result<(), StreamError> {
        Err(StreamError::Protocol("OS DSoftBus stream is unsupported (placeholder)".into()))
    }

    fn recv(&mut self) -> Result<Option<crate::FramePayload>, StreamError> {
        Err(StreamError::Protocol("OS DSoftBus stream is unsupported (placeholder)".into()))
    }
}
