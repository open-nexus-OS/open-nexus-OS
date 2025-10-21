#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::string::{String, ToString};
use core::fmt;

use bundlemgrd;
use execd;
use keystored;
use packagefsd;
use policyd;
use samgrd;
use vfsd;

use nexus_abi::yield_;
use nexus_ipc::set_default_target;
use nexus_sync::SpinLock;

/// Callback invoked when the cooperative bootstrap has reached a stable state.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Create a new notifier from the supplied closure.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Execute the wrapped callback.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Placeholder error type used by the os-lite backend.
#[derive(Debug)]
pub enum InitError {}

/// Errors produced when launching services from the os-lite runtime.
#[derive(Debug)]
pub enum ServiceError {
    /// Wrapper around the lightweight packagefs service error type.
    Lite(packagefsd::LiteError),
    /// Wrapper around the lightweight VFS dispatcher error type.
    Vfs(vfsd::Error),
    /// Wrapper around IPC errors surfaced while initializing transports.
    Ipc(nexus_ipc::IpcError),
    /// Error message describing a failure without structured context.
    Message(&'static str),
    /// Owned string describing a failure bubbled up from a service loop.
    Detail(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lite(err) => write!(f, "{err}"),
            Self::Vfs(err) => match err {
                vfsd::Error::Transport => write!(f, "transport error"),
                vfsd::Error::InvalidPath => write!(f, "invalid path"),
                vfsd::Error::NotFound => write!(f, "entry not found"),
                vfsd::Error::BadHandle => write!(f, "bad file handle"),
            },
            Self::Ipc(err) => write!(f, "{err}"),
            Self::Message(msg) => write!(f, "{msg}"),
            Self::Detail(msg) => write!(f, "{msg}"),
        }
    }
}

struct ServiceReadyState {
    name: &'static str,
    signaled: bool,
}

struct ServiceReadyCell {
    state: SpinLock<Option<ServiceReadyState>>,
}

impl ServiceReadyCell {
    const fn new() -> Self {
        Self { state: SpinLock::new(None) }
    }

    fn begin(&self, name: &'static str) -> Option<ServiceReadyState> {
        let mut guard = self.state.lock();
        core::mem::replace(&mut *guard, Some(ServiceReadyState { name, signaled: false }))
    }

    fn mark_ready(&self) {
        let mut guard = self.state.lock();
        if let Some(state) = guard.as_mut() {
            if !state.signaled {
                state.signaled = true;
                emit_line(&format!("init: up {}", state.name));
            }
        }
    }

    fn was_signaled(&self) -> bool {
        let guard = self.state.lock();
        guard.as_ref().map_or(false, |state| state.signaled)
    }

    fn restore(&self, previous: Option<ServiceReadyState>) {
        let mut guard = self.state.lock();
        *guard = previous;
    }
}

static SERVICE_READY: ServiceReadyCell = ServiceReadyCell::new();

fn emit_service_ready() {
    SERVICE_READY.mark_ready();
}

fn start_service(name: &'static str, f: impl FnOnce() -> Result<(), ServiceError>) {
    emit_line(&format!("init: start {name}"));
    let previous = SERVICE_READY.begin(name);
    let result = f();
    match result {
        Ok(()) => {
            if !SERVICE_READY.was_signaled() {
                emit_service_ready();
            }
        }
        Err(err) => {
            emit_line(&format!("init: fail {name}: {err}"));
        }
    }
    SERVICE_READY.restore(previous);
}

impl From<keystored::ServerError> for ServiceError {
    fn from(err: keystored::ServerError) -> Self {
        Self::Detail(err.to_string())
    }
}

impl From<policyd::ServerError> for ServiceError {
    fn from(err: policyd::ServerError) -> Self {
        Self::Detail(err.to_string())
    }
}

impl From<samgrd::ServerError> for ServiceError {
    fn from(err: samgrd::ServerError) -> Self {
        Self::Detail(err.to_string())
    }
}

impl From<bundlemgrd::ServerError> for ServiceError {
    fn from(err: bundlemgrd::ServerError) -> Self {
        Self::Detail(err.to_string())
    }
}

impl From<packagefsd::LiteError> for ServiceError {
    fn from(err: packagefsd::LiteError) -> Self {
        Self::Lite(err)
    }
}

impl From<vfsd::Error> for ServiceError {
    fn from(err: vfsd::Error) -> Self {
        Self::Vfs(err)
    }
}

impl From<execd::ServerError> for ServiceError {
    fn from(err: execd::ServerError) -> Self {
        Self::Detail(err.to_string())
    }
}

impl From<nexus_ipc::IpcError> for ServiceError {
    fn from(err: nexus_ipc::IpcError) -> Self {
        Self::Ipc(err)
    }
}


/// No-op for parity with the std backend which warms schema caches.
pub fn touch_schemas() {}

/// Sequential bootstrap loop that emits stage0-compatible UART markers and
/// cooperatively yields control back to the scheduler.
pub fn service_main_loop<F>(notifier: ReadyNotifier<F>) -> Result<(), InitError>
where
    F: FnOnce() + Send,
{
    emit_line("init: start");

    set_default_target("keystored");
    start_service("keystored", || {
        keystored::service_main_loop(keystored::ReadyNotifier::new(|| emit_service_ready()))
            .map_err(ServiceError::from)
    });
    let _ = yield_();

    set_default_target("policyd");
    start_service("policyd", || {
        policyd::service_main_loop(policyd::ReadyNotifier::new(|| emit_service_ready()))
            .map_err(ServiceError::from)
    });
    let _ = yield_();

    set_default_target("samgrd");
    start_service("samgrd", || {
        samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| emit_service_ready()))
            .map_err(ServiceError::from)
    });
    let _ = yield_();

    set_default_target("bundlemgrd");
    start_service("bundlemgrd", || {
        bundlemgrd::service_main_loop(
            bundlemgrd::ReadyNotifier::new(|| emit_service_ready()),
            bundlemgrd::ArtifactStore::new(),
        )
        .map_err(ServiceError::from)
    });
    let _ = yield_();

    set_default_target("packagefsd");
    start_service("packagefsd", || {
        packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| emit_service_ready()))
            .map_err(ServiceError::from)
    });
    let _ = yield_();

    set_default_target("vfsd");
    start_service("vfsd", || {
        vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| emit_service_ready()))
            .map_err(ServiceError::from)
    });
    let _ = yield_();

    set_default_target("execd");
    start_service("execd", || {
        execd::service_main_loop(execd::ReadyNotifier::new(|| emit_service_ready()))
            .map_err(ServiceError::from)
    });
    let _ = yield_();

    notifier.notify();
    emit_line("init: ready");
    loop {
        let _ = yield_();
    }
}

fn emit_line(message: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        for b in message.as_bytes() {
            uart_write_byte(*b);
        }
        uart_write_byte(b'\n');
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn uart_write_byte(byte: u8) {
    const UART0_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;
    unsafe {
        while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        if byte == b'\n' {
            core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, b'\r');
            while core::ptr::read_volatile((UART0_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {}
        }
        core::ptr::write_volatile((UART0_BASE + UART_TX) as *mut u8, byte);
    }
}
