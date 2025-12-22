// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Virtual file system client library
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! PUBLIC API:
//!   - VfsClient: Virtual file system client
//!   - VfsError: Client error types
//!
//! DEPENDENCIES:
//!   - nexus-ipc: IPC communication
//!   - nexus-idl-runtime: IDL bindings
//!
//! ADR: docs/adr/0004-idl-runtime-architecture.md

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]
#![cfg_attr(nexus_env = "os", no_std)]

//! Userspace Virtual File System client helpers.
//!
//! Host builds use Cap'n Proto (IDL) frames over loopback transport.
//! OS builds use the bring-up opcode protocol over kernel IPC v1 (see `vfsd` os-lite).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

#[cfg(all(nexus_env = "host", not(feature = "idl-capnp")))]
compile_error!("Enable the `idl-capnp` feature for host builds of nexus-vfs.");

#[cfg(all(nexus_env = "host", feature = "idl-capnp"))]
use nexus_idl_runtime::vfs_capnp::{
    close_request, close_response, open_request, open_response, read_request, read_response,
    stat_request, stat_response,
};

const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;
const OPCODE_STAT: u8 = 4;

/// Result alias for VFS client operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the VFS client helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Requested path does not exist.
    NotFound,
    /// The provided file handle is invalid or has been closed already.
    InvalidHandle,
    /// The provided path is not valid.
    InvalidPath,
    /// Failed to encode a Cap'n Proto request.
    Encode,
    /// Failed to decode a Cap'n Proto response.
    Decode,
    /// The underlying transport rejected the operation.
    Ipc(String),
    /// Backend is not implemented for this build configuration.
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("path not found"),
            Self::InvalidHandle => f.write_str("invalid file handle"),
            Self::InvalidPath => f.write_str("invalid path"),
            Self::Encode => f.write_str("failed to encode request"),
            Self::Decode => f.write_str("failed to decode response"),
            Self::Ipc(s) => write!(f, "ipc error: {s}"),
            Self::Unsupported => f.write_str("backend unsupported"),
        }
    }
}

/// Handle identifying an opened file in the VFS service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileHandle(u32);

impl FileHandle {
    /// Creates a handle from the raw identifier returned by the service.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// Exposes the raw identifier.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Type of a directory entry returned by the VFS service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// Regular file.
    File,
    /// Directory entry.
    Directory,
    /// Any other file type.
    Other(u16),
}

impl FileKind {
    fn from_raw(value: u16) -> Self {
        match value {
            0 => Self::File,
            1 => Self::Directory,
            other => Self::Other(other),
        }
    }
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => f.write_str("file"),
            Self::Directory => f.write_str("directory"),
            Self::Other(kind) => write!(f, "kind-{kind}"),
        }
    }
}

/// Metadata returned by [`VfsClient::stat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metadata {
    size: u64,
    kind: FileKind,
}

impl Metadata {
    /// Creates a new metadata description.
    pub const fn new(size: u64, kind: FileKind) -> Self {
        Self { size, kind }
    }

    /// Returns the file size in bytes.
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Returns the file kind reported by the service.
    pub const fn kind(&self) -> FileKind {
        self.kind
    }
}

/// Client facade over the VFS service.
pub struct VfsClient {
    backend: Backend,
}

enum Backend {
    #[cfg(nexus_env = "host")]
    Host(host::Client),
    #[cfg(nexus_env = "os")]
    Os(os::Client),
}

impl VfsClient {
    /// Creates a client using the default backend.
    pub fn new() -> Result<Self> {
        Backend::new().map(|backend| Self { backend })
    }

    /// Opens a file located at `path`.
    pub fn open(&self, path: &str) -> Result<FileHandle> {
        #[cfg(nexus_env = "os")]
        {
            if !path.starts_with("pkg:/") {
                return Err(Error::InvalidPath);
            }
            let mut frame = Vec::with_capacity(1 + path.len());
            frame.push(OPCODE_OPEN);
            frame.extend_from_slice(path.as_bytes());
            let rsp = self.backend.call(frame)?;
            if rsp.len() < 1 + 4 || rsp[0] != 1 {
                return Err(Error::NotFound);
            }
            let fh = u32::from_le_bytes([rsp[1], rsp[2], rsp[3], rsp[4]]);
            return Ok(FileHandle::from_raw(fh));
        }
        #[cfg(all(nexus_env = "host", feature = "idl-capnp"))]
        {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut request = message.init_root::<open_request::Builder<'_>>();
            request.set_path(path);
        }
        let response = self.dispatch(OPCODE_OPEN, &message)?;
        let (opcode, payload) = response.split_first().ok_or(Error::Decode)?;
        if *opcode != OPCODE_OPEN {
            return Err(Error::Decode);
        }
        let mut cursor = std::io::Cursor::new(payload);
        let message =
            capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
                .map_err(|_| Error::Decode)?;
        let response = message
            .get_root::<open_response::Reader<'_>>()
            .map_err(|_| Error::Decode)?;
        if !response.get_ok() {
            return Err(Error::NotFound);
        }
        Ok(FileHandle::from_raw(response.get_fh()))
        }
    }

    /// Reads up to `len` bytes from file handle `fh` starting at offset `off`.
    pub fn read(&self, fh: FileHandle, off: u64, len: usize) -> Result<Vec<u8>> {
        #[cfg(nexus_env = "os")]
        {
            let mut frame = Vec::with_capacity(1 + 4 + 8 + 4);
            frame.push(OPCODE_READ);
            frame.extend_from_slice(&fh.raw().to_le_bytes());
            frame.extend_from_slice(&off.to_le_bytes());
            frame.extend_from_slice(&(len.min(u32::MAX as usize) as u32).to_le_bytes());
            let rsp = self.backend.call(frame)?;
            if rsp.first().copied() != Some(1) {
                return Err(Error::InvalidHandle);
            }
            return Ok(rsp[1..].to_vec());
        }
        #[cfg(all(nexus_env = "host", feature = "idl-capnp"))]
        {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut request = message.init_root::<read_request::Builder<'_>>();
            request.set_fh(fh.raw());
            request.set_off(off);
            request.set_len(len.min(u32::MAX as usize) as u32);
        }
        let response = self.dispatch(OPCODE_READ, &message)?;
        let (opcode, payload) = response.split_first().ok_or(Error::Decode)?;
        if *opcode != OPCODE_READ {
            return Err(Error::Decode);
        }
        let mut cursor = std::io::Cursor::new(payload);
        let message =
            capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
                .map_err(|_| Error::Decode)?;
        let response = message
            .get_root::<read_response::Reader<'_>>()
            .map_err(|_| Error::Decode)?;
        if !response.get_ok() {
            return Err(Error::InvalidHandle);
        }
        response
            .get_bytes()
            .map(|data| data.to_vec())
            .map_err(|_| Error::Decode)
        }
    }

    /// Closes the provided file handle.
    pub fn close(&self, fh: FileHandle) -> Result<()> {
        #[cfg(nexus_env = "os")]
        {
            let mut frame = Vec::with_capacity(1 + 4);
            frame.push(OPCODE_CLOSE);
            frame.extend_from_slice(&fh.raw().to_le_bytes());
            let rsp = self.backend.call(frame)?;
            if rsp.first().copied() == Some(1) {
                return Ok(());
            }
            return Err(Error::InvalidHandle);
        }
        #[cfg(all(nexus_env = "host", feature = "idl-capnp"))]
        {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut request = message.init_root::<close_request::Builder<'_>>();
            request.set_fh(fh.raw());
        }
        let response = self.dispatch(OPCODE_CLOSE, &message)?;
        let (opcode, payload) = response.split_first().ok_or(Error::Decode)?;
        if *opcode != OPCODE_CLOSE {
            return Err(Error::Decode);
        }
        let mut cursor = std::io::Cursor::new(payload);
        let message =
            capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
                .map_err(|_| Error::Decode)?;
        let response = message
            .get_root::<close_response::Reader<'_>>()
            .map_err(|_| Error::Decode)?;
        if response.get_ok() {
            Ok(())
        } else {
            Err(Error::InvalidHandle)
        }
        }
    }

    /// Retrieves metadata for the provided `path`.
    pub fn stat(&self, path: &str) -> Result<Metadata> {
        #[cfg(nexus_env = "os")]
        {
            if !path.starts_with("pkg:/") {
                return Err(Error::InvalidPath);
            }
            let mut frame = Vec::with_capacity(1 + path.len());
            frame.push(OPCODE_STAT);
            frame.extend_from_slice(path.as_bytes());
            let rsp = self.backend.call(frame)?;
            if rsp.len() < 1 + 8 + 2 || rsp[0] != 1 {
                return Err(Error::NotFound);
            }
            let size = u64::from_le_bytes([
                rsp[1], rsp[2], rsp[3], rsp[4], rsp[5], rsp[6], rsp[7], rsp[8],
            ]);
            let kind = u16::from_le_bytes([rsp[9], rsp[10]]);
            return Ok(Metadata::new(size, FileKind::from_raw(kind)));
        }
        #[cfg(all(nexus_env = "host", feature = "idl-capnp"))]
        {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut request = message.init_root::<stat_request::Builder<'_>>();
            request.set_path(path);
        }
        let response = self.dispatch(OPCODE_STAT, &message)?;
        let (opcode, payload) = response.split_first().ok_or(Error::Decode)?;
        if *opcode != OPCODE_STAT {
            return Err(Error::Decode);
        }
        let mut cursor = std::io::Cursor::new(payload);
        let message =
            capnp::serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
                .map_err(|_| Error::Decode)?;
        let response = message
            .get_root::<stat_response::Reader<'_>>()
            .map_err(|_| Error::Decode)?;
        if !response.get_ok() {
            return Err(Error::NotFound);
        }
        Ok(Metadata::new(
            response.get_size(),
            FileKind::from_raw(response.get_kind()),
        ))
        }
    }

    #[cfg(all(nexus_env = "host", feature = "idl-capnp"))]
    fn dispatch(
        &self,
        opcode: u8,
        message: &capnp::message::Builder<capnp::message::HeapAllocator>,
    ) -> Result<Vec<u8>> {
        let mut payload = Vec::new();
        capnp::serialize::write_message(&mut payload, message).map_err(|_| Error::Encode)?;
        let mut frame = Vec::with_capacity(1 + payload.len());
        frame.push(opcode);
        frame.extend_from_slice(&payload);
        self.backend.call(frame)
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

    /// Host backend using the in-process loopback transport.
    #[derive(Clone)]
    pub struct Client {
        ipc: Arc<LoopbackClient>,
    }

    impl Client {
        /// Wraps an existing loopback client handle.
        pub fn from_loopback(client: LoopbackClient) -> Self {
            Self {
                ipc: Arc::new(client),
            }
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

    impl super::VfsClient {
        /// Creates a client bound to the provided loopback connection.
        pub fn from_loopback(client: LoopbackClient) -> Self {
            Self {
                backend: super::Backend::Host(Client::from_loopback(client)),
            }
        }
    }
}

#[cfg(nexus_env = "os")]
mod os {
    use alloc::{format, vec::Vec};

    use super::{Error, Result};
    use nexus_ipc::{Client as _, KernelClient, Wait};

    /// OS backend forwarding requests to the kernel IPC channel.
    pub struct Client {
        ipc: KernelClient,
    }

    impl Client {
        pub fn new() -> Result<Self> {
            // Route to the vfsd service (init-lite responder).
            let ipc = KernelClient::new_for("vfsd").map_err(map_ipc_error)?;
            Ok(Self { ipc })
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
