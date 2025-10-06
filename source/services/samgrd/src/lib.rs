// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;

use samgr::{Endpoint, Registry, ServiceHandle};

#[cfg(all(feature = "backend-host", feature = "backend-os"))]
compile_error!("Enable only one of `backend-host` or `backend-os`.");

#[cfg(not(any(feature = "backend-host", feature = "backend-os")))]
compile_error!("Select a backend feature for samgrd.");

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build samgrd handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, HeapAllocator, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::samgr_capnp::{
    heartbeat, register_request, register_response, resolve_request, resolve_response,
};

const OPCODE_REGISTER: u8 = 1;
const OPCODE_RESOLVE: u8 = 2;
const OPCODE_HEARTBEAT: u8 = 3;

/// Trait implemented by transports capable of delivering request frames to the daemon.
pub trait Transport {
    /// Error type returned by the transport.
    type Error: Into<TransportError>;

    /// Receives the next request frame if available.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors originating from the transport layer.
#[derive(Debug)]
pub enum TransportError {
    /// Transport has been closed by the peer.
    Closed,
    /// I/O level failure while reading or writing.
    Io(std::io::Error),
    /// The transport is not implemented for this build.
    Unsupported,
    /// Any other transport issue described via string message.
    Other(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "transport closed"),
            Self::Io(err) => write!(f, "transport io error: {err}"),
            Self::Unsupported => write!(f, "transport unsupported"),
            Self::Other(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<String> for TransportError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

impl From<&str> for TransportError {
    fn from(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

/// Errors returned by the SAMGR server when processing requests.
#[derive(Debug)]
pub enum ServerError {
    /// Transport level failure.
    Transport(TransportError),
    /// Cap'n Proto decode failure.
    Decode(String),
    /// Cap'n Proto encode failure.
    Encode(capnp::Error),
    /// Backend registry returned a domain error.
    Registry(samgr::Error),
    /// Heartbeat referenced an unknown endpoint handle.
    UnknownEndpoint(u32),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Decode(msg) => write!(f, "decode error: {msg}"),
            Self::Encode(err) => write!(f, "encode error: {err}"),
            Self::Registry(err) => write!(f, "registry error: {err}"),
            Self::UnknownEndpoint(id) => write!(f, "unknown endpoint: {id}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<samgr::Error> for ServerError {
    fn from(err: samgr::Error) -> Self {
        Self::Registry(err)
    }
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

struct Server {
    registry: Registry,
    handles: HashMap<u32, ServiceHandle>,
}

impl Server {
    fn new(registry: Registry) -> Self {
        Self {
            registry,
            handles: HashMap::new(),
        }
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        match opcode {
            OPCODE_REGISTER => self.handle_register(payload),
            OPCODE_RESOLVE => self.handle_resolve(payload),
            OPCODE_HEARTBEAT => self.handle_heartbeat(payload),
            other => Err(ServerError::Decode(format!("unknown opcode {other}"))),
        }
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_register(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(format!("register read: {err}")))?;
        let request = message
            .get_root::<register_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(format!("register root: {err}")))?;
        let name = request
            .get_name()
            .map_err(|err| ServerError::Decode(format!("register name: {err}")))?
            .to_string();
        let endpoint_id = request.get_endpoint();
        let endpoint = Endpoint::new(endpoint_id.to_string());
        let result = self.registry.register(name, endpoint);
        let mut response = Builder::new_default();
        response
            .init_root::<register_response::Builder<'_>>()
            .set_ok(result.is_ok());
        if let Ok(handle) = result {
            self.handles.insert(endpoint_id, handle);
        }
        Self::encode_response(OPCODE_REGISTER, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_resolve(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(format!("resolve read: {err}")))?;
        let request = message
            .get_root::<resolve_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(format!("resolve root: {err}")))?;
        let name = request
            .get_name()
            .map_err(|err| ServerError::Decode(format!("resolve name: {err}")))?
            .to_string();
        let mut response = Builder::new_default();
        let mut builder = response.init_root::<resolve_response::Builder<'_>>();
        match self.registry.resolve(&name) {
            Ok(handle) => {
                let endpoint_id =
                    handle.endpoint.as_str().parse::<u32>().map_err(|err| {
                        ServerError::Decode(format!("resolve endpoint parse: {err}"))
                    })?;
                builder.set_found(true);
                builder.set_endpoint(endpoint_id);
                self.handles.insert(endpoint_id, handle);
            }
            Err(samgr::Error::NotFound) => {
                builder.set_found(false);
                builder.set_endpoint(0);
            }
            Err(other) => return Err(ServerError::Registry(other)),
        }
        Self::encode_response(OPCODE_RESOLVE, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_heartbeat(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(format!("heartbeat read: {err}")))?;
        let request = message
            .get_root::<heartbeat::Reader<'_>>()
            .map_err(|err| ServerError::Decode(format!("heartbeat root: {err}")))?;
        let endpoint_id = request.get_endpoint();
        let handle = self
            .handles
            .get(&endpoint_id)
            .cloned()
            .ok_or(ServerError::UnknownEndpoint(endpoint_id))?;
        self.registry.heartbeat(&handle)?;
        let mut response = Builder::new_default();
        response
            .init_root::<heartbeat::Builder<'_>>()
            .set_endpoint(endpoint_id);
        Self::encode_response(OPCODE_HEARTBEAT, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn encode_response(
        opcode: u8,
        message: &Builder<HeapAllocator>,
    ) -> Result<Vec<u8>, ServerError> {
        let mut payload = Vec::new();
        serialize::write_message(&mut payload, message).map_err(ServerError::Encode)?;
        let mut frame = Vec::with_capacity(1 + payload.len());
        frame.push(opcode);
        frame.extend_from_slice(&payload);
        Ok(frame)
    }
}

/// Runs the server using the provided transport and a fresh registry backend.
#[cfg(feature = "idl-capnp")]
pub fn run_with_transport<T: Transport>(transport: &mut T) -> Result<(), ServerError> {
    let registry = Registry::new();
    serve_with_registry(transport, registry)
}

/// Serves requests using the provided transport and registry instance.
#[cfg(feature = "idl-capnp")]
pub fn serve_with_registry<T: Transport>(
    transport: &mut T,
    registry: Registry,
) -> Result<(), ServerError> {
    let mut server = Server::new(registry);
    loop {
        let frame = match transport
            .recv()
            .map_err(|err| ServerError::Transport(err.into()))?
        {
            Some(frame) => frame,
            None => break,
        };
        if frame.is_empty() {
            continue;
        }
        let (opcode, payload) = frame
            .split_first()
            .ok_or_else(|| ServerError::Decode("empty frame".into()))?;
        let response = server.handle_frame(*opcode, payload)?;
        transport
            .send(&response)
            .map_err(|err| ServerError::Transport(err.into()))?;
    }
    Ok(())
}

/// Executes the server using the default system transport (currently unsupported).
pub fn run_default() -> Result<(), ServerError> {
    Err(ServerError::Transport(TransportError::Unsupported))
}

/// Touches Cap'n Proto schemas to keep `capnpc` outputs linked in release builds.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<register_request::Reader<'static>>();
        let _ = core::any::type_name::<register_response::Reader<'static>>();
        let _ = core::any::type_name::<resolve_request::Reader<'static>>();
        let _ = core::any::type_name::<resolve_response::Reader<'static>>();
        let _ = core::any::type_name::<heartbeat::Reader<'static>>();
    }
}
