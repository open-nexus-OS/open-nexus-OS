#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ffi::c_void;
use core::fmt;

use bundlemgrd;
use execd;
use keystored;
use packagefsd;
use policyd;
use samgrd;
use vfsd;

use nexus_abi::{self, yield_, AbiError, IpcError, Rights};
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

#[derive(Copy, Clone, Eq, PartialEq)]
enum ReadyStatus {
    Pending,
    Ready,
    Failed,
}

impl ReadyStatus {
    fn is_signaled(self) -> bool {
        !matches!(self, ReadyStatus::Pending)
    }
}

struct ServiceReadyState {
    name: &'static str,
    status: ReadyStatus,
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
        core::mem::replace(
            &mut *guard,
            Some(ServiceReadyState { name, status: ReadyStatus::Pending }),
        )
    }

    fn mark_ready(&self) {
        let mut guard = self.state.lock();
        if let Some(state) = guard.as_mut() {
            if !state.status.is_signaled() {
                state.status = ReadyStatus::Ready;
                emit_line(&format!("init: up {}", state.name));
            }
        }
    }

    fn mark_failed(&self) {
        let mut guard = self.state.lock();
        if let Some(state) = guard.as_mut() {
            if !state.status.is_signaled() {
                state.status = ReadyStatus::Failed;
            }
        }
    }

    fn was_signaled(&self) -> bool {
        let guard = self.state.lock();
        guard
            .as_ref()
            .map_or(false, |state| state.status.is_signaled())
    }

    fn status(&self) -> Option<ReadyStatus> {
        let guard = self.state.lock();
        guard.as_ref().map(|state| state.status)
    }

    fn restore(&self, previous: Option<ServiceReadyState>) {
        let mut guard = self.state.lock();
        *guard = previous;
    }
}

static SERVICE_READY: ServiceReadyCell = ServiceReadyCell::new();

const DEFAULT_STACK_SIZE: usize = 16 * 1024;
const STACK_ALIGN: usize = 16;
const PAGE_SIZE: u64 = 4096;
const USER_STACK_TOP: u64 = 0x4000_0000;
const PROT_READ: u32 = 1 << 0;
const PROT_WRITE: u32 = 1 << 1;
const MAP_FLAG_USER: u32 = 1 << 0;
const BOOTSTRAP_SLOT: u32 = 0;

struct BootstrapSlotState {
    slot: u32,
    in_use: SpinLock<bool>,
}

impl BootstrapSlotState {
    const fn new(slot: u32) -> Self {
        Self { slot, in_use: SpinLock::new(false) }
    }

    fn claim(&self) -> Result<BootstrapLease<'_>, SpawnError> {
        let mut guard = self.in_use.lock();
        if *guard {
            return Err(SpawnError::BootstrapBusy);
        }
        *guard = true;
        Ok(BootstrapLease { state: self, released: false })
    }

    fn release(&self) {
        let mut guard = self.in_use.lock();
        *guard = false;
    }
}

struct BootstrapLease<'a> {
    state: &'a BootstrapSlotState,
    released: bool,
}

impl<'a> BootstrapLease<'a> {
    fn slot(&self) -> u32 {
        self.state.slot
    }

    fn relinquish(mut self) {
        if !self.released {
            self.state.release();
            self.released = true;
        }
    }
}

impl Drop for BootstrapLease<'_> {
    fn drop(&mut self) {
        if !self.released {
            self.state.release();
        }
    }
}

static BOOTSTRAP_SLOT_STATE: BootstrapSlotState = BootstrapSlotState::new(BOOTSTRAP_SLOT);

type ServiceRunner = fn() -> Result<(), ServiceError>;

struct ServiceMetadata {
    name: &'static str,
    target: &'static str,
    runner: ServiceRunner,
    stack_size: Option<usize>,
}

struct ServiceRuntime {
    name: &'static str,
    target: &'static str,
    runner: ServiceRunner,
}

impl ServiceRuntime {
    const fn new(name: &'static str, target: &'static str, runner: ServiceRunner) -> Self {
        Self { name, target, runner }
    }
}

struct ServiceRuntimeQueue {
    entries: SpinLock<Vec<*mut ServiceRuntime>>,
}

impl ServiceRuntimeQueue {
    const fn new() -> Self {
        Self { entries: SpinLock::new(Vec::new()) }
    }

    fn push(&self, runtime: *mut ServiceRuntime) {
        let mut guard = self.entries.lock();
        guard.push(runtime);
    }

    fn pop_front(&self) -> Option<*mut ServiceRuntime> {
        let mut guard = self.entries.lock();
        if guard.is_empty() {
            None
        } else {
            Some(guard.remove(0))
        }
    }

    fn remove(&self, runtime: *mut ServiceRuntime) -> bool {
        let mut guard = self.entries.lock();
        if let Some(index) = guard.iter().position(|&ptr| ptr == runtime) {
            guard.remove(index);
            true
        } else {
            false
        }
    }
}

static SERVICE_RUNTIMES: ServiceRuntimeQueue = ServiceRuntimeQueue::new();

struct RuntimeRegistration {
    runtime: *mut ServiceRuntime,
    released: bool,
}

impl RuntimeRegistration {
    fn new(runtime: *mut ServiceRuntime) -> Self {
        Self { runtime, released: false }
    }

    fn release(&mut self) {
        if !self.released {
            if SERVICE_RUNTIMES.remove(self.runtime) {
                unsafe {
                    drop(Box::from_raw(self.runtime));
                }
            }
            self.released = true;
        }
    }

    fn commit(mut self) {
        self.released = true;
        core::mem::forget(self);
    }
}

impl Drop for RuntimeRegistration {
    fn drop(&mut self) {
        if !self.released {
            if SERVICE_RUNTIMES.remove(self.runtime) {
                unsafe {
                    drop(Box::from_raw(self.runtime));
                }
            }
        }
    }
}

struct StackMapping {
    base: u64,
    len: u64,
    vmo: Option<nexus_abi::Handle>,
}

impl StackMapping {
    fn new(handle: nexus_abi::AsHandle, size: usize) -> Result<Self, SpawnError> {
        if size == 0 {
            return Err(SpawnError::StackSize);
        }

        let aligned = align_stack_len(size)?;
        let base = USER_STACK_TOP
            .checked_sub(aligned)
            .ok_or(SpawnError::StackSize)?;

        let vmo = nexus_abi::vmo_create(aligned as usize).map_err(SpawnError::StackVmo)?;
        nexus_abi::as_map(
            handle,
            vmo,
            base,
            aligned,
            PROT_READ | PROT_WRITE,
            MAP_FLAG_USER,
        )
        .map_err(SpawnError::StackMap)?;

        Ok(Self { base, len: aligned, vmo: Some(vmo) })
    }

    fn stack_top(&self) -> u64 {
        self.base + self.len
    }

    fn release(&mut self) {
        // Drop the parent's reference to the stack VMO. The underlying kernel object
        // stays mapped in the child task. Once kernel-side destruction hooks land, the
        // `vmo_destroy` syscall will also tear down the allocation. Until then we
        // ignore unsupported/invalid-syscall errors so the teardown path remains
        // idempotent.
        if let Some(vmo) = self.vmo.take() {
            if let Err(err) = nexus_abi::vmo_destroy(vmo) {
                if !matches!(err, AbiError::InvalidSyscall | AbiError::Unsupported) {
                    emit_line(&format!(
                        "init: warn stack vmo destroy failed: {:?}",
                        err
                    ));
                }
            }
        }
        self.base = 0;
        self.len = 0;
    }
}

struct AddressSpaceLease {
    handle: nexus_abi::AsHandle,
    stack: StackMapping,
}

impl AddressSpaceLease {
    fn new(stack_size: usize) -> Result<Self, SpawnError> {
        let handle = nexus_abi::as_create().map_err(SpawnError::AddressSpace)?;
        let stack = StackMapping::new(handle, stack_size)?;
        Ok(Self { handle, stack })
    }

    fn stack_top(&self) -> u64 {
        self.stack.stack_top()
    }

    fn raw(&self) -> u64 {
        self.handle
    }

    fn into_parts(self) -> (nexus_abi::AsHandle, StackMapping) {
        let AddressSpaceLease { handle, stack } = self;
        (handle, stack)
    }
}

fn align_stack_len(size: usize) -> Result<u64, SpawnError> {
    let size_u64 = size as u64;
    let aligned = size_u64
        .checked_add(PAGE_SIZE - 1)
        .map(|value| value & !(PAGE_SIZE - 1))
        .ok_or(SpawnError::StackSize)?;
    if aligned == 0 {
        return Err(SpawnError::StackSize);
    }
    Ok(aligned)
}

#[derive(Debug)]
pub struct SpawnHandle {
    pid: nexus_abi::Pid,
    address_space: Option<nexus_abi::AsHandle>,
    stack: StackMapping,
}

impl SpawnHandle {
    fn new(pid: nexus_abi::Pid, address_space: nexus_abi::AsHandle, stack: StackMapping) -> Self {
        Self { pid, address_space: Some(address_space), stack }
    }

    #[allow(dead_code)]
    fn pid(&self) -> nexus_abi::Pid {
        self.pid
    }
}

impl Drop for SpawnHandle {
    fn drop(&mut self) {
        self.stack.release();
        if let Some(handle) = self.address_space.take() {
            release_address_space(handle);
        }
    }
}

#[derive(Debug)]
enum SpawnError {
    StackSize,
    AddressSpace(AbiError),
    StackVmo(IpcError),
    StackMap(AbiError),
    Spawn(nexus_abi::AbiError),
    CapTransfer(nexus_abi::AbiError),
    Yield(nexus_abi::AbiError),
    ServiceFailed,
    BootstrapBusy,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StackSize => write!(f, "invalid stack configuration"),
            Self::Spawn(err) => write!(f, "spawn failed: {err:?}"),
            Self::CapTransfer(err) => write!(f, "cap transfer failed: {err:?}"),
            Self::Yield(err) => write!(f, "yield failed: {err:?}"),
            Self::AddressSpace(err) => write!(f, "address space failed: {err:?}"),
            Self::StackVmo(err) => write!(f, "stack allocation failed: {err:?}"),
            Self::StackMap(err) => write!(f, "stack map failed: {err:?}"),
            Self::ServiceFailed => write!(f, "service exited before readiness"),
            Self::BootstrapBusy => write!(f, "bootstrap slot busy"),
        }
    }
}

fn spawn_service(metadata: &ServiceMetadata) -> Result<SpawnHandle, SpawnError> {
    set_default_target(metadata.target);

    let bootstrap = BOOTSTRAP_SLOT_STATE.claim()?;
    let stack_size = metadata.stack_size.unwrap_or(DEFAULT_STACK_SIZE);
    let address_space = AddressSpaceLease::new(stack_size)?;
    let align_mask = (STACK_ALIGN as u64).saturating_sub(1);
    let stack_sp = address_space.stack_top() & !align_mask;
    let runtime = Box::into_raw(Box::new(ServiceRuntime::new(
        metadata.name,
        metadata.target,
        metadata.runner,
    )));
    SERVICE_RUNTIMES.push(runtime);
    let mut registration = RuntimeRegistration::new(runtime);
    let entry_pc = service_task_entry as usize as u64;

    let pid = match nexus_abi::spawn(entry_pc, stack_sp, address_space.raw(), bootstrap.slot()) {
        Ok(pid) => pid,
        Err(err) => {
            registration.release();
            return Err(SpawnError::Spawn(err));
        }
    };

    if let Err(err) = nexus_abi::cap_transfer(pid, bootstrap.slot(), Rights::SEND) {
        registration.release();
        return Err(SpawnError::CapTransfer(err));
    }

    bootstrap.relinquish();

    while !SERVICE_READY.was_signaled() {
        match yield_() {
            Ok(_) => {}
            Err(err) => {
                registration.release();
                return Err(SpawnError::Yield(err));
            }
        }
    }

    let readiness = SERVICE_READY.status();
    if readiness != Some(ReadyStatus::Ready) {
        registration.release();
        return Err(SpawnError::ServiceFailed);
    }

    registration.commit();

    let (address_space_handle, stack) = address_space.into_parts();

    Ok(SpawnHandle::new(pid, address_space_handle, stack))
}

extern "C" fn service_task_entry(_bootstrap: *const c_void) -> ! {
    if let Some(ptr) = SERVICE_RUNTIMES.pop_front() {
        let runtime = unsafe { Box::from_raw(ptr) };
        let name = runtime.name;
        let target = runtime.target;
        let runner = runtime.runner;
        drop(runtime);
        set_default_target(target);
        run_service_task(name, runner)
    } else {
        SERVICE_READY.mark_failed();
        emit_line("init: fail service: bootstrap mismatch");
        loop {
            let _ = yield_();
        }
    }
}

fn emit_service_ready() {
    SERVICE_READY.mark_ready();
}

fn start_spawned_service(metadata: ServiceMetadata) -> Option<SpawnHandle> {
    emit_line(&format!("init: start {}", metadata.name));
    let previous = SERVICE_READY.begin(metadata.name);
    let result = spawn_service(&metadata);
    let outcome = match result {
        Ok(handle) => {
            if !SERVICE_READY.was_signaled() {
                emit_service_ready();
            }
            Some(handle)
        }
        Err(err) => {
            emit_line(&format!("init: fail {}: {err}", metadata.name));
            None
        }
    };
    SERVICE_READY.restore(previous);
    outcome
}

fn run_service_task(name: &'static str, runner: ServiceRunner) -> ! {
    if let Err(err) = runner() {
        SERVICE_READY.mark_failed();
        emit_line(&format!("init: fail {name}: {err}"));
    }
    loop {
        let _ = yield_();
    }
}

fn run_keystored() -> Result<(), ServiceError> {
    keystored::service_main_loop(keystored::ReadyNotifier::new(|| emit_service_ready()))
        .map_err(ServiceError::from)
}

fn run_policyd() -> Result<(), ServiceError> {
    policyd::service_main_loop(policyd::ReadyNotifier::new(|| emit_service_ready()))
        .map_err(ServiceError::from)
}

fn run_samgrd() -> Result<(), ServiceError> {
    samgrd::service_main_loop(samgrd::ReadyNotifier::new(|| emit_service_ready()))
        .map_err(ServiceError::from)
}

fn run_bundlemgrd() -> Result<(), ServiceError> {
    bundlemgrd::service_main_loop(
        bundlemgrd::ReadyNotifier::new(|| emit_service_ready()),
        bundlemgrd::ArtifactStore::new(),
    )
    .map_err(ServiceError::from)
}

fn run_packagefsd() -> Result<(), ServiceError> {
    packagefsd::service_main_loop(packagefsd::ReadyNotifier::new(|| emit_service_ready()))
        .map_err(ServiceError::from)
}

fn run_vfsd() -> Result<(), ServiceError> {
    vfsd::service_main_loop(vfsd::ReadyNotifier::new(|| emit_service_ready()))
        .map_err(ServiceError::from)
}

fn run_execd() -> Result<(), ServiceError> {
    execd::service_main_loop(execd::ReadyNotifier::new(|| emit_service_ready()))
        .map_err(ServiceError::from)
}

fn release_address_space(_handle: nexus_abi::AsHandle) {
    if let Err(err) = nexus_abi::as_destroy(_handle) {
        if !matches!(err, AbiError::InvalidSyscall | AbiError::Unsupported) {
            emit_line(&format!("init: warn as destroy failed: {:?}", err));
        }
    }
}

fn teardown_services(handles: Vec<SpawnHandle>) {
    handles.into_iter().for_each(drop);
    release_bootstrap_capability();
}

fn release_bootstrap_capability() {
    if let Err(err) = nexus_abi::cap_close(BOOTSTRAP_SLOT) {
        if !matches!(err, AbiError::InvalidSyscall | AbiError::Unsupported | AbiError::CapabilityDenied) {
            emit_line(&format!("init: warn bootstrap drop failed: {:?}", err));
        }
    }
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

    let mut spawned: Vec<SpawnHandle> = Vec::new();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "keystored",
        target: "keystored",
        runner: run_keystored,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "policyd",
        target: "policyd",
        runner: run_policyd,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "samgrd",
        target: "samgrd",
        runner: run_samgrd,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "bundlemgrd",
        target: "bundlemgrd",
        runner: run_bundlemgrd,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "packagefsd",
        target: "packagefsd",
        runner: run_packagefsd,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "vfsd",
        target: "vfsd",
        runner: run_vfsd,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    if let Some(handle) = start_spawned_service(ServiceMetadata {
        name: "execd",
        target: "execd",
        runner: run_execd,
        stack_size: None,
    }) {
        spawned.push(handle);
    }
    let _ = yield_();

    // Release parent's references to per-service resources and bootstrap slot.
    teardown_services(spawned);
    notifier.notify();
    emit_line("init: ready");

    teardown_services(spawned);

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
