// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Userland IDL runtime: Cap'n Proto glue for control-plane messaging
//! OWNERS: @runtime
//! PUBLIC API: generated *_capnp modules, IdlError
//! DEPENDS_ON: capnp build outputs (OUT_DIR)
//! INVARIANTS: No unsafe; only exposes generated bindings; stable module names
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
#![forbid(unsafe_code)]

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod samgr_capnp {
    include!(concat!(env!("OUT_DIR"), "/samgr_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod bundlemgr_capnp {
    include!(concat!(env!("OUT_DIR"), "/bundlemgr_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod vfs_capnp {
    include!(concat!(env!("OUT_DIR"), "/vfs_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod packagefs_capnp {
    include!(concat!(env!("OUT_DIR"), "/packagefs_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod keystored_capnp {
    include!(concat!(env!("OUT_DIR"), "/keystored_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod identity_capnp {
    include!(concat!(env!("OUT_DIR"), "/identity_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod dsoftbus_capnp {
    include!(concat!(env!("OUT_DIR"), "/dsoftbus_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub mod policyd_capnp {
    include!(concat!(env!("OUT_DIR"), "/policyd_capnp.rs"));
}

#[cfg(feature = "capnp")]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
