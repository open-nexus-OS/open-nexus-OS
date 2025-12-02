//! CONTEXT: Policy daemon domain library (service API and handlers)
//! INTENT: Policy/entitlement/DAC checks, audit
//! IDL (target): checkPermission(subject,cap), addPolicy(entry), audit(record)
//! DEPS: keystored/identityd (crypto/IDs)
//! READINESS: print "policyd: ready"; register/heartbeat with samgr
//! TESTS: checkPermission loopback; deny/allow paths
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! policyd daemon: loads capability policies and answers allow/deny queries via Cap'n Proto IPC.

#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use nexus_ipc::{self, Wait};
use policy::PolicyDoc;
use thiserror::Error;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build policyd handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::policyd_capnp::{check_request, check_response};

const OPCODE_CHECK: u8 = 1;

/// Trait implemented by transports capable of delivering request frames to the daemon.
pub trait Transport {
    /// Error type surfaced by the transport implementation.
    type Error: Into<TransportError>;

    /// Receives the next request frame if one is available.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors emitted by transports when interacting with the daemon.
#[derive(Debug)]
pub enum TransportError {
    /// Transport has been closed by the peer.
    Closed,
    /// I/O error while reading from or writing to the transport.
    Io(std::io::Error),
    /// Current platform lacks an implementation for the transport.
    Unsupported,
    /// Any other error described by a string message.
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

impl From<nexus_ipc::IpcError> for TransportError {
    fn from(err: nexus_ipc::IpcError) -> Self {
        match err {
            nexus_ipc::IpcError::Disconnected => Self::Closed,
            nexus_ipc::IpcError::Unsupported => Self::Unsupported,
            nexus_ipc::IpcError::WouldBlock | nexus_ipc::IpcError::Timeout => {
                Self::Other("operation timed out".to_string())
            }
            nexus_ipc::IpcError::Kernel(inner) => {
                Self::Other(format!("kernel ipc error: {inner:?}"))
            }
        }
    }
}

/// Notifies init that the daemon has completed its startup sequence.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Transport backed by the [`nexus-ipc`] runtime.
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    /// Wraps a server implementation.
    pub fn new(server: T) -> Self {
        Self { server }
    }
}

impl<T> Transport for IpcTransport<T>
where
    T: nexus_ipc::Server + Send,
{
    type Error = nexus_ipc::IpcError;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        match self.server.recv(Wait::Blocking) {
            Ok(frame) => Ok(Some(frame)),
            Err(nexus_ipc::IpcError::Disconnected) => Ok(None),
            Err(nexus_ipc::IpcError::WouldBlock | nexus_ipc::IpcError::Timeout) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.server.send(frame, Wait::Blocking)
    }
}

/// Errors surfaced by the policy server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Transport level failure.
    #[error("transport error: {0}")]
    Transport(TransportError),
    /// Failed to decode an incoming request frame.
    #[error("decode error: {0}")]
    Decode(String),
    /// Failed to encode an outgoing response frame.
    #[error("encode error: {0}")]
    Encode(#[from] capnp::Error),
    /// Failed to load policies from disk.
    #[error("policy error: {0}")]
    Policy(#[from] policy::Error),
    /// Failed to inspect the policy directory.
    #[error("init error: {0}")]
    Init(String),
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

struct PolicyService {
    policy: PolicyDoc,
}

impl PolicyService {
    fn new(policy: PolicyDoc) -> Self {
        Self { policy }
    }

    fn handle_frame(&self, frame: &[u8]) -> Result<Vec<u8>, ServerError> {
        if frame.is_empty() {
            return Err(ServerError::Decode("empty request".to_string()));
        }
        match frame[0] {
            OPCODE_CHECK => self.handle_check(&frame[1..]),
            other => Err(ServerError::Decode(format!("unknown opcode {other}"))),
        }
    }

    fn handle_check(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        #[cfg(feature = "idl-capnp")]
        {
            let mut cursor = Cursor::new(payload);
            let message = serialize::read_message(&mut cursor, ReaderOptions::new())
                .map_err(|err| ServerError::Decode(format!("failed to read request: {err}")))?;
            let reader = message
                .get_root::<check_request::Reader<'_>>()
                .map_err(|err| {
                    ServerError::Decode(format!("failed to read request root: {err}"))
                })?;

            let subject = reader
                .get_subject()
                .map_err(|err| ServerError::Decode(format!("subject read error: {err}")))?;
            let subject = subject
                .to_str()
                .map_err(|err| ServerError::Decode(format!("subject utf8 error: {err}")))?;

            let caps_reader = reader
                .get_required_caps()
                .map_err(|err| ServerError::Decode(format!("required caps read error: {err}")))?;
            let mut caps = Vec::with_capacity(caps_reader.len() as usize);
            for idx in 0..caps_reader.len() {
                let cap = caps_reader
                    .get(idx)
                    .map_err(|err| ServerError::Decode(format!("cap read error: {err}")))?;
                let cap = cap
                    .to_str()
                    .map_err(|err| ServerError::Decode(format!("cap utf8 error: {err}")))?;
                caps.push(cap.to_string());
            }
            let cap_refs: Vec<&str> = caps.iter().map(String::as_str).collect();

            let mut message = Builder::new_default();
            {
                let mut response = message.init_root::<check_response::Builder<'_>>();
                match self.policy.check(&cap_refs, subject) {
                    Ok(()) => {
                        response.set_allowed(true);
                        response.reborrow().init_missing(0);
                    }
                    Err(denied) => {
                        response.set_allowed(false);
                        let mut missing = response
                            .reborrow()
                            .init_missing(denied.missing.len() as u32);
                        for (idx, cap) in denied.missing.iter().enumerate() {
                            missing.set(idx as u32, cap);
                        }
                    }
                }
            }

            let mut body = Vec::new();
            serialize::write_message(&mut body, &message)?;
            let mut frame = Vec::with_capacity(1 + body.len());
            frame.push(OPCODE_CHECK);
            frame.extend_from_slice(&body);
            Ok(frame)
        }

        #[cfg(not(feature = "idl-capnp"))]
        {
            let _ = payload;
            Err(ServerError::Decode("capnp support disabled".to_string()))
        }
    }
}

/// Runs the daemon main loop using the default transport backend.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), ServerError> {
    #[cfg(nexus_env = "host")]
    {
        let (client, server) = nexus_ipc::loopback_channel();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        run_with_transport_ready(&mut transport, notifier)
    }

    #[cfg(nexus_env = "os")]
    {
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        run_with_transport_ready(&mut transport, notifier)
    }
}

/// Runs the daemon using the provided transport and emits readiness markers.
pub fn run_with_transport_ready<T: Transport>(
    transport: &mut T,
    notifier: ReadyNotifier,
) -> Result<(), ServerError> {
    let (service, files, subjects, caps) = load_policy_service()?;
    log_policy_counts(files, subjects, caps);
    notifier.notify();
    println!("policyd: ready");
    let _ = try_register_with_samgr();
    serve(&service, transport)
}

/// Runs the daemon using the provided transport without emitting readiness markers.
pub fn run_with_transport<T: Transport>(transport: &mut T) -> Result<(), ServerError> {
    let (service, files, subjects, caps) = load_policy_service()?;
    log_policy_counts(files, subjects, caps);
    serve(&service, transport)
}

fn serve<T>(service: &PolicyService, transport: &mut T) -> Result<(), ServerError>
where
    T: Transport,
{
    loop {
        match transport
            .recv()
            .map_err(|err| ServerError::Transport(err.into()))?
        {
            Some(frame) => {
                let response = service.handle_frame(&frame)?;
                transport
                    .send(&response)
                    .map_err(|err| ServerError::Transport(err.into()))?;
            }
            None => return Ok(()),
        }
    }
}

fn policy_dir() -> PathBuf {
    match env::var("NEXUS_POLICY_DIR") {
        Ok(path) => PathBuf::from(path),
        Err(_) => Path::new("recipes/policy").to_path_buf(),
    }
}

fn count_policy_files(dir: &Path) -> Result<usize, ServerError> {
    let entries = std::fs::read_dir(dir).map_err(|err| {
        ServerError::Init(format!(
            "failed to read policy dir {}: {err}",
            dir.display()
        ))
    })?;
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|err| {
            ServerError::Init(format!(
                "failed to read policy dir {}: {err}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            count += 1;
        }
    }
    Ok(count)
}

fn load_policy_service() -> Result<(PolicyService, usize, usize, usize), ServerError> {
    let dir = policy_dir();
    let file_count = count_policy_files(&dir)?;
    let policy = PolicyDoc::load_dir(&dir)?;
    let subjects = policy.subject_count();
    let caps = policy.capability_count();
    Ok((PolicyService::new(policy), file_count, subjects, caps))
}

fn log_policy_counts(files: usize, subjects: usize, caps: usize) {
    println!("policyd: policies files={files} subjects={subjects} caps={caps}");
}

/// Attempts to register the daemon with `samgr` if a client is available.
fn try_register_with_samgr() -> Result<(), String> {
    // Placeholder: once shared IPC clients exist, register policyd with samgrd.
    Ok(())
}

/// Runs the daemon entry point until termination.
pub fn daemon_main<R: FnOnce() + Send + 'static>(notify: R) -> ! {
    touch_schemas();
    if let Err(err) = service_main_loop(ReadyNotifier::new(notify)) {
        eprintln!("policyd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Creates a loopback transport pair for host-side tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (
    nexus_ipc::LoopbackClient,
    IpcTransport<nexus_ipc::LoopbackServer>,
) {
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

/// Touches the Cap'n Proto schema so release builds keep the generated module.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<check_request::Reader<'static>>();
        let _ = core::any::type_name::<check_response::Reader<'static>>();
    }
}
