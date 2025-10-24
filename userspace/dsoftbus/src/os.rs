//! CONTEXT: OS backend implementation for DSoftBus-lite distributed service fabric
//!
//! OWNERS: @runtime
//!
//! PUBLIC API:
//!   - struct OsDiscovery: Placeholder discovery backend for OS configuration
//!   - struct OsAuthenticator: Placeholder authenticator for OS configuration
//!   - struct OsSession: Placeholder session object for OS configuration
//!   - struct OsStream: Placeholder stream object for OS configuration
//!
//! IMPLEMENTATION STATUS:
//!   - All methods return `todo!()` macros
//!   - Pending kernel transport integration
//!   - Provides compilation compatibility for OS builds
//!   - No functional implementation until kernel networking is available
//!
//! SECURITY INVARIANTS:
//!   - No unsafe code in placeholder implementations
//!   - All methods will be implemented with proper security validation
//!   - Kernel transport will enforce authentication and encryption
//!
//! ERROR CONDITIONS:
//!   - All operations currently panic with `todo!()` macros
//!   - Future implementation will provide proper error handling
//!   - Kernel transport errors will be mapped to appropriate error types
//!
//! DEPENDENCIES:
//!   - identity: Device identity and signing support
//!   - std::net::SocketAddr: Network address types
//!
//! FEATURES:
//!   - Compilation compatibility for OS builds
//!   - Placeholder implementations for all DSoftBus interfaces
//!   - Future kernel transport integration
//!
//! TEST SCENARIOS:
//!   - test_compilation(): Verify OS backend compiles without errors
//!   - test_placeholder_behavior(): Verify placeholder methods panic as expected
//!   - test_kernel_integration(): Future tests for kernel transport integration
//!   - test_discovery_operations(): Future tests for OS discovery backend
//!   - test_authentication_flow(): Future tests for OS authentication
//!   - test_session_management(): Future tests for OS session handling
//!   - test_stream_operations(): Future tests for OS stream operations
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
