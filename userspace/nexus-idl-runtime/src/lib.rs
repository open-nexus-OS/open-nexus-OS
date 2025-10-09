// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Userland IDL runtime: Cap'n Proto glue for local+remote control-plane messaging.
#![forbid(unsafe_code)]

#[cfg(feature = "capnp")]
pub mod samgr_capnp {
    include!(concat!(env!("OUT_DIR"), "/samgr_capnp.rs"));
}

#[cfg(feature = "capnp")]
pub mod bundlemgr_capnp {
    include!(concat!(env!("OUT_DIR"), "/bundlemgr_capnp.rs"));
}

#[cfg(feature = "capnp")]
pub mod identity_capnp {
    include!(concat!(env!("OUT_DIR"), "/identity_capnp.rs"));
}

#[cfg(feature = "capnp")]
pub mod dsoftbus_capnp {
    include!(concat!(env!("OUT_DIR"), "/dsoftbus_capnp.rs"));
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
