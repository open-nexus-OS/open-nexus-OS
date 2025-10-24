//! CONTEXT: OS backend stubs for DSoftBus-lite
//! INTENT: Placeholder implementations pending kernel transport integration
//! IDL (target): announce(), bind(), accept(), connect(), send(), recv()
//! DEPS: identity (device keys)
//! READINESS: OS backend not ready; requires kernel transport
//! TESTS: Compilation only; functionality pending kernel integration
//!
//! The kernel transport will expose discovery and authenticated sessions in a
//! future change. For now the OS build provides placeholders so userland daemons
//! can compile without pulling in host-only TCP dependencies.

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
        todo!("OS discovery backend pending kernel transport integration")
    }

    fn get(&self, _device: &identity::DeviceId) -> Result<Option<Announcement>, Self::Error> {
        todo!("OS discovery lookup pending kernel transport integration")
    }

    fn watch(&self) -> Result<Self::Stream, Self::Error> {
        todo!("OS discovery stream pending kernel transport integration")
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
        todo!("OS authenticator pending kernel transport integration")
    }

    fn accept(&self) -> Result<Self::Session, AuthError> {
        todo!("OS authenticator accept pending kernel transport integration")
    }

    fn connect(&self, _announcement: &Announcement) -> Result<Self::Session, AuthError> {
        todo!("OS authenticator connect pending kernel transport integration")
    }
}

/// Placeholder session object for the OS configuration.
pub struct OsSession;

impl Session for OsSession {
    type Stream = OsStream;

    fn remote_device_id(&self) -> &identity::DeviceId {
        todo!("OS session remote identity pending kernel transport integration")
    }

    fn into_stream(self) -> Result<Self::Stream, SessionError> {
        todo!("OS session stream pending kernel transport integration")
    }
}

/// Placeholder stream object for the OS configuration.
pub struct OsStream;

impl Stream for OsStream {
    fn send(&mut self, _channel: u32, _payload: &[u8]) -> Result<(), StreamError> {
        todo!("OS stream send pending kernel transport integration")
    }

    fn recv(&mut self) -> Result<Option<crate::FramePayload>, StreamError> {
        todo!("OS stream recv pending kernel transport integration")
    }
}
