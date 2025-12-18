extern crate alloc;

use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(nexus_env = "os")]
use nexus_abi::{self, AbiError, IpcError, Rights};

// Tooling/host diagnostics compatibility:
// `os_payload` is OS-only (selected by `lib.rs`), but rust-analyzer may still parse this file
// under a host `cfg` set. Provide minimal stubs so diagnostics don't fail the workspace.
#[cfg(not(nexus_env = "os"))]
mod abi_compat {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum AbiError {
        Unsupported,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum IpcError {
        Unsupported,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Rights(pub u32);

    impl Rights {
        pub const SEND: Rights = Rights(1 << 0);
        pub const RECV: Rights = Rights(1 << 1);
    }

    impl core::ops::BitOr for Rights {
        type Output = Rights;
        fn bitor(self, rhs: Rights) -> Rights {
            Rights(self.0 | rhs.0)
        }
    }

    pub type SysResult<T> = core::result::Result<T, AbiError>;

    pub fn yield_() -> SysResult<()> {
        Ok(())
    }

    pub fn exec(_elf: &[u8], _stack_pages: usize, _global_pointer: u64) -> SysResult<u32> {
        Err(AbiError::Unsupported)
    }

pub fn exec_v2(
    _elf: &[u8],
    _stack_pages: usize,
    _global_pointer: u64,
    _service_name: &str,
) -> SysResult<u32> {
    Err(AbiError::Unsupported)
}

    pub fn cap_transfer(_pid: u32, _slot: u32, _rights: Rights) -> SysResult<()> {
        Err(AbiError::Unsupported)
    }

    pub fn ipc_endpoint_create(_queue_depth: usize) -> SysResult<u32> {
        Err(AbiError::Unsupported)
    }

    pub fn debug_putc(_byte: u8) -> SysResult<()> {
        Ok(())
    }

    pub fn debug_write(_bytes: &[u8]) -> SysResult<()> {
        Ok(())
    }

    pub fn debug_println(_s: &str) -> SysResult<()> {
        Ok(())
    }
}

#[cfg(not(nexus_env = "os"))]
use abi_compat as nexus_abi;
#[cfg(not(nexus_env = "os"))]
use abi_compat::{AbiError, IpcError, Rights};
use nexus_log::{self, LineBuilder, StrRef};

const MAX_LOG_STR_LEN: usize = 512;

extern "C" {
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __data_end: u8;
    static __small_data_guard: u8;
    static __image_end: u8;
}

/// Prepackaged service image embedded into the init payload.
pub struct ServiceImage {
    /// Logical service name used for logging.
    pub name: &'static str,
    /// Raw ELF bytes for the service.
    pub elf: &'static [u8],
    /// Number of stack pages to allocate for the service.
    pub stack_pages: u64,
    /// RISC-V global pointer used when spawning the task.
    pub global_pointer: u64,
}

/// Errors produced while materialising service images.
#[derive(Debug, Clone)]
pub enum InitError {
    /// Kernel ABI call failed.
    Abi(AbiError),
    /// IPC syscall failed.
    Ipc(IpcError),
    /// Malformed ELF payload.
    Elf(&'static str),
    /// Segment mapping violation (overflow, overlap, etc.).
    Map(&'static str),
    /// Requested service carried an empty image.
    MissingElf,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::Abi(code) => write!(f, "abi error: {:?}", code),
            InitError::Ipc(code) => write!(f, "ipc error: {:?}", code),
            InitError::Elf(msg) => write!(f, "elf error: {msg}"),
            InitError::Map(msg) => write!(f, "map error: {msg}"),
            InitError::MissingElf => write!(f, "no elf payload"),
        }
    }
}

/// Callback invoked once the bootstrapper reaches `init: ready`.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Create a new notifier that will run `func` when signalled.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Execute the wrapped callback.
    pub fn notify(self) {
        (self.0)();
    }
}

mod log_topics {
    use nexus_log::{Topic, TOPIC_GENERAL};

    pub const GENERAL: Topic = TOPIC_GENERAL;
    pub const SERVICE_META: Topic = Topic::bit(1);
    pub const PROBE: Topic = Topic::bit(2);

    pub const DEFAULT_MASK: Topic = GENERAL;

    fn matches_general(slice: &[u8]) -> bool {
        slice.len() == 7
            && slice[0] == b'g'
            && slice[1] == b'e'
            && slice[2] == b'n'
            && slice[3] == b'e'
            && slice[4] == b'r'
            && slice[5] == b'a'
            && slice[6] == b'l'
    }

    fn matches_svc_meta(slice: &[u8]) -> bool {
        slice.len() == 8
            && slice[0] == b's'
            && slice[1] == b'v'
            && slice[2] == b'c'
            && slice[3] == b'-'
            && slice[4] == b'm'
            && slice[5] == b'e'
            && slice[6] == b't'
            && slice[7] == b'a'
    }

    fn matches_probe(slice: &[u8]) -> bool {
        slice.eq_ignore_ascii_case(b"probe")
    }

    pub fn parse_spec(spec: &[u8]) -> Topic {
        fn apply_token(mask: &mut Topic, bytes: &[u8], start: usize, end: usize) {
            let mut lo = start;
            let mut hi = end;
            while lo < hi && bytes[lo].is_ascii_whitespace() {
                lo += 1;
            }
            while hi > lo && bytes[hi - 1].is_ascii_whitespace() {
                hi -= 1;
            }
            if lo == hi {
                return;
            }
            let slice = &bytes[lo..hi];
            if matches_general(slice) {
                *mask |= GENERAL;
                return;
            }
            if matches_svc_meta(slice) {
                *mask |= SERVICE_META;
                return;
            }
            if matches_probe(slice) {
                *mask |= PROBE;
            }
        }

        let mut mask = Topic::empty();
        let mut start = 0usize;
        for (idx, &byte) in spec.iter().enumerate() {
            if byte == b',' {
                apply_token(&mut mask, spec, start, idx);
                start = idx + 1;
            }
        }
        apply_token(&mut mask, spec, start, spec.len());

        if mask.is_empty() {
            DEFAULT_MASK
        } else {
            mask | GENERAL
        }
    }
}

type Result<T> = core::result::Result<T, InitError>;

static PROBE_ENABLED: AtomicBool = AtomicBool::new(false);

// RFC-0005: per-service bootstrap routing protocol (init-lite responder over a private control EP).
const CTRL_EP_DEPTH: usize = 4;
const CTRL_CHILD_SEND_SLOT: u32 = 1; // First cap_transfer into a freshly spawned task (slot 0 is reserved).
const CTRL_CHILD_RECV_SLOT: u32 = 2; // Second cap_transfer (paired reply endpoint).
const ROUTE_GET: u8 = 0x40;
const ROUTE_RSP: u8 = 0x41;

#[derive(Clone, Copy)]
struct CtrlChannel {
    /// Service PID owning the control endpoint in child-space.
    pid: u32,
    /// Capability slot in init-lite that references the request endpoint (child sends, init receives).
    ctrl_req_parent_slot: u32,
    /// Capability slot in init-lite that references the reply endpoint (init sends, child receives).
    ctrl_rsp_parent_slot: u32,
    /// Optional routing for target "vfsd" from the perspective of this PID:
    /// - send_slot: where this PID should send requests/replies
    /// - recv_slot: where this PID should receive replies/requests
    vfs_send_slot: Option<u32>,
    vfs_recv_slot: Option<u32>,
    pkg_send_slot: Option<u32>,
    pkg_recv_slot: Option<u32>,
    pol_send_slot: Option<u32>,
    pol_recv_slot: Option<u32>,
    bnd_send_slot: Option<u32>,
    bnd_recv_slot: Option<u32>,
    sam_send_slot: Option<u32>,
    sam_recv_slot: Option<u32>,
    exe_send_slot: Option<u32>,
    exe_recv_slot: Option<u32>,
    key_send_slot: Option<u32>,
    key_recv_slot: Option<u32>,
}

/// Optional bring-up watchdog to force a panic if init spins forever.
fn watchdog_limit_ticks() -> Option<usize> {
    match option_env!("INIT_LITE_WATCHDOG_TICKS") {
        Some(val) if !val.is_empty() => usize::from_str_radix(val, 10).ok(),
        _ => None,
    }
}

/// Emit a fatal marker and trap so hangs/errors are visible in UART logs.
fn fatal(msg: &str) -> ! {
    debug_write_bytes(b"!fatal ");
    debug_write_str(msg);
    debug_write_byte(b'\n');
    nexus_log::error("init", |line| {
        line.text(msg);
    });
    panic!("{}", msg);
}

/// Log a fatal init error and abort the init task.
pub fn fatal_err(err: InitError) -> ! {
    debug_write_bytes(b"!fatal-err ");
    match err {
        InitError::Abi(code) => {
            debug_write_str("abi:");
            debug_write_str(abi_error_label(code.clone()));
        }
        InitError::Ipc(code) => {
            debug_write_str("ipc:");
            debug_write_str(ipc_error_label(code.clone()));
        }
        InitError::Elf(msg) => {
            debug_write_str("elf:");
            debug_write_str(msg);
        }
        InitError::Map(msg) => {
            debug_write_str("map:");
            debug_write_str(msg);
        }
        InitError::MissingElf => debug_write_str("missing-elf"),
    }
    debug_write_byte(b'\n');
    nexus_log::error("init", |line| {
        line.text("fatal err=");
        describe_init_error(line, &err);
    });
    fatal("init-lite fatal");
}

fn configure_log_topics() {
    let mask = match option_env!("INIT_LITE_LOG_TOPICS") {
        Some(spec) if !spec.is_empty() => log_topics::parse_spec(spec.as_bytes()),
        _ => log_topics::DEFAULT_MASK,
    };
    nexus_log::set_topic_mask(mask);
    let mut probe = (mask.bits() & log_topics::PROBE.bits()) != 0;
    if option_env!("INIT_LITE_FORCE_PROBE") == Some("1") {
        probe = true;
        debug_write_bytes(b"probe override active\n");
    }
    PROBE_ENABLED.store(probe, Ordering::Relaxed);
    debug_write_bytes(b"log topics mask=0x");
    debug_write_hex(mask.bits() as usize);
    debug_write_byte(b'\n');
}

fn probes_enabled() -> bool {
    PROBE_ENABLED.load(Ordering::Relaxed)
}

const GUARD_STR_PROBE_LIMIT: usize = 128;
static GUARD_STR_PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);

fn debug_write_byte(byte: u8) {
    let _ = nexus_abi::debug_putc(byte);
}

fn debug_write_bytes(bytes: &[u8]) {
    for &b in bytes {
        debug_write_byte(b);
    }
}

fn debug_write_str(s: &str) {
    debug_write_bytes(s.as_bytes());
}

fn debug_write_hex(value: usize) {
    const NIBBLES: usize = core::mem::size_of::<usize>() * 2;
    for shift in (0..NIBBLES).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + (nibble - 10)
        };
        debug_write_byte(ch);
    }
}

fn probe_debug_write_words() {
    if !probes_enabled() {
        return;
    }
    const PROBE_WORDS: usize = 4;
    let base = nexus_abi::debug_write as usize;
    debug_write_bytes(b"!dbg-probe base=0x");
    debug_write_hex(base);
    debug_write_byte(b'\n');
    unsafe {
        for idx in 0..PROBE_WORDS {
            let ptr = (base + idx * core::mem::size_of::<u32>()) as *const u32;
            let word = core::ptr::read_unaligned(ptr) as usize;
            debug_write_bytes(b"!dbg-word idx=0x");
            debug_write_hex(idx);
            debug_write_bytes(b" val=0x");
            debug_write_hex(word);
            debug_write_byte(b'\n');
        }
    }
}

fn raw_probe_str(tag: &str, value: &str) {
    if !probes_enabled() {
        return;
    }
    // Probe output must stay extremely robust (no long hex dumps) so it doesn't
    // perturb boot timing or trigger truncation under UART capture.
    let _ = value; // keep signature stable for future richer probes
    debug_write_byte(b'^');
    debug_write_str(tag);
    debug_write_byte(b'\n');
}

fn log_str_ptr(tag: &str, value: &str) {
    raw_probe_str(tag, value);
    nexus_log::trace_topic("init", log_topics::SERVICE_META, |line| {
        line.text_ref(StrRef::from(tag));
        line.text(" ptr=");
        line.hex(value.as_ptr() as u64);
        line.text(" len=");
        line.dec(value.len() as u64);
    });
}

fn trace_guard_str(event: &str, ptr: usize, len: usize, truncated: bool) {
    if !probes_enabled() {
        return;
    }
    // Keep probe output minimal and robust: no long hex prints during boot.
    if GUARD_STR_PROBE_COUNT.fetch_add(1, Ordering::Relaxed) >= GUARD_STR_PROBE_LIMIT {
        return;
    }
    debug_write_bytes(b"!guard ");
    debug_write_str(event);
    if truncated {
        debug_write_bytes(b" trunc");
    }
    debug_write_bytes(b" ptr=0x");
    debug_write_hex(ptr);
    debug_write_bytes(b" len=0x");
    debug_write_hex(len);
    debug_write_byte(b'\n');
}

fn section_range(start: &u8, end: &u8) -> core::ops::Range<usize> {
    let base = start as *const u8 as usize;
    let end = end as *const u8 as usize;
    base..end
}

fn section_contains(range: &core::ops::Range<usize>, ptr: usize, len: usize) -> bool {
    if range.is_empty() {
        return false;
    }
    let end = match ptr.checked_add(len) {
        Some(end) => end,
        None => return false,
    };
    ptr >= range.start && end <= range.end
}

fn is_user_str_valid(ptr: usize, len: usize) -> bool {
    if len == 0 || len > MAX_LOG_STR_LEN {
        return false;
    }
    let ro_range = unsafe { section_range(&__rodata_start, &__rodata_end) };
    let data_range = unsafe { section_range(&__data_start, &__data_end) };
    section_contains(&ro_range, ptr, len) || section_contains(&data_range, ptr, len)
}

struct ServiceNameGuard<'a> {
    value: Option<&'a str>,
    ptr: usize,
    len: usize,
}

impl<'a> ServiceNameGuard<'a> {
    fn new(raw: &'a str) -> Self {
        let ptr = raw.as_ptr() as usize;
        let len = raw.len();
        let value = if is_user_str_valid(ptr, len) {
            trace_guard_str("svc-name", ptr, len, false);
            Some(raw)
        } else {
            trace_guard_str("svc-name-invalid", ptr, len, false);
            None
        };
        Self { value, ptr, len }
    }

    fn trace_metadata(&self) {
        if !probes_enabled() {
            return;
        }
        debug_write_bytes(b"!svc-meta\n");
    }
}

/// Map, zero, and spawn every service image once, signalling `notifier` on completion.
pub fn bootstrap_service_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<Vec<CtrlChannel>>
where
    F: FnOnce() + Send,
{
    debug_write_bytes(b"!init-lite entry\n");
    debug_write_str("init: entry");
    debug_write_byte(b'\n');
    probe_debug_write_words();
    configure_log_topics();
    log_str_ptr("init-msg", "init: start");
    debug_write_str("init: start");
    debug_write_byte(b'\n');
    if probes_enabled() {
        debug_write_bytes(b"!images\n");
    }

    if images.is_empty() {
        debug_write_str("init: warn no services configured");
        debug_write_byte(b'\n');
    }

    // RFC-0005: Service IPC capability distribution (minimal VFS wiring).
    //
    // Create one request endpoint and one response endpoint for selftest-client <-> vfsd.
    // Each spawned process receives the bootstrap cap in slot 0; these transfers will land in
    // slot 1 and slot 2 deterministically (CapTable::allocate).
    let vfs_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let vfs_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    // Create one request endpoint and one response endpoint for selftest-client <-> packagefsd.
    let pkg_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let pkg_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    // Create one request endpoint and one response endpoint for selftest-client <-> policyd.
    let pol_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let pol_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    // Create one request endpoint and one response endpoint for selftest-client <-> bundlemgrd.
    let bnd_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let bnd_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    // Create one request endpoint and one response endpoint for selftest-client <-> samgrd.
    let sam_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let sam_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    // Create one request endpoint and one response endpoint for selftest-client <-> execd.
    let exe_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let exe_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    // Create one request endpoint and one response endpoint for selftest-client <-> keystored.
    let key_req = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;
    let key_rsp = nexus_abi::ipc_endpoint_create(8).map_err(InitError::Abi)?;

    let mut ctrl_channels: Vec<CtrlChannel> = Vec::new();
    for (_idx, image) in images.iter().enumerate() {
        if probes_enabled() {
            debug_write_bytes(b"!svc-loop\n");
        }
        let name = ServiceNameGuard::new(image.name);
        if probes_enabled() {
            // Keep probe-only pointer diagnostics out of nexus_log to avoid boot-time coupling.
            raw_probe_str("svc-name", image.name);
        }
        name.trace_metadata();
        debug_write_str("init: start ");
        if let Some(value) = name.value {
            debug_write_str(value);
        } else {
            debug_write_str("[svc@0x");
            debug_write_hex(name.ptr);
            debug_write_str("/");
            debug_write_hex(name.len);
            debug_write_byte(b']');
        }
        debug_write_byte(b'\n');
        match spawn_service(image, &name) {
            Ok(pid) => {
                // Create private control endpoints (REQ/RSP) for this service and transfer them first.
                // This ensures a deterministic slot assignment in the child (slots 1 and 2).
                let ctrl_req_parent_slot =
                    nexus_abi::ipc_endpoint_create(CTRL_EP_DEPTH).map_err(InitError::Abi)?;
                let ctrl_rsp_parent_slot =
                    nexus_abi::ipc_endpoint_create(CTRL_EP_DEPTH).map_err(InitError::Abi)?;
                let child_send_slot =
                    nexus_abi::cap_transfer(pid, ctrl_req_parent_slot, Rights::SEND)
                        .map_err(InitError::Abi)?;
                let child_recv_slot =
                    nexus_abi::cap_transfer(pid, ctrl_rsp_parent_slot, Rights::RECV)
                        .map_err(InitError::Abi)?;
                if probes_enabled()
                    && (child_send_slot != CTRL_CHILD_SEND_SLOT || child_recv_slot != CTRL_CHILD_RECV_SLOT)
                {
                    debug_write_bytes(b"!route-warn ctrl-child-slots send=0x");
                    debug_write_hex(child_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(child_recv_slot as usize);
                    debug_write_bytes(b" expected send=0x");
                    debug_write_hex(CTRL_CHILD_SEND_SLOT as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(CTRL_CHILD_RECV_SLOT as usize);
                    debug_write_byte(b'\n');
                }

                let mut ctrl = CtrlChannel {
                    pid,
                    ctrl_req_parent_slot,
                    ctrl_rsp_parent_slot,
                    vfs_send_slot: None,
                    vfs_recv_slot: None,
                    pkg_send_slot: None,
                    pkg_recv_slot: None,
                    pol_send_slot: None,
                    pol_recv_slot: None,
                    bnd_send_slot: None,
                    bnd_recv_slot: None,
                    sam_send_slot: None,
                    sam_recv_slot: None,
                    exe_send_slot: None,
                    exe_recv_slot: None,
                    key_send_slot: None,
                    key_recv_slot: None,
                };

                // Minimal wiring for VFS E2E over kernel IPC:
                // - vfsd receives requests and sends replies
                // - selftest-client sends requests and receives replies
                if let Some(svc) = name.value {
                    if svc == "vfsd" {
                        let recv_slot = nexus_abi::cap_transfer(pid, vfs_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, vfs_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.vfs_send_slot = Some(send_slot);
                        ctrl.vfs_recv_slot = Some(recv_slot);
                    } else if svc == "packagefsd" {
                        let recv_slot = nexus_abi::cap_transfer(pid, pkg_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, pkg_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.pkg_send_slot = Some(send_slot);
                        ctrl.pkg_recv_slot = Some(recv_slot);
                    } else if svc == "selftest-client" {
                        let send_slot = nexus_abi::cap_transfer(pid, vfs_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, vfs_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.vfs_send_slot = Some(send_slot);
                        ctrl.vfs_recv_slot = Some(recv_slot);
                        let send_slot = nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.pkg_send_slot = Some(send_slot);
                        ctrl.pkg_recv_slot = Some(recv_slot);
                        let send_slot = nexus_abi::cap_transfer(pid, pol_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, pol_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.pol_send_slot = Some(send_slot);
                        ctrl.pol_recv_slot = Some(recv_slot);
                        let send_slot = nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, bnd_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.bnd_send_slot = Some(send_slot);
                        ctrl.bnd_recv_slot = Some(recv_slot);
                        let send_slot = nexus_abi::cap_transfer(pid, sam_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, sam_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.sam_send_slot = Some(send_slot);
                        ctrl.sam_recv_slot = Some(recv_slot);
                        let send_slot = nexus_abi::cap_transfer(pid, exe_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, exe_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.exe_send_slot = Some(send_slot);
                        ctrl.exe_recv_slot = Some(recv_slot);
                        let send_slot = nexus_abi::cap_transfer(pid, key_req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        let recv_slot = nexus_abi::cap_transfer(pid, key_rsp, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        ctrl.key_send_slot = Some(send_slot);
                        ctrl.key_recv_slot = Some(recv_slot);
                    }
                    if svc == "policyd" {
                        let recv_slot = nexus_abi::cap_transfer(pid, pol_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, pol_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.pol_send_slot = Some(send_slot);
                        ctrl.pol_recv_slot = Some(recv_slot);
                    }
                    if svc == "bundlemgrd" {
                        let recv_slot = nexus_abi::cap_transfer(pid, bnd_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, bnd_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.bnd_send_slot = Some(send_slot);
                        ctrl.bnd_recv_slot = Some(recv_slot);
                    }
                    if svc == "samgrd" {
                        let recv_slot = nexus_abi::cap_transfer(pid, sam_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, sam_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.sam_send_slot = Some(send_slot);
                        ctrl.sam_recv_slot = Some(recv_slot);
                    }
                    if svc == "execd" {
                        let recv_slot = nexus_abi::cap_transfer(pid, exe_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, exe_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.exe_send_slot = Some(send_slot);
                        ctrl.exe_recv_slot = Some(recv_slot);
                    }
                    if svc == "keystored" {
                        let recv_slot = nexus_abi::cap_transfer(pid, key_req, Rights::RECV)
                            .map_err(InitError::Abi)?;
                        let send_slot = nexus_abi::cap_transfer(pid, key_rsp, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        ctrl.key_send_slot = Some(send_slot);
                        ctrl.key_recv_slot = Some(recv_slot);
                    }
                }
                ctrl_channels.push(ctrl);
                if probes_enabled() {
                    debug_write_bytes(b"!spawn ok pid=0x");
                    debug_write_hex(pid as usize);
                    debug_write_byte(b'\n');
                }
                debug_write_str("init: up ");
                if let Some(value) = name.value {
                    debug_write_str(value);
                } else {
                    debug_write_str("[svc@0x");
                    debug_write_hex(name.ptr);
                    debug_write_str("/");
                    debug_write_hex(name.len);
                    debug_write_byte(b']');
                }
                debug_write_byte(b'\n');
            }
            Err(err) => {
                debug_write_str("init: fail ");
                if let Some(value) = name.value {
                    debug_write_str(value);
                } else {
                    debug_write_str("[svc@0x");
                    debug_write_hex(name.ptr);
                    debug_write_str("/");
                    debug_write_hex(name.len);
                    debug_write_byte(b']');
                }
                debug_write_str(" err=");
                // Minimal reason tag for UART; richer info stays in fatal_err.
                match &err {
                    InitError::Abi(_) => debug_write_str("abi"),
                    InitError::Ipc(_) => debug_write_str("ipc"),
                    InitError::Elf(_) => debug_write_str("elf"),
                    InitError::Map(_) => debug_write_str("map"),
                    InitError::MissingElf => debug_write_str("missing-elf"),
                }
                debug_write_byte(b'\n');
                fatal_err(err);
            }
        }
        // Yielding here is helpful for cooperative bring-up, but it can also mask
        // scheduler/AS-switching issues by jumping into the newly spawned task mid-print.
        // Keep the default bring-up deterministic: spawn the full set first, then yield.
    }

    notifier.notify();
    debug_write_str("init: ready");
    debug_write_byte(b'\n');
    debug_write_bytes(b"!init-lite ready\n");
    let _ = nexus_abi::yield_();
    Ok(ctrl_channels)
}

/// Same as [`bootstrap_service_images`] but keeps the init task alive forever.
pub fn service_main_loop_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<()>
where
    F: FnOnce() + Send,
{
    let ctrl_channels = bootstrap_service_images(images, notifier)?;
    let watchdog = watchdog_limit_ticks();
    let mut ticks: usize = 0;
    loop {
        // RFC-0005: routing responder loop (per-service private control endpoints).
        // Services query init-lite to learn which capability slots were assigned for a target.
        for chan in &ctrl_channels {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 64];
            let n = match nexus_abi::ipc_recv_v1(
                chan.ctrl_req_parent_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => n as usize,
                Err(nexus_abi::IpcError::QueueEmpty) => continue,
                Err(_) => continue,
            };
            if n < 2 {
                continue;
            }
            if buf[0] != ROUTE_GET {
                continue;
            }
            let name_len = buf[1] as usize;
            if 2 + name_len > n {
                continue;
            }
            let name = &buf[2..2 + name_len];
            let (status, send_slot, recv_slot) = if name == b"vfsd" {
                match (chan.vfs_send_slot, chan.vfs_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else if name == b"packagefsd" {
                match (chan.pkg_send_slot, chan.pkg_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else if name == b"policyd" {
                match (chan.pol_send_slot, chan.pol_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else if name == b"bundlemgrd" {
                match (chan.bnd_send_slot, chan.bnd_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else if name == b"samgrd" {
                match (chan.sam_send_slot, chan.sam_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else if name == b"execd" {
                match (chan.exe_send_slot, chan.exe_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else if name == b"keystored" {
                match (chan.key_send_slot, chan.key_recv_slot) {
                    (Some(send), Some(recv)) => (0u8, send, recv),
                    _ => (1u8, 0u32, 0u32),
                }
            } else {
                (1u8, 0u32, 0u32)
            };
            let mut rsp = [0u8; 1 + 1 + 4 + 4];
            rsp[0] = ROUTE_RSP;
            rsp[1] = status;
            rsp[2..6].copy_from_slice(&send_slot.to_le_bytes());
            rsp[6..10].copy_from_slice(&recv_slot.to_le_bytes());
            let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
            let _ = nexus_abi::ipc_send_v1(
                chan.ctrl_rsp_parent_slot,
                &rh,
                &rsp,
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            );
        }
        let _ = nexus_abi::yield_();
        if let Some(limit) = watchdog {
            ticks = ticks.saturating_add(1);
            if ticks >= limit {
                fatal("init-lite: watchdog fired");
            }
        }
    }
}

fn spawn_service(image: &ServiceImage, _name: &ServiceNameGuard<'_>) -> Result<u32> {
    if image.elf.is_empty() {
        return Err(InitError::MissingElf);
    }

    let stack_pages = image.stack_pages.max(1) as usize;
    if probes_enabled() {
        debug_write_bytes(b"!exec call name=");
        debug_write_str(image.name);
        debug_write_byte(b'\n');
    }
    let pid = nexus_abi::exec_v2(image.elf, stack_pages, image.global_pointer, image.name)
        .map_err(InitError::Abi)?;
    if probes_enabled() {
        debug_write_bytes(b"!exec ret\n");
    }

    // NOTE: Child bootstrap endpoint is already seeded at slot 0 by `spawn`/`exec_v2`
    // (TaskTable::spawn copies the parent's bootstrap slot into the child cap table).
    // Do NOT cap_transfer it again here, otherwise it shifts deterministic slot assignment
    // for service endpoints (e.g. VFS req/rsp slots 1/2).

    Ok(pid)
}

impl From<AbiError> for InitError {
    fn from(err: AbiError) -> Self {
        Self::Abi(err)
    }
}

impl From<IpcError> for InitError {
    fn from(err: IpcError) -> Self {
        Self::Ipc(err)
    }
}

/// Helper that renders an [`InitError`] into the shared logging line format.
pub fn describe_init_error(line: &mut LineBuilder<'_, '_>, err: &InitError) {
    match err {
        InitError::Abi(code) => {
            line.text("abi:");
            line.text(abi_error_label(*code));
        }
        InitError::Ipc(code) => {
            line.text("ipc:");
            line.text(ipc_error_label(*code));
        }
        InitError::Elf(msg) => {
            line.text("elf:");
            line.text(msg);
        }
        InitError::Map(msg) => {
            line.text("map:");
            line.text(msg);
        }
        InitError::MissingElf => {
            line.text("missing-elf");
        }
    }
}

fn abi_error_label(err: AbiError) -> &'static str {
    match err {
        AbiError::InvalidSyscall => "invalid-syscall",
        AbiError::CapabilityDenied => "capability-denied",
        AbiError::IpcFailure => "ipc-failure",
        AbiError::SpawnFailed => "spawn-failed",
        AbiError::TransferFailed => "transfer-failed",
        AbiError::ChildUnavailable => "child-unavailable",
        AbiError::NoSuchPid => "no-such-pid",
        AbiError::InvalidArgument => "invalid-argument",
        AbiError::Unsupported => "unsupported",
    }
}

fn ipc_error_label(err: IpcError) -> &'static str {
    match err {
        IpcError::NoSuchEndpoint => "no-such-endpoint",
        IpcError::QueueFull => "queue-full",
        IpcError::QueueEmpty => "queue-empty",
        IpcError::PermissionDenied => "permission-denied",
        IpcError::TimedOut => "timed-out",
        IpcError::Unsupported => "unsupported",
    }
}
