// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

//! Client helpers for the package file system service.

use thiserror::Error;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build the packagefs client.");

use nexus_idl_runtime::packagefs_capnp::{
    publish_bundle, publish_response, resolve_path, resolve_response,
};

const OPCODE_PUBLISH: u8 = 1;
const OPCODE_RESOLVE: u8 = 2;

/// Result alias for packagefs client operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the packagefs client.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    /// Underlying IPC channel rejected the request.
    #[error("ipc error: {0}")]
    Ipc(String),
    /// Failed to encode a Cap'n Proto request.
    #[error("encode error")]
    Encode,
    /// Failed to decode a Cap'n Proto response.
    #[error("decode error")]
    Decode,
    /// Requested entry was not found.
    #[error("entry not found")]
    NotFound,
    /// The provided path was invalid.
    #[error("invalid path")]
    InvalidPath,
    /// Service rejected the publish request.
    #[error("publish rejected")]
    Rejected,
    /// Backend not wired for this build configuration.
    #[error("backend unsupported")]
    Unsupported,
}

/// Bundle entry to be published.
#[derive(Debug, Clone)]
pub struct BundleEntry<'a> {
    path: &'a str,
    kind: u16,
    bytes: &'a [u8],
}

impl<'a> BundleEntry<'a> {
    /// Creates a new bundle entry description.
    pub fn new(path: &'a str, kind: u16, bytes: &'a [u8]) -> Self {
        Self { path, kind, bytes }
    }

    fn path(&self) -> &str {
        self.path
    }

    fn kind(&self) -> u16 {
        self.kind
    }

    fn bytes(&self) -> &[u8] {
        self.bytes
    }
}

/// Parameters for `publish_bundle`.
#[derive(Debug, Clone)]
pub struct PublishRequest<'a> {
    /// Bundle identifier.
    pub name: &'a str,
    /// Semantic version string.
    pub version: &'a str,
    /// Root VMO handle placeholder.
    pub root_vmo: u32,
    /// File entries exposed by the bundle.
    pub entries: &'a [BundleEntry<'a>],
}

/// Entry returned by [`PackageFsClient::resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEntry {
    size: u64,
    kind: u16,
    bytes: Vec<u8>,
}

impl ResolvedEntry {
    /// Creates a new resolved entry.
    pub fn new(size: u64, kind: u16, bytes: Vec<u8>) -> Self {
        Self { size, kind, bytes }
    }

    /// Returns the file size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the raw file kind identifier.
    pub fn kind(&self) -> u16 {
        self.kind
    }

    /// Returns the file contents.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Package file system client facade.
pub struct PackageFsClient {
    backend: Backend,
}

enum Backend {
    #[cfg(nexus_env = "host")]
    Host(host::Client),
    #[cfg(nexus_env = "os")]
    Os(os::Client),
}

impl PackageFsClient {
    /// Creates a client using the default backend.
    pub fn new() -> Result<Self> {
        Backend::new().map(|backend| Self { backend })
    }

    /// Publishes bundle metadata to packagefs.
    pub fn publish_bundle(&self, request: PublishRequest<'_>) -> Result<()> {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut req = message.init_root::<publish_bundle::Builder<'_>>();
            req.set_name(request.name);
            req.set_version(request.version);
            req.set_root_vmo(request.root_vmo);
            let mut entries = req.reborrow().init_entries(request.entries.len() as u32);
            for (idx, entry) in request.entries.iter().enumerate() {
                let mut slot = entries.reborrow().get(idx as u32);
                slot.set_path(entry.path());
                slot.set_kind(entry.kind());
                slot.set_bytes(entry.bytes());
            }
        }
        let mut payload = Vec::new();
        capnp::serialize::write_message(&mut payload, &message).map_err(|_| Error::Encode)?;
        let mut frame = Vec::with_capacity(1 + payload.len());
        frame.push(OPCODE_PUBLISH);
        frame.extend_from_slice(&payload);
        let response = self.backend.call(frame)?;
        let (opcode, body) = response.split_first().ok_or(Error::Decode)?;
        if *opcode != OPCODE_PUBLISH {
            return Err(Error::Decode);
        }
        let mut cursor = std::io::Cursor::new(body);
        let message = capnp::serialize::read_message(
            &mut cursor,
            capnp::message::ReaderOptions::new(),
        )
        .map_err(|_| Error::Decode)?;
        let response = message
            .get_root::<publish_response::Reader<'_>>()
            .map_err(|_| Error::Decode)?;
        if response.get_ok() {
            Ok(())
        } else {
            Err(Error::Rejected)
        }
    }

    /// Resolves a path relative to the package root.
    pub fn resolve(&self, rel: &str) -> Result<ResolvedEntry> {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut req = message.init_root::<resolve_path::Builder<'_>>();
            req.set_rel(rel);
        }
        let mut payload = Vec::new();
        capnp::serialize::write_message(&mut payload, &message).map_err(|_| Error::Encode)?;
        let mut frame = Vec::with_capacity(1 + payload.len());
        frame.push(OPCODE_RESOLVE);
        frame.extend_from_slice(&payload);
        let response = self.backend.call(frame)?;
        let (opcode, body) = response.split_first().ok_or(Error::Decode)?;
        if *opcode != OPCODE_RESOLVE {
            return Err(Error::Decode);
        }
        let mut cursor = std::io::Cursor::new(body);
        let message = capnp::serialize::read_message(
            &mut cursor,
            capnp::message::ReaderOptions::new(),
        )
        .map_err(|_| Error::Decode)?;
        let response = message
            .get_root::<resolve_response::Reader<'_>>()
            .map_err(|_| Error::Decode)?;
        if !response.get_ok() {
            return Err(Error::NotFound);
        }
        let size = response.get_size();
        let kind = response.get_kind();
        let bytes = response
            .get_bytes()
            .map_err(|_| Error::Decode)?
            .to_vec();
        Ok(ResolvedEntry::new(size, kind, bytes))
    }

    #[cfg(nexus_env = "host")]
    /// Creates a client bound to the provided loopback connection.
    pub fn from_loopback(client: nexus_ipc::LoopbackClient) -> Self {
        Self { backend: Backend::Host(host::Client::from_loopback(client)) }
    }
}

impl Backend {
    fn new() -> Result<Self> {
        #[cfg(nexus_env = "host")]
        {
            Err(Error::Unsupported)
        }
        #[cfg(nexus_env = "os")]
        {
            os::Client::new().map(Self::Os)
        }
    }

    fn call(&self, frame: Vec<u8>) -> Result<Vec<u8>> {
        match self {
            #[cfg(nexus_env = "host")]
            Self::Host(client) => client.call(frame),
            #[cfg(nexus_env = "os")]
            Self::Os(client) => client.call(frame),
        }
    }
}

#[cfg(nexus_env = "host")]
mod host {
    use std::sync::Arc;

    use super::{Error, Result};
    use nexus_ipc::{Client as _, LoopbackClient, Wait};

    #[derive(Clone)]
    pub struct Client {
        ipc: Arc<LoopbackClient>,
    }

    impl Client {
        pub fn from_loopback(client: LoopbackClient) -> Self {
            Self { ipc: Arc::new(client) }
        }

        pub fn call(&self, frame: Vec<u8>) -> Result<Vec<u8>> {
            if let Err(err) = self.ipc.send(&frame, Wait::Blocking) {
                return Err(map_ipc_error(err));
            }
            self.ipc.recv(Wait::Blocking).map_err(map_ipc_error)
        }
    }

    fn map_ipc_error(err: nexus_ipc::IpcError) -> Error {
        match err {
            nexus_ipc::IpcError::Unsupported => Error::Unsupported,
            other => Error::Ipc(format!("{other:?}")),
        }
    }
}

#[cfg(nexus_env = "os")]
mod os {
    use super::{Error, Result};
    use nexus_ipc::{Client as _, KernelClient, Wait};

    pub struct Client {
        ipc: KernelClient,
    }

    impl Client {
        pub fn new() -> Result<Self> {
            KernelClient::new()
                .map(|ipc| Self { ipc })
                .map_err(map_ipc_error)
        }

        pub fn call(&self, frame: Vec<u8>) -> Result<Vec<u8>> {
            if let Err(err) = self.ipc.send(&frame, Wait::Blocking) {
                return Err(map_ipc_error(err));
            }
            self.ipc.recv(Wait::Blocking).map_err(map_ipc_error)
        }
    }

    fn map_ipc_error(err: nexus_ipc::IpcError) -> Error {
        match err {
            nexus_ipc::IpcError::Unsupported => Error::Unsupported,
            other => Error::Ipc(format!("{other:?}")),
        }
    }
}
