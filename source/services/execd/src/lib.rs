// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! execd daemon: executes service bundles after policy approval.

#![forbid(unsafe_code)]

#[cfg(nexus_env = "os")]
use core::convert::TryFrom;
use std::fmt;
use std::io::Cursor;

#[cfg(nexus_env = "os")]
use std::sync::OnceLock;

use nexus_ipc::{self, Wait};
use thiserror::Error;

#[cfg(nexus_env = "os")]
use exec_payloads::{hello_child_entry, BootstrapMsg, HELLO_ELF};
#[cfg(nexus_env = "os")]
use nexus_abi::{self, AbiError, Rights};
#[cfg(nexus_env = "os")]
use nexus_loader::{
    self,
    os_mapper::{OsMapper, StackBuilder},
};

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build execd handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(all(feature = "idl-capnp", nexus_env = "os"))]
use nexus_idl_runtime::bundlemgr_capnp::{get_payload_request, get_payload_response};
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::execd_capnp::{exec_request, exec_response};

const OPCODE_EXEC: u8 = 1;
#[cfg(all(feature = "idl-capnp", nexus_env = "os"))]
const BUNDLE_OPCODE_GET_PAYLOAD: u8 = 3;

#[cfg(nexus_env = "os")]
const CHILD_STACK_LEN: usize = 4096;
#[cfg(nexus_env = "os")]
const BOOTSTRAP_SLOT: u32 = 0;
#[cfg(nexus_env = "os")]
const CHILD_STACK_TOP: u64 = 0x4000_0000;
#[cfg(nexus_env = "os")]
const CHILD_STACK_PAGES: u64 = 4;
#[cfg(nexus_env = "os")]
static CHILD_STACK_BASE: OnceLock<usize> = OnceLock::new();

/// Trait implemented by transports capable of delivering execution requests.
pub trait Transport {
    type Error: Into<TransportError>;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors emitted by transports interacting with execd.
#[derive(Debug)]
pub enum TransportError {
    Closed,
    Io(std::io::Error),
    Unsupported,
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

/// Notifies the init process that the daemon has completed its startup sequence.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    pub fn notify(self) {
        (self.0)();
    }
}

/// Transport backed by the [`nexus-ipc`] runtime.
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
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

/// Errors surfaced by execd when processing requests.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("transport error: {0}")]
    Transport(TransportError),
    #[error("decode error: {0}")]
    Decode(String),
    #[cfg(feature = "idl-capnp")]
    #[error("encode error: {0}")]
    Encode(#[from] capnp::Error),
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

/// Errors surfaced by the minimal exec path.
#[derive(Debug, Error)]
pub enum ExecError {
    #[error("exec unsupported on this build")]
    Unsupported,
    #[cfg(nexus_env = "os")]
    #[error("spawn syscall failed: {0:?}")]
    Spawn(AbiError),
    #[cfg(nexus_env = "os")]
    #[error("capability transfer failed: {0:?}")]
    CapTransfer(AbiError),
    #[cfg(nexus_env = "os")]
    #[error("address space creation failed: {0:?}")]
    AsCreate(AbiError),
    #[cfg(nexus_env = "os")]
    #[error("address space map failed: {0:?}")]
    AsMap(AbiError),
    #[cfg(nexus_env = "os")]
    #[error("VMO operation failed: {0:?}")]
    Vmo(nexus_abi::IpcError),
    #[cfg(nexus_env = "os")]
    #[error("ipc error: {0:?}")]
    Ipc(nexus_ipc::IpcError),
    #[cfg(nexus_env = "os")]
    #[error("bundle payload error: {0}")]
    Payload(String),
    #[cfg(nexus_env = "os")]
    #[error("loader error: {0}")]
    Loader(nexus_loader::Error),
    #[cfg(nexus_env = "os")]
    #[error("ELF image exceeds VMO limits")]
    ImageTooLarge,
}

struct ExecService;

impl ExecService {
    fn new() -> Self {
        Self
    }

    fn handle_frame(&self, frame: &[u8]) -> Result<Vec<u8>, ServerError> {
        if frame.is_empty() {
            return Err(ServerError::Decode("empty request".to_string()));
        }
        match frame[0] {
            OPCODE_EXEC => self.handle_exec(&frame[1..]),
            other => Err(ServerError::Decode(format!("unknown opcode {other}"))),
        }
    }

    fn handle_exec(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        #[cfg(feature = "idl-capnp")]
        {
            let mut cursor = Cursor::new(payload);
            let message = serialize::read_message(&mut cursor, ReaderOptions::new())
                .map_err(|err| ServerError::Decode(format!("read exec request: {err}")))?;
            let reader = message
                .get_root::<exec_request::Reader<'_>>()
                .map_err(|err| ServerError::Decode(format!("exec request root: {err}")))?;
            let name = reader
                .get_name()
                .map_err(|err| ServerError::Decode(format!("exec name read: {err}")))?
                .to_str()
                .map_err(|err| ServerError::Decode(format!("exec name utf8: {err}")))?
                .to_string();

            println!("execd: exec {name}");
            let exec_result = self.exec_minimal(&name);

            let mut response = Builder::new_default();
            {
                let mut builder = response.init_root::<exec_response::Builder<'_>>();
                match exec_result {
                    Ok(()) => {
                        builder.set_ok(true);
                        builder.set_message("");
                    }
                    Err(err) => {
                        builder.set_ok(false);
                        builder.set_message(format!("{err}"));
                    }
                }
            }

            let mut body = Vec::new();
            serialize::write_message(&mut body, &response)?;
            let mut frame = Vec::with_capacity(1 + body.len());
            frame.push(OPCODE_EXEC);
            frame.extend_from_slice(&body);
            Ok(frame)
        }

        #[cfg(not(feature = "idl-capnp"))]
        {
            let _ = payload;
            Err(ServerError::Decode("capnp support disabled".to_string()))
        }
    }

    fn exec_minimal(&self, subject: &str) -> Result<(), ExecError> {
        #[cfg(nexus_env = "os")]
        {
            exec_minimal(subject)
        }

        #[cfg(not(nexus_env = "os"))]
        {
            let _ = subject;
            Err(ExecError::Unsupported)
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

/// Executes the minimal bootstrap path for `subject` using the OS syscalls.
pub fn exec_minimal(subject: &str) -> Result<(), ExecError> {
    exec_minimal_impl(subject)
}

#[cfg(nexus_env = "os")]
pub fn exec_elf_bytes(bytes: &[u8], argv: &[&str], env: &[&str]) -> Result<(), ExecError> {
    let _ = run_loaded_elf(bytes, argv, env)?;
    Ok(())
}

#[cfg(nexus_env = "os")]
pub fn exec_hello_elf() -> Result<(), ExecError> {
    let _ = run_loaded_elf(HELLO_ELF, &["hello-elf"], &["K=V"])?;
    Ok(())
}

#[cfg(nexus_env = "os")]
fn run_loaded_elf(bytes: &[u8], argv: &[&str], env: &[&str]) -> Result<nexus_abi::Pid, ExecError> {
    let plan_info = nexus_loader::parse_elf64_riscv(bytes).map_err(ExecError::Loader)?;
    let vmo_len = compute_vmo_len(&plan_info, bytes.len())?;
    let vmo_len_usize = usize::try_from(vmo_len).map_err(|_| ExecError::ImageTooLarge)?;

    let as_handle = nexus_abi::as_create().map_err(ExecError::AsCreate)?;
    let bundle_vmo = nexus_abi::vmo_create(vmo_len_usize).map_err(ExecError::Vmo)?;
    nexus_abi::vmo_write(bundle_vmo, 0, bytes).map_err(ExecError::Vmo)?;

    let mut mapper = OsMapper::new(as_handle, bundle_vmo);
    let plan = nexus_loader::load_with(bytes, &mut mapper).map_err(ExecError::Loader)?;

    let stack_builder =
        StackBuilder::new(CHILD_STACK_TOP, CHILD_STACK_PAGES).map_err(ExecError::Loader)?;
    let stack_vmo = stack_builder.map_stack(as_handle).map_err(ExecError::Loader)?;
    let (stack_sp, argv_ptr, env_ptr) =
        stack_builder.populate(stack_vmo, argv, env).map_err(ExecError::Loader)?;

    let argc = argv.len() as u32;
    let _bootstrap =
        BootstrapMsg { argc, argv_ptr, env_ptr, cap_seed_ep: BOOTSTRAP_SLOT, flags: 0 };

    let entry_pc = plan.entry;
    let pid = nexus_abi::spawn(entry_pc, stack_sp, as_handle, BOOTSTRAP_SLOT)
        .map_err(ExecError::Spawn)?;
    let _slot = nexus_abi::cap_transfer(pid, BOOTSTRAP_SLOT, Rights::SEND)
        .map_err(ExecError::CapTransfer)?;

    println!("execd: elf load ok {pid}");
    Ok(pid)
}

#[cfg(all(feature = "idl-capnp", nexus_env = "os"))]
fn request_bundle_payload(name: &str) -> Result<Vec<u8>, ExecError> {
    let client = nexus_ipc::KernelClient::new().map_err(ExecError::Ipc)?;

    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<get_payload_request::Builder<'_>>();
        request.set_name(name);
    }

    let mut body = Vec::new();
    serialize::write_message(&mut body, &message)
        .map_err(|err| ExecError::Payload(format!("encode payload request: {err}")))?;

    let mut frame = Vec::with_capacity(1 + body.len());
    frame.push(BUNDLE_OPCODE_GET_PAYLOAD);
    frame.extend_from_slice(&body);
    client.send(&frame, Wait::Blocking).map_err(ExecError::Ipc)?;

    let response = client.recv(Wait::Blocking).map_err(ExecError::Ipc)?;
    let (opcode, payload) = response
        .split_first()
        .ok_or_else(|| ExecError::Payload("empty get_payload response".into()))?;
    if *opcode != BUNDLE_OPCODE_GET_PAYLOAD {
        return Err(ExecError::Payload(format!("unexpected opcode {opcode}")));
    }

    let mut cursor = Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ExecError::Payload(format!("decode get_payload: {err}")))?;
    let reader = message
        .get_root::<get_payload_response::Reader<'_>>()
        .map_err(|err| ExecError::Payload(format!("get_payload root: {err}")))?;
    if !reader.get_ok() {
        return Err(ExecError::Payload(format!("bundle {name} payload unavailable")));
    }
    let bytes =
        reader.get_bytes().map_err(|err| ExecError::Payload(format!("payload bytes: {err}")))?;
    Ok(bytes.to_vec())
}

#[cfg(all(feature = "idl-capnp", nexus_env = "os"))]
pub fn exec_elf(bundle: &str, argv: &[&str], env: &[&str]) -> Result<(), ExecError> {
    println!("execd: exec {bundle}");
    let payload = request_bundle_payload(bundle)?;
    let _ = run_loaded_elf(&payload, argv, env)?;
    Ok(())
}

#[cfg(all(not(feature = "idl-capnp"), nexus_env = "os"))]
pub fn exec_elf(_bundle: &str, _argv: &[&str], _env: &[&str]) -> Result<(), ExecError> {
    Err(ExecError::Unsupported)
}

#[cfg(nexus_env = "os")]
fn exec_minimal_impl(subject: &str) -> Result<(), ExecError> {
    println!("execd: exec_minimal {subject}");
    let stack_top = child_stack_top();
    let entry_pc = hello_child_entry as usize as u64;
    let pid = nexus_abi::spawn(entry_pc, stack_top, 0, BOOTSTRAP_SLOT).map_err(ExecError::Spawn)?;
    let _slot = nexus_abi::cap_transfer(pid, BOOTSTRAP_SLOT, Rights::SEND)
        .map_err(ExecError::CapTransfer)?;
    println!("execd: spawn ok {pid}");
    Ok(())
}

#[cfg(nexus_env = "os")]
fn compute_vmo_len(plan: &nexus_loader::LoadPlan, file_len: usize) -> Result<u64, ExecError> {
    let mut required =
        align_up(file_len as u64, nexus_loader::PAGE_SIZE).ok_or(ExecError::ImageTooLarge)?;
    for seg in &plan.segments {
        let file_extent = seg.off.checked_add(seg.filesz).ok_or(ExecError::ImageTooLarge)?;
        required = required.max(file_extent);
        let mem_len =
            align_up(seg.memsz, nexus_loader::PAGE_SIZE).ok_or(ExecError::ImageTooLarge)?;
        required = required.max(mem_len);
    }
    align_up(required, nexus_loader::PAGE_SIZE).ok_or(ExecError::ImageTooLarge)
}

#[cfg(nexus_env = "os")]
fn align_up(value: u64, align: u64) -> Option<u64> {
    if align == 0 {
        return Some(value);
    }
    let mask = align - 1;
    value.checked_add(mask).map(|sum| sum & !mask)
}

#[cfg(not(nexus_env = "os"))]
fn exec_minimal_impl(_subject: &str) -> Result<(), ExecError> {
    Err(ExecError::Unsupported)
}

#[cfg(nexus_env = "os")]
fn child_stack_top() -> u64 {
    // TEMP: child tasks still share the parent's address space. Carve-out a static stack
    // until per-task allocators and address spaces are wired.
    let base = *CHILD_STACK_BASE.get_or_init(|| {
        let mut buf = Box::new([0u8; CHILD_STACK_LEN]);
        let base = buf.as_mut_ptr() as usize;
        Box::leak(buf);
        base
    });
    let top = base + CHILD_STACK_LEN;
    (top & !0xf) as u64
}

/// Runs the daemon using the provided transport and emits readiness markers.
pub fn run_with_transport_ready<T: Transport>(
    transport: &mut T,
    notifier: ReadyNotifier,
) -> Result<(), ServerError> {
    touch_schemas();
    let service = ExecService::new();
    notifier.notify();
    println!("execd: ready");
    serve(&service, transport)
}

/// Runs the daemon using the provided transport without emitting readiness markers.
pub fn run_with_transport<T: Transport>(transport: &mut T) -> Result<(), ServerError> {
    touch_schemas();
    let service = ExecService::new();
    serve(&service, transport)
}

fn serve<T>(service: &ExecService, transport: &mut T) -> Result<(), ServerError>
where
    T: Transport,
{
    loop {
        match transport.recv().map_err(|err| ServerError::Transport(err.into()))? {
            Some(frame) => {
                let response = service.handle_frame(&frame)?;
                transport.send(&response).map_err(|err| ServerError::Transport(err.into()))?;
            }
            None => return Ok(()),
        }
    }
}

/// Runs the daemon entry point until termination.
pub fn daemon_main<R: FnOnce() + Send + 'static>(notify: R) -> ! {
    touch_schemas();
    if let Err(err) = service_main_loop(ReadyNotifier::new(notify)) {
        eprintln!("execd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Creates a loopback transport pair for host-side tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

/// Touches the Cap'n Proto schema so release builds keep the generated module.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<exec_request::Reader<'static>>();
        let _ = core::any::type_name::<exec_response::Reader<'static>>();
    }
}
