// Copyright 2025 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IDL runtime providing Cap'n Proto bindings for control-plane messaging
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - Generated modules: samgr_capnp, bundlemgr_capnp, etc.
//!   - IdlError: Serialization error types
//!
//! DEPENDENCIES:
//!   - capnp: Cap'n Proto serialization
//!   - Generated code from .capnp schemas
//!
//! ADR: docs/adr/0004-idl-runtime-architecture.md
#![forbid(unsafe_code)]

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod samgr_capnp {
    include!(concat!(env!("OUT_DIR"), "/samgr_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod bundlemgr_capnp {
    include!(concat!(env!("OUT_DIR"), "/bundlemgr_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod vfs_capnp {
    include!(concat!(env!("OUT_DIR"), "/vfs_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod packagefs_capnp {
    include!(concat!(env!("OUT_DIR"), "/packagefs_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod keystored_capnp {
    include!(concat!(env!("OUT_DIR"), "/keystored_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod identity_capnp {
    include!(concat!(env!("OUT_DIR"), "/identity_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod dsoftbus_capnp {
    include!(concat!(env!("OUT_DIR"), "/dsoftbus_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod policyd_capnp {
    include!(concat!(env!("OUT_DIR"), "/policyd_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::needless_lifetimes)]
pub mod execd_capnp {
    include!(concat!(env!("OUT_DIR"), "/execd_capnp.rs"));
}

/// Common error type for IDL encode/decode boundaries.
#[derive(Debug)]
pub enum IdlError {
    Encode,
    Decode,
    Io(std::io::Error),
}

impl From<std::io::Error> for IdlError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
