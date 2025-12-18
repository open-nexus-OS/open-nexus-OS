#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, exec, yield_, Pid};
use nexus_ipc::{KernelServer, Server as _, Wait};

use demo_exit0::DEMO_EXIT0_ELF;
use exec_payloads::HELLO_ELF;

/// Result alias surfaced by the lite execd backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Restart policy for launched services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart the service after exit.
    Never,
    /// Always restart the service when it exits.
    Always,
}

/// Ready notifier invoked once execd finishes initialization.
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

/// Errors surfaced by the lite execd backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Placeholder error until the lite backend is implemented.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "execd unsupported"),
        }
    }
}

/// Errors returned by exec helpers on the lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// Functionality not available yet.
    Unsupported,
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "exec unsupported"),
        }
    }
}

/// Schema warmer placeholder retained for parity.
pub fn touch_schemas() {}

const OPCODE_SPAWN: u8 = 1;
const PAYLOAD_HELLO: u8 = 1;
const PAYLOAD_EXIT0: u8 = 2;

/// Stubbed service loop that reports readiness and yields forever.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("execd: ready (stub)");
    let server = KernelServer::new_for("execd").map_err(|_| ServerError::Unsupported)?;
    loop {
        match server.recv(Wait::Blocking) {
            Ok(frame) => {
                if frame.len() < 2 || frame[0] != OPCODE_SPAWN {
                    continue;
                }
                let which = frame[1];
                let elf = match which {
                    PAYLOAD_HELLO => HELLO_ELF,
                    PAYLOAD_EXIT0 => DEMO_EXIT0_ELF,
                    _ => continue,
                };
                // Spawn via kernel exec path.
                let pid = exec(elf, 8, 0).ok();
                let mut rsp = [0u8; 1 + 1 + 4];
                rsp[0] = OPCODE_SPAWN;
                if let Some(pid) = pid {
                    rsp[1] = 1;
                    rsp[2..6].copy_from_slice(&(pid as u32).to_le_bytes());
                } else {
                    rsp[1] = 0;
                    rsp[2..6].copy_from_slice(&0u32.to_le_bytes());
                }
                let _ = server.send(&rsp, Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }
    }
}

/// Stubbed bundle exec helper exposed for API compatibility.
pub fn exec_elf(
    _bundle: &str,
    _argv: &[&str],
    _env: &[&str],
    _restart: RestartPolicy,
) -> Result<Pid, ExecError> {
    Err(ExecError::Unsupported)
}

fn emit_line(message: &str) {
    for byte in message
        .as_bytes()
        .iter()
        .copied()
        .chain(core::iter::once(b'\n'))
    {
        let _ = debug_putc(byte);
    }
}
