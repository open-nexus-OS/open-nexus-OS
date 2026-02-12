//! CONTEXT: OS payload backend for init-lite (service spawning + capability distribution + routing responder)
//! OWNERS: @init-team @runtime
//! STATUS: Functional (bring-up)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//!   - Service spawn/readiness markers (init: start/up, *: ready)
//!   - Policy-gated MMIO distribution (device.mmio.net/rng/blk)
//!   - Routing responder (IPC channel grants for services)
//!   - Control channel setup (policyd/logd/samgrd/bundlemgrd)
//! ADR: docs/adr/0017-service-architecture.md
//!
//! This module is compiled only for `nexus_env="os"` and is used by `init-lite` as the minimal
//! wrapper around kernel `exec_v2` + RFC-0005 capability distribution and routing responder.

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

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

    pub fn ipc_endpoint_create_v2(_factory_cap: u32, _queue_depth: usize) -> SysResult<u32> {
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

    pub mod routing {
        const MAGIC0: u8 = b'R';
        const MAGIC1: u8 = b'T';
        pub const VERSION: u8 = 1;
        pub const OP_ROUTE_GET: u8 = 0x40;
        pub const OP_ROUTE_RSP: u8 = 0x41;
        pub const STATUS_OK: u8 = 0;
        pub const STATUS_NOT_FOUND: u8 = 1;
        pub const STATUS_MALFORMED: u8 = 2;
        pub const MAX_SERVICE_NAME_LEN: usize = 48;

        pub fn encode_route_get(name: &[u8], out: &mut [u8]) -> Option<usize> {
            if name.is_empty() || name.len() > MAX_SERVICE_NAME_LEN || out.len() < 5 + name.len() {
                return None;
            }
            out[0] = MAGIC0;
            out[1] = MAGIC1;
            out[2] = VERSION;
            out[3] = OP_ROUTE_GET;
            out[4] = name.len() as u8;
            out[5..5 + name.len()].copy_from_slice(name);
            Some(5 + name.len())
        }

        pub fn decode_route_get(frame: &[u8]) -> Option<&[u8]> {
            if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
                return None;
            }
            if frame[3] != OP_ROUTE_GET {
                return None;
            }
            let n = frame[4] as usize;
            if n == 0 || n > MAX_SERVICE_NAME_LEN || frame.len() != 5 + n {
                return None;
            }
            Some(&frame[5..])
        }

        pub fn encode_route_rsp(status: u8, send_slot: u32, recv_slot: u32) -> [u8; 13] {
            let mut out = [0u8; 13];
            out[0] = MAGIC0;
            out[1] = MAGIC1;
            out[2] = VERSION;
            out[3] = OP_ROUTE_RSP;
            out[4] = status;
            out[5..9].copy_from_slice(&send_slot.to_le_bytes());
            out[9..13].copy_from_slice(&recv_slot.to_le_bytes());
            out
        }

        pub fn decode_route_rsp(frame: &[u8]) -> Option<(u8, u32, u32)> {
            if frame.len() != 13 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION
            {
                return None;
            }
            if frame[3] != OP_ROUTE_RSP {
                return None;
            }
            let status = frame[4];
            let send_slot = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            let recv_slot = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
            Some((status, send_slot, recv_slot))
        }
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
const CTRL_EP_DEPTH: usize = 8;
const CTRL_CHILD_SEND_SLOT: u32 = 1; // First cap_transfer into a freshly spawned task (slot 0 is reserved).
const CTRL_CHILD_RECV_SLOT: u32 = 2; // Second cap_transfer (paired reply endpoint).

const INIT_HEALTH_MAGIC0: u8 = b'I';
const INIT_HEALTH_MAGIC1: u8 = b'H';
const INIT_HEALTH_VERSION: u8 = 1;
const INIT_HEALTH_OP_OK: u8 = 1;
const INIT_HEALTH_STATUS_OK: u8 = 0;
const INIT_HEALTH_STATUS_FAILED: u8 = 1;

#[derive(Clone, Copy)]
struct CtrlChannel {
    /// Service name for this PID (init-lite authoritative).
    svc_name: &'static str,
    /// PID of the spawned service task.
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
    upd_send_slot: Option<u32>,
    upd_recv_slot: Option<u32>,
    sam_send_slot: Option<u32>,
    sam_recv_slot: Option<u32>,
    exe_send_slot: Option<u32>,
    exe_recv_slot: Option<u32>,
    key_send_slot: Option<u32>,
    key_recv_slot: Option<u32>,
    state_send_slot: Option<u32>,
    state_recv_slot: Option<u32>,
    /// Optional routing for target "rngd" from the perspective of this PID:
    /// - send_slot: where this PID should send entropy requests
    /// - recv_slot: where this PID should receive direct replies (if used)
    rng_send_slot: Option<u32>,
    rng_recv_slot: Option<u32>,
    timed_send_slot: Option<u32>,
    timed_recv_slot: Option<u32>,
    net_send_slot: Option<u32>,
    net_recv_slot: Option<u32>,
    log_send_slot: Option<u32>,
    log_recv_slot: Option<u32>,
    /// Optional routing for target "dsoftbusd" from the perspective of this PID:
    /// - send_slot: where this PID should send requests/replies
    /// - recv_slot: where this PID should receive replies/requests
    dsoft_send_slot: Option<u32>,
    dsoft_recv_slot: Option<u32>,
    /// Self reply-inbox slots (only populated for requesters that need CAP_MOVE reply routing).
    reply_send_slot: Option<u32>,
    reply_recv_slot: Option<u32>,
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

// Nonce for policyd v2 (correlated) control-plane requests.
static POLICY_NONCE: AtomicU32 = AtomicU32::new(1);
// Deterministic DeviceMmio slot (per-service cap table).
const DEVICE_MMIO_CAP_SLOT: u32 = 48;
// QEMU `virt` virtio-mmio layout (per-device windows).
const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
const VIRTIO_MMIO_STRIDE: usize = 0x1000;

fn virtio_mmio_window(slot: usize) -> (usize, usize) {
    (VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE, VIRTIO_MMIO_STRIDE)
}

fn probe_virtio_mmio_slots() -> Result<(usize, usize, Option<usize>)> {
    // Map the full virtio-mmio window to discover device slots, then mint per-device caps.
    const MAX_SLOTS: usize = 8;
    const MMIO_VA: usize = 0x2000_e000;
    const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976; // "virt"
    const VIRTIO_DEVICE_ID_NET: u32 = 1;
    const VIRTIO_DEVICE_ID_RNG: u32 = 4;
    const VIRTIO_DEVICE_ID_BLK: u32 = 2;

    let full_len = VIRTIO_MMIO_STRIDE * MAX_SLOTS;
    let cap = nexus_abi::device_mmio_cap_create(VIRTIO_MMIO_BASE, full_len, usize::MAX)
        .map_err(InitError::Abi)?;

    let mut net_slot: Option<usize> = None;
    let mut rng_slot: Option<usize> = None;
    let mut blk_slot: Option<usize> = None;
    for slot in 0..MAX_SLOTS {
        let off = slot * VIRTIO_MMIO_STRIDE;
        let va = MMIO_VA + off;
        match nexus_abi::mmio_map(cap, va, off) {
            Ok(()) => {}
            Err(nexus_abi::AbiError::InvalidArgument) => {}
            Err(_) => continue,
        }
        let magic = unsafe { core::ptr::read_volatile((va + 0x000) as *const u32) };
        if magic != VIRTIO_MMIO_MAGIC {
            continue;
        }
        let device_id = unsafe { core::ptr::read_volatile((va + 0x008) as *const u32) };
        if device_id == VIRTIO_DEVICE_ID_NET {
            net_slot = Some(slot);
        } else if device_id == VIRTIO_DEVICE_ID_RNG {
            rng_slot = Some(slot);
        } else if device_id == VIRTIO_DEVICE_ID_BLK {
            blk_slot = Some(slot);
        }
        if net_slot.is_some() && rng_slot.is_some() && blk_slot.is_some() {
            break;
        }
    }
    let _ = nexus_abi::cap_close(cap);
    let net_slot = net_slot.ok_or(InitError::Map("virtio-net slot not found"))?;
    let rng_slot = rng_slot.ok_or(InitError::Map("virtio-rng slot not found"))?;
    Ok((net_slot, rng_slot, blk_slot))
}

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
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
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
fn bootstrap_service_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<BootstrapState>
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
    // Phase-2 hardening: init-lite holds an EndpointFactory capability (slot 1) for endpoint_create.
    const ENDPOINT_FACTORY_CAP_SLOT: u32 = 1;
    //
    // Phase-2 hardening (ownership correctness):
    // We create *service request endpoints* owned by the target service PID (close-on-exit),
    // which requires knowing the PID. Therefore we create response endpoints up front, spawn
    // services, then create request endpoints (owner=service PID) and distribute caps in a
    // second pass before the first yield.
    // NOTE: response endpoints are owned by their receiver (typically the requester).
    // We create them after spawning once the requester PID is known.
    // Private init-lite -> policyd response channels (init-lite receives replies).
    let pol_ctl_route_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let init_pid = nexus_abi::pid().map_err(InitError::Abi)?;
    let init_reply_send = nexus_abi::cap_clone(pol_ctl_route_rsp).map_err(InitError::Abi)?;
    let init_reply_send =
        nexus_abi::cap_transfer(init_pid, init_reply_send, Rights::SEND).map_err(InitError::Abi)?;
    let pol_ctl_exec_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;

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
                //
                // IMPORTANT: These endpoints must remain usable by init-lite for the routing responder
                // loop. Creating them as init-owned endpoints avoids needing `cap_clone` (which adds
                // extra syscalls and increases preemption windows during bring-up).
                let ctrl_req_parent_slot =
                    nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, CTRL_EP_DEPTH)
                        .map_err(InitError::Abi)?;
                let ctrl_rsp_parent_slot =
                    nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, CTRL_EP_DEPTH)
                        .map_err(InitError::Abi)?;
                // IMPORTANT: The kernel IPC backend assumes the per-service routing control
                // channels live in deterministic slots (userspace `nexus-ipc` uses 1/2).
                // Use cap_transfer_to_slot to avoid slot drift when we add new capabilities.
                let child_send_slot = nexus_abi::cap_transfer_to_slot(
                    pid,
                    ctrl_req_parent_slot,
                    Rights::SEND,
                    CTRL_CHILD_SEND_SLOT,
                )
                .map_err(InitError::Abi)?;
                let child_recv_slot = nexus_abi::cap_transfer_to_slot(
                    pid,
                    ctrl_rsp_parent_slot,
                    Rights::RECV,
                    CTRL_CHILD_RECV_SLOT,
                )
                .map_err(InitError::Abi)?;
                if image.name == "updated" {
                    debug_write_bytes(b"init: updated ctrl slots send=0x");
                    debug_write_hex(child_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(child_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                if probes_enabled()
                    && (child_send_slot != CTRL_CHILD_SEND_SLOT
                        || child_recv_slot != CTRL_CHILD_RECV_SLOT)
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

                let ctrl = CtrlChannel {
                    svc_name: image.name,
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
                    upd_send_slot: None,
                    upd_recv_slot: None,
                    sam_send_slot: None,
                    sam_recv_slot: None,
                    exe_send_slot: None,
                    exe_recv_slot: None,
                    key_send_slot: None,
                    key_recv_slot: None,
                    state_send_slot: None,
                    state_recv_slot: None,
                    rng_send_slot: None,
                    rng_recv_slot: None,
                    timed_send_slot: None,
                    timed_recv_slot: None,
                    net_send_slot: None,
                    net_recv_slot: None,
                    log_send_slot: None,
                    log_recv_slot: None,
                    dsoft_send_slot: None,
                    dsoft_recv_slot: None,
                    reply_send_slot: None,
                    reply_recv_slot: None,
                };
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
    // Second pass: create request endpoints owned by the target service PID and distribute caps.
    fn find_pid(ctrls: &[CtrlChannel], name: &str) -> Option<u32> {
        ctrls.iter().find(|c| c.svc_name == name).map(|c| c.pid)
    }

    let selftest_pid = find_pid(&ctrl_channels, "selftest-client").ok_or(InitError::MissingElf)?;
    let vfsd_pid = find_pid(&ctrl_channels, "vfsd").ok_or(InitError::MissingElf)?;
    let packagefsd_pid = find_pid(&ctrl_channels, "packagefsd").ok_or(InitError::MissingElf)?;
    let policyd_pid = find_pid(&ctrl_channels, "policyd").ok_or(InitError::MissingElf)?;
    let netstackd_pid = find_pid(&ctrl_channels, "netstackd").ok_or(InitError::MissingElf)?;
    let dsoftbusd_pid = find_pid(&ctrl_channels, "dsoftbusd").ok_or(InitError::MissingElf)?;
    let bundlemgrd_pid = find_pid(&ctrl_channels, "bundlemgrd").ok_or(InitError::MissingElf)?;
    let updated_pid = find_pid(&ctrl_channels, "updated").ok_or(InitError::MissingElf)?;
    let samgrd_pid = find_pid(&ctrl_channels, "samgrd").ok_or(InitError::MissingElf)?;
    let execd_pid = find_pid(&ctrl_channels, "execd").ok_or(InitError::MissingElf)?;
    let _keystored_pid = find_pid(&ctrl_channels, "keystored").ok_or(InitError::MissingElf)?;
    let _statefsd_pid = find_pid(&ctrl_channels, "statefsd").ok_or(InitError::MissingElf)?;
    let rngd_pid = find_pid(&ctrl_channels, "rngd").ok_or(InitError::MissingElf)?;
    let timed_pid = find_pid(&ctrl_channels, "timed").ok_or(InitError::MissingElf)?;
    let logd_pid = find_pid(&ctrl_channels, "logd");

    // selftest-client <-> service endpoint pairs
    let vfs_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, vfsd_pid, 8)
        .map_err(InitError::Abi)?;
    let vfs_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let pkg_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, packagefsd_pid, 8)
        .map_err(InitError::Abi)?;
    let pkg_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let pol_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, policyd_pid, 8)
        .map_err(InitError::Abi)?;
    let pol_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, bundlemgrd_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_rsp_updated =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, updated_pid, 8)
            .map_err(InitError::Abi)?;
    let upd_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, updated_pid, 8)
        .map_err(InitError::Abi)?;
    let upd_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let sam_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, samgrd_pid, 8)
        .map_err(InitError::Abi)?;
    let sam_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let exe_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
        .map_err(InitError::Abi)?;
    let exe_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    // Create init-owned endpoints so init-lite can deterministically distribute RECV/SEND rights.
    // `ipc_endpoint_create_for(... owner=keystored ...)` does not guarantee the creator holds RECV,
    // and `cap_transfer(... Rights::RECV)` can be rejected by the kernel.
    let key_req =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    // #region agent log (probe key_req rights via self-transfer)
    if let Ok(me) = nexus_abi::pid() {
        debug_write_bytes(b"init: probe key_req self-xfer pid=0x");
        debug_write_hex(me as usize);
        debug_write_bytes(b" cap=0x");
        debug_write_hex(key_req as usize);
        debug_write_byte(b'\n');
        match nexus_abi::cap_transfer(me, key_req, Rights::SEND) {
            Ok(slot) => {
                debug_write_bytes(b"init: probe key_req self-xfer SEND ok slot=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
                let _ = nexus_abi::cap_close(slot);
            }
            Err(e) => {
                debug_write_bytes(b"init: probe key_req self-xfer SEND err=abi:");
                debug_write_str(abi_error_label(e.clone()));
                debug_write_byte(b'\n');
            }
        }
        match nexus_abi::cap_transfer(me, key_req, Rights::RECV) {
            Ok(slot) => {
                debug_write_bytes(b"init: probe key_req self-xfer RECV ok slot=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
                let _ = nexus_abi::cap_close(slot);
            }
            Err(e) => {
                debug_write_bytes(b"init: probe key_req self-xfer RECV err=abi:");
                debug_write_str(abi_error_label(e.clone()));
                debug_write_byte(b'\n');
            }
        }
    } else {
        debug_write_bytes(b"init: probe key_req self-xfer pid() failed\n");
    }
    // #endregion agent log
    let key_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    // NOTE: keep this endpoint init-owned so statefsd's cap table stays clear at slot 0x30
    // until the policy-gated MMIO grant is transferred there (statefsd probes MMIO at slot 48).
    let state_req =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let state_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // rngd <-> clients endpoints:
    // - rng_req owned by rngd (server receives requests)
    // - rng_rsp owned by selftest-client (server can send direct replies to selftest without CAP_MOVE)
    let rng_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, rngd_pid, 8)
        .map_err(InitError::Abi)?;
    let rng_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let timed_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, timed_pid, 8)
        .map_err(InitError::Abi)?;
    let timed_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // logd (optional) service endpoints (request/response).
    // If logd is present in the image set, selftest-client gets a dedicated pair.
    let (log_req, log_rsp) = if let Some(_pid) = logd_pid {
        // logd is a high-fan-in sink (policyd/execd/bundlemgrd/updated/etc). Use a larger queue
        // budget to avoid CAP_MOVE senders hitting QueueFull under cooperative scheduling.
        // NOTE: Keep the request endpoint init-owned so it remains valid independent of bring-up
        // ordering. Init-lite distributes SEND/RECV rights explicitly to the participants.
        let req = nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 64)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };

    // bundlemgrd <-> execd dedicated pair (avoid reusing selftest-client <-> execd channels)
    let bnd_exe_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_exe_rsp =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, bundlemgrd_pid, 8)
            .map_err(InitError::Abi)?;

    // Selftest reply-inbox endpoint:
    // - owned by selftest-client (receiver)
    // - selftest-client holds RECV to await replies and a SEND cap that it can CAP_MOVE to a server
    let reply_ep = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // execd reply-inbox endpoint (for CAP_MOVE request/reply, e.g. logd crash append).
    let execd_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
            .map_err(InitError::Abi)?;

    // DSoftBusd reply-inbox endpoint (for CAP_MOVE request/reply).
    let dsoft_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, dsoftbusd_pid, 8)
            .map_err(InitError::Abi)?;

    // DSoftBusd service endpoints (request/response) so other tasks (e.g. selftest-client) can route to it.
    let dsoft_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, dsoftbusd_pid, 8)
        .map_err(InitError::Abi)?;
    let dsoft_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // Netstackd service endpoints (request/response).
    let net_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, netstackd_pid, 8)
        .map_err(InitError::Abi)?;
    let net_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, netstackd_pid, 8)
        .map_err(InitError::Abi)?;
    // Client-side netstackd receive endpoints (currently unused by the CAP_MOVE protocol but required for routing).
    let net_selftest_rsp =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
            .map_err(InitError::Abi)?;
    let net_dsoft_rsp =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, dsoftbusd_pid, 8)
            .map_err(InitError::Abi)?;

    // packagefsd reply-inbox endpoint (for CAP_MOVE request/reply to other services, e.g. bundlemgrd):
    let pkg_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, packagefsd_pid, 8)
            .map_err(InitError::Abi)?;

    // Private init-lite <-> policyd channels: request endpoints are owned by policyd (it receives queries).
    let pol_ctl_route_req =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, policyd_pid, 8)
            .map_err(InitError::Abi)?;
    let pol_ctl_exec_req =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, policyd_pid, 8)
            .map_err(InitError::Abi)?;

    // Ensure policyd control channels are live before policy-gated grants.
    let _ = nexus_abi::cap_transfer(policyd_pid, pol_ctl_route_req, Rights::RECV)
        .map_err(InitError::Abi)?;
    let _ = nexus_abi::cap_transfer(policyd_pid, pol_ctl_route_rsp, Rights::SEND)
        .map_err(InitError::Abi)?;
    let _ = nexus_abi::cap_transfer(policyd_pid, pol_ctl_exec_req, Rights::RECV)
        .map_err(InitError::Abi)?;
    let _ = nexus_abi::cap_transfer(policyd_pid, pol_ctl_exec_rsp, Rights::SEND)
        .map_err(InitError::Abi)?;

    // Policy-gated DeviceMmio grants (per-device windows) before other cap transfers.
    let grant_mmio_with_wait =
        |pid: u32, svc_name: &str, cap_name: &str, slot: usize| -> Result<()> {
            let (mmio_base, mmio_len) = virtio_mmio_window(slot);
            let deadline = match nexus_abi::nsec() {
                Ok(now) => now.saturating_add(1_000_000_000),
                Err(_) => 0,
            };
            loop {
                match grant_mmio_cap(
                    pid,
                    svc_name,
                    cap_name,
                    mmio_base,
                    mmio_len,
                    pol_ctl_route_req,
                    pol_ctl_route_rsp,
                    DEVICE_MMIO_CAP_SLOT,
                )? {
                    Some(_) => break,
                    None => {
                        let now = match nexus_abi::nsec() {
                            Ok(value) => value,
                            Err(_) => 0,
                        };
                        if now >= deadline {
                            return Err(InitError::Map("mmio policy timeout"));
                        }
                        let _ = nexus_abi::yield_();
                    }
                }
            }
            Ok(())
        };

    // Policy negative proof: deny-by-default for a non-matching MMIO capability.
    //
    // Today we use a stable, always-present subject (`netstackd`) and a capability that must not
    // be granted to it (`device.mmio.blk`). This is independent of device enumeration and proves:
    // - init consults policyd (no local allowlist)
    // - policyd denies by default for a capability not in policy
    // - a deterministic UART marker is emitted only on real denial
    let deny_deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(1_000_000_000),
        Err(_) => 0,
    };
    loop {
        let subject_id = nexus_abi::service_id_from_name(b"netstackd");
        match policyd_cap_allowed(
            pol_ctl_route_req,
            pol_ctl_route_rsp,
            subject_id,
            b"device.mmio.blk",
        ) {
            Some(false) => {
                debug_write_str("init: mmio policy deny ok");
                debug_write_byte(b'\n');
                break;
            }
            Some(true) => {
                return Err(InitError::Map("mmio policy deny unexpectedly allowed"));
            }
            None => {
                let now = match nexus_abi::nsec() {
                    Ok(value) => value,
                    Err(_) => 0,
                };
                if now >= deny_deadline {
                    return Err(InitError::Map("mmio policy deny timeout"));
                }
                let _ = nexus_abi::yield_();
            }
        }
    }

    let (net_slot, rng_slot, blk_slot) = probe_virtio_mmio_slots()?;
    grant_mmio_with_wait(netstackd_pid, "netstackd", "device.mmio.net", net_slot)?;
    grant_mmio_with_wait(rngd_pid, "rngd", "device.mmio.rng", rng_slot)?;
    grant_mmio_with_wait(selftest_pid, "selftest-client", "device.mmio.net", net_slot)?;

    if let Some(virtioblkd_pid) = find_pid(&ctrl_channels, "virtioblkd") {
        let blk_slot = blk_slot.ok_or(InitError::Map("virtio-blk slot not found"))?;
        grant_mmio_with_wait(virtioblkd_pid, "virtioblkd", "device.mmio.blk", blk_slot)?;
    }
    if let Some(statefsd_pid) = find_pid(&ctrl_channels, "statefsd") {
        let blk_slot = blk_slot.ok_or(InitError::Map("virtio-blk slot not found"))?;
        grant_mmio_with_wait(statefsd_pid, "statefsd", "device.mmio.blk", blk_slot)?;
    }

    for chan in &mut ctrl_channels {
        let pid = chan.pid;
        // #region agent log (wire-up progress)
        debug_write_bytes(b"init: wire svc=");
        debug_write_str(chan.svc_name);
        debug_write_bytes(b" pid=0x");
        debug_write_hex(pid as usize);
        debug_write_byte(b'\n');
        // #endregion agent log
        match chan.svc_name {
            "netstackd" => {
                // Provide netstackd its own request/response endpoints (server side).
                // #region agent log (netstackd cap transfers)
                debug_write_bytes(b"init: wire netstackd xfer net_req RECV\n");
                // #endregion agent log
                let recv_slot = match nexus_abi::cap_transfer(pid, net_req, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (netstackd cap transfer error)
                        debug_write_bytes(b"init: wire netstackd xfer net_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };

                // #region agent log (netstackd cap transfers)
                debug_write_bytes(b"init: wire netstackd xfer net_rsp SEND\n");
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, net_rsp, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (netstackd cap transfer error)
                        debug_write_bytes(b"init: wire netstackd xfer net_rsp err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.net_send_slot = Some(send_slot);
                chan.net_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: netstackd svc slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');
            }
            "dsoftbusd" => {
                // Allow dsoftbusd to send requests to netstackd (and optionally receive on a dedicated inbox).
                // Place into fixed slots to match userspace bring-up constants (avoid relying on allocation order).
                let send_slot = nexus_abi::cap_transfer_to_slot(pid, net_req, Rights::SEND, 0x03)
                    .map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, net_dsoft_rsp, Rights::RECV, 0x04)
                        .map_err(InitError::Abi)?;
                chan.net_send_slot = Some(send_slot);
                chan.net_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: dsoftbusd netstackd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, dsoft_reply_ep, Rights::RECV, 0x05)
                        .map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer_to_slot(pid, dsoft_reply_ep, Rights::SEND, 0x06)
                        .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                debug_write_bytes(b"init: dsoftbusd reply slots recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_byte(b'\n');

                // Allow dsoftbusd to call into samgrd/bundlemgrd via CAP_MOVE reply inbox.
                // - send to service request endpoint
                // - receive replies on local reply inbox recv slot
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(reply_recv_slot);
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(reply_recv_slot);

                // Provide dsoftbusd its own request/response endpoints (server side).
                let recv_slot = nexus_abi::cap_transfer(pid, dsoft_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, dsoft_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.dsoft_send_slot = Some(send_slot);
                chan.dsoft_recv_slot = Some(recv_slot);

                // TASK-0006: allow dsoftbusd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }
            }
            "vfsd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, vfs_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, vfs_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.vfs_send_slot = Some(send_slot);
                chan.vfs_recv_slot = Some(recv_slot);

                // vfsd needs to resolve pkg:/ paths against packagefsd (real data path).
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);
            }
            "packagefsd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE replies.
                let reply_recv_slot = nexus_abi::cap_transfer(pid, pkg_reply_ep, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let reply_send_slot = nexus_abi::cap_transfer(pid, pkg_reply_ep, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);

                // Allow packagefsd to talk to bundlemgrd using CAP_MOVE replies:
                // - send to bundlemgrd's request endpoint
                // - receive replies on the local reply inbox recv slot
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(reply_recv_slot);
            }
            "policyd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: policyd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                chan.state_recv_slot = Some(reply_recv_slot);
                debug_write_bytes(b"init: policyd reply slots recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_byte(b'\n');

                // TASK-0006: allow policyd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                    debug_write_bytes(b"init: policyd logd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "bundlemgrd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: bundlemgrd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Allow bundlemgrd to route to execd (policyd may still deny).
                let send_slot = nexus_abi::cap_transfer(pid, bnd_exe_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, bnd_exe_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);

                // TASK-0006: allow bundlemgrd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }
            }
            "updated" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, upd_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, upd_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.upd_send_slot = Some(send_slot);
                chan.upd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: updated slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                let transfer = |cap: u32, rights: Rights, label: &'static str| -> Option<u32> {
                    match nexus_abi::cap_transfer(pid, cap, rights) {
                        Ok(slot) => Some(slot),
                        Err(err) => {
                            debug_write_bytes(b"init: updated cap transfer fail ");
                            debug_write_str(label);
                            debug_write_bytes(b" err=");
                            debug_write_str(abi_error_label(err.clone()));
                            debug_write_byte(b'\n');
                            None
                        }
                    }
                };

                // Allow updated to call bundlemgrd (slot-aware publication).
                let send_slot = transfer(bnd_req, Rights::SEND, "bundlemgrd send");
                let recv_slot = transfer(bnd_rsp_updated, Rights::RECV, "bundlemgrd recv");
                if let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) {
                    chan.bnd_send_slot = Some(send_slot);
                    chan.bnd_recv_slot = Some(recv_slot);
                }

                // Allow updated to call keystored for signature verification.
                let send_slot = transfer(key_req, Rights::SEND, "keystored send");
                let recv_slot = transfer(key_rsp, Rights::RECV, "keystored recv");
                if let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) {
                    chan.key_send_slot = Some(send_slot);
                    chan.key_recv_slot = Some(recv_slot);
                }

                // Allow updated to call statefsd for persistence.
                let send_slot = transfer(state_req, Rights::SEND, "statefsd send");
                if let Some(send_slot) = send_slot {
                    chan.state_send_slot = Some(send_slot);
                    debug_write_bytes(b"init: updated statefsd send slot=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot = transfer(reply_ep, Rights::RECV, "reply recv");
                let reply_send_slot = transfer(reply_ep, Rights::SEND, "reply send");
                if let (Some(reply_recv_slot), Some(reply_send_slot)) =
                    (reply_recv_slot, reply_send_slot)
                {
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.state_recv_slot = Some(reply_recv_slot);
                    debug_write_bytes(b"init: updated reply recv slot=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                    debug_write_bytes(b"init: updated reply send slot=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // TASK-0006: allow updated to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    if let Some(send_slot) = transfer(req, Rights::SEND, "logd send") {
                        chan.log_send_slot = Some(send_slot);
                        if let Some(reply_recv_slot) = reply_recv_slot {
                            chan.log_recv_slot = Some(reply_recv_slot);
                        }
                    }
                }
            }
            "samgrd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: samgrd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);

                // TASK-0006: allow samgrd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }
            }
            "execd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, exe_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, exe_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);

                // Reply inbox: provide both RECV (stay with execd) and SEND (to be moved to servers).
                let reply_recv_slot = nexus_abi::cap_transfer(pid, execd_reply_ep, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let reply_send_slot = nexus_abi::cap_transfer(pid, execd_reply_ep, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                debug_write_bytes(b"init: execd reply slots recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_byte(b'\n');

                // Optional: allow execd to send crash reports to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                    debug_write_bytes(b"init: execd logd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "keystored" => {
                // #region agent log (keystored arm entry)
                debug_write_bytes(b"init: ks arm\n");
                // #endregion agent log
                // #region agent log (keystored wire-up tracing)
                debug_write_bytes(b"init: wire keystored xfer key_req RECV cap=0x");
                debug_write_hex(key_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let recv_slot = match nexus_abi::cap_transfer(pid, key_req, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer key_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };

                // #region agent log (keystored wire-up tracing)
                debug_write_bytes(b"init: wire keystored xfer key_rsp SEND cap=0x");
                debug_write_hex(key_rsp as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, key_rsp, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer key_rsp err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.key_send_slot = Some(send_slot);
                chan.key_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE reply routing (used by statefsd + log sinks).
                // #region agent log (keystored reply-inbox create)
                debug_write_bytes(b"init: wire keystored create reply_ep\n");
                // #endregion agent log
                let reply_ep =
                    match nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (keystored wire-up error)
                            debug_write_bytes(b"init: wire keystored create reply_ep err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    };

                // #region agent log (keystored reply-inbox transfer)
                debug_write_bytes(b"init: wire keystored xfer reply_ep RECV cap=0x");
                debug_write_hex(reply_ep as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let reply_recv_slot = match nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer reply_ep RECV err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                // #region agent log (keystored reply-inbox transfer)
                debug_write_bytes(b"init: wire keystored xfer reply_ep SEND cap=0x");
                debug_write_hex(reply_ep as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let reply_send_slot = match nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer reply_ep SEND err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);

                // statefsd SEND cap + use reply inbox for responses
                // #region agent log (keystored statefsd send cap)
                debug_write_bytes(b"init: wire keystored xfer state_req SEND cap=0x");
                debug_write_hex(state_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, state_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer state_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(reply_recv_slot);

                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }

                // Allow keystored to call policyd (reply via CAP_MOVE/@reply).
                // #region agent log (keystored policyd send cap)
                debug_write_bytes(b"init: wire keystored xfer pol_req SEND cap=0x");
                debug_write_hex(pol_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, pol_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer pol_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(reply_recv_slot);

                // Allow keystored to send entropy requests to rngd (replies via CAP_MOVE/@reply).
                // #region agent log (keystored rngd send cap)
                debug_write_bytes(b"init: wire keystored xfer rng_req SEND cap=0x");
                debug_write_hex(rng_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, rng_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer rng_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.rng_send_slot = Some(send_slot);
                // Use reply inbox recv slot for routing responses (CAP_MOVE replies land here).
                chan.rng_recv_slot = Some(reply_recv_slot);
            }
            "statefsd" => {
                let recv_slot = nexus_abi::cap_transfer(pid, state_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: statefsd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Provide a reply inbox for CAP_MOVE reply routing (policyd checks, logd).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);

                // Allow statefsd to call policyd (reply via CAP_MOVE/@reply).
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(reply_recv_slot);
            }
            "rngd" => {
                // Server-side endpoints for rngd.
                let recv_slot =
                    nexus_abi::cap_transfer(pid, rng_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, rng_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.rng_send_slot = Some(send_slot);
                chan.rng_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE reply routing (used by clients).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);

                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }

                // Allow rngd to call policyd (reply via CAP_MOVE/@reply).
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(reply_recv_slot);
            }
            "timed" => {
                let recv_slot = nexus_abi::cap_transfer(pid, timed_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, timed_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.timed_send_slot = Some(send_slot);
                chan.timed_recv_slot = Some(recv_slot);
            }
            "logd" => {
                if let (Some(req), Some(rsp)) = (log_req, log_rsp) {
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::RECV).map_err(InitError::Abi)?;
                    let send_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: logd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "selftest-client" => {
                let send_slot =
                    nexus_abi::cap_transfer(pid, vfs_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, vfs_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.vfs_send_slot = Some(send_slot);
                chan.vfs_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest vfsd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pol_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest policyd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, bnd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest bundlemgrd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, upd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, upd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.upd_send_slot = Some(send_slot);
                chan.upd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest updated slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, sam_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest samgrd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, exe_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, exe_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest execd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, key_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, key_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.key_send_slot = Some(send_slot);
                chan.key_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest keystored slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest statefsd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                if let (Some(req), Some(rsp)) = (log_req, log_rsp) {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::RECV).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: selftest logd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                debug_write_bytes(b"init: selftest reply slots send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_byte(b'\n');

                // Allow selftest-client to send requests to netstackd.
                let send_slot =
                    nexus_abi::cap_transfer(pid, net_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, net_selftest_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.net_send_slot = Some(send_slot);
                chan.net_recv_slot = Some(recv_slot);

                // Allow selftest-client to send requests to dsoftbusd (TASK-0005 remote proxy proof).
                let send_slot = nexus_abi::cap_transfer(pid, dsoft_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, dsoft_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.dsoft_send_slot = Some(send_slot);
                chan.dsoft_recv_slot = Some(recv_slot);

                // Allow selftest-client to send requests to rngd and receive direct replies.
                let send_slot =
                    nexus_abi::cap_transfer(pid, rng_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, rng_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.rng_send_slot = Some(send_slot);
                chan.rng_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest rngd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                // Allow selftest-client to send requests to timed and receive direct replies.
                let send_slot = nexus_abi::cap_transfer(pid, timed_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, timed_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.timed_send_slot = Some(send_slot);
                chan.timed_recv_slot = Some(recv_slot);
            }
            _ => {}
        }
    }

    // Yield after cap distribution so services observe a consistent slot layout.
    let _ = nexus_abi::yield_();

    let mut upd_pending: nexus_ipc::reqrep::FrameStash<8, 16> =
        nexus_ipc::reqrep::FrameStash::new();
    match updated_boot_attempt(&mut upd_pending, upd_req, init_reply_send, pol_ctl_route_rsp) {
        Ok(Some(slot)) => {
            let ok = bundlemgrd_set_active_slot(
                &mut upd_pending,
                bnd_req,
                init_reply_send,
                pol_ctl_route_rsp,
                slot,
            );
            if !ok {
                debug_write_str("init: rollback fail");
                debug_write_byte(b'\n');
            }
        }
        Ok(None) => {}
        Err(_) => {
            debug_write_str("init: boot attempt fail");
            debug_write_byte(b'\n');
        }
    }

    Ok(BootstrapState {
        ctrl_channels,
        pol_ctl_route_req,
        pol_ctl_route_rsp,
        pol_ctl_exec_req,
        pol_ctl_exec_rsp,
        upd_req,
        upd_reply_send: init_reply_send,
        upd_reply_recv: pol_ctl_route_rsp,
        upd_pending,
    })
}

struct BootstrapState {
    ctrl_channels: Vec<CtrlChannel>,
    pol_ctl_route_req: u32,
    pol_ctl_route_rsp: u32,
    pol_ctl_exec_req: u32,
    pol_ctl_exec_rsp: u32,
    upd_req: u32,
    upd_reply_send: u32,
    upd_reply_recv: u32,
    upd_pending: nexus_ipc::reqrep::FrameStash<8, 16>,
}

fn decode_route_get_with_optional_nonce(frame: &[u8]) -> Option<(&[u8], Option<u32>)> {
    // v1: [R,T,1,OP_ROUTE_GET, name_len, name...]
    if let Some(name) = nexus_abi::routing::decode_route_get(frame) {
        return Some((name, None));
    }
    // v1 extension (nonce-correlated, backwards compatible):
    // [R,T,1,OP_ROUTE_GET, name_len, name..., nonce:u32le]
    if frame.len() < 9
        || frame[0] != b'R'
        || frame[1] != b'T'
        || frame[2] != nexus_abi::routing::VERSION
    {
        return None;
    }
    if frame[3] != nexus_abi::routing::OP_ROUTE_GET {
        return None;
    }
    let n = frame[4] as usize;
    if n == 0 || n > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return None;
    }
    if frame.len() != 5 + n + 4 {
        return None;
    }
    let nonce = u32::from_le_bytes([frame[5 + n], frame[6 + n], frame[7 + n], frame[8 + n]]);
    Some((&frame[5..5 + n], Some(nonce)))
}

/// Same as [`bootstrap_service_images`] but keeps the init task alive forever.
pub fn service_main_loop_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<()>
where
    F: FnOnce() + Send,
{
    let state = bootstrap_service_images(images, notifier)?;
    let ctrl_channels = state.ctrl_channels;
    let pol_ctl_route_req = state.pol_ctl_route_req;
    let pol_ctl_route_rsp = state.pol_ctl_route_rsp;
    let pol_ctl_exec_req = state.pol_ctl_exec_req;
    let pol_ctl_exec_rsp = state.pol_ctl_exec_rsp;
    let upd_req = state.upd_req;
    let upd_reply_send = state.upd_reply_send;
    let upd_reply_recv = state.upd_reply_recv;
    let mut upd_pending = state.upd_pending;
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
            if chan.svc_name == "updated" {
                debug_write_bytes(b"init: ctrl req from updated\n");
            }
            // Health gate: allow selftest-client to notify init.
            if chan.svc_name == "selftest-client" && decode_init_health_ok_req(&buf[..n]) {
                let nonce = decode_init_health_ok_req_with_optional_nonce(&buf[..n]).flatten();
                let status = match updated_health_ok(
                    &mut upd_pending,
                    upd_req,
                    upd_reply_send,
                    upd_reply_recv,
                ) {
                    Ok(slot) => {
                        debug_write_str("init: health ok (slot ");
                        debug_write_byte(slot);
                        debug_write_str(")");
                        debug_write_byte(b'\n');
                        INIT_HEALTH_STATUS_OK
                    }
                    Err(err) => {
                        debug_write_str("init: health fail ");
                        match err {
                            InitError::Map(msg) => debug_write_str(msg),
                            InitError::Abi(code) => debug_write_str(abi_error_label(code)),
                            InitError::Ipc(code) => debug_write_str(ipc_error_label(code)),
                            InitError::Elf(msg) => debug_write_str(msg),
                            InitError::MissingElf => debug_write_str("missing-elf"),
                        }
                        debug_write_byte(b'\n');
                        INIT_HEALTH_STATUS_FAILED
                    }
                };
                if nonce.is_some() {
                    let rsp = encode_init_health_ok_rsp_with_optional_nonce(status, nonce);
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = encode_init_health_ok_rsp(status);
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                }
                continue;
            }

            // Either ROUTE_GET (routing) or policy exec-check requests.
            let (name, route_nonce) = match decode_route_get_with_optional_nonce(&buf[..n]) {
                Some((name, nonce)) => (name, nonce),
                None => {
                    if let Some((nonce, requester, image_id)) =
                        nexus_abi::policy::decode_exec_check(&buf[..n])
                    {
                        // Identity-binding hardening:
                        // - This exec-check control path is a **proxy** from `execd` to `policyd`
                        //   via init-lite (bring-up topology).
                        // - The `requester` bytes inside the frame are *not* authoritative; only
                        //   the control channel identity is. Therefore: only accept these frames
                        //   on execd's private control channel.
                        if chan.svc_name != "execd" {
                            let rsp = nexus_abi::policy::encode_exec_check_rsp(
                                nonce,
                                nexus_abi::policy::STATUS_DENY,
                            );
                            let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                            let _ = nexus_abi::ipc_send_v1(
                                chan.ctrl_rsp_parent_slot,
                                &rh,
                                &rsp,
                                nexus_abi::IPC_SYS_NONBLOCK,
                                0,
                            );
                            continue;
                        }
                        let allowed = policyd_exec_allowed(
                            pol_ctl_exec_req,
                            pol_ctl_exec_rsp,
                            requester,
                            image_id,
                        )
                        .unwrap_or(true);
                        let status = if allowed {
                            nexus_abi::policy::STATUS_ALLOW
                        } else {
                            nexus_abi::policy::STATUS_DENY
                        };
                        let rsp = nexus_abi::policy::encode_exec_check_rsp(nonce, status);
                        let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                        let _ = nexus_abi::ipc_send_v1(
                            chan.ctrl_rsp_parent_slot,
                            &rh,
                            &rsp,
                            nexus_abi::IPC_SYS_NONBLOCK,
                            0,
                        );
                    }
                    continue;
                }
            };
            if name == b"samgrd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route samgrd from selftest-client\n");
            }
            if name == b"statefsd" {
                debug_write_bytes(b"init: route statefsd from ");
                debug_write_str(chan.svc_name);
                debug_write_byte(b'\n');
            }
            if name == b"vfsd" {
                debug_write_bytes(b"init: route vfsd from ");
                debug_write_str(chan.svc_name);
                debug_write_byte(b'\n');
            }
            // Special route: requester-local reply inbox for CAP_MOVE reply routing.
            // Returns (send_slot, recv_slot) for the requester's own reply endpoint.
            if name == b"@reply" {
                let status = if chan.reply_send_slot.is_some() && chan.reply_recv_slot.is_some() {
                    nexus_abi::routing::STATUS_OK
                } else {
                    nexus_abi::routing::STATUS_NOT_FOUND
                };
                let send_slot = chan.reply_send_slot.unwrap_or(0);
                let recv_slot = chan.reply_recv_slot.unwrap_or(0);
                if let Some(nonce) = route_nonce {
                    let base = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                    let mut rsp = [0u8; 17];
                    rsp[..13].copy_from_slice(&base);
                    rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                }
                continue;
            }
            // policy gating (bring-up): consult policyd if available.
            // Never gate policyd's own routing to avoid deadlocks during early bring-up.
            //
            // Default is fail-open during early bring-up (policyd may not be started yet).
            // For privileged routes we can require a policyd answer (fail-closed) case-by-case.
            let allowed = if name == chan.svc_name.as_bytes() {
                // Self-routing (service resolving its own endpoint slots) is always allowed.
                true
            } else if chan.svc_name == "policyd" {
                true
            } else if chan.svc_name == "bundlemgrd" && name == b"execd" {
                // Deterministic proof route: require policyd to answer; otherwise deny.
                policyd_route_allowed(pol_ctl_route_req, pol_ctl_route_rsp, chan.svc_name, name)
                    .unwrap_or(false)
            } else {
                policyd_route_allowed(pol_ctl_route_req, pol_ctl_route_rsp, chan.svc_name, name)
                    .unwrap_or(true)
            };
            if !allowed {
                if name == b"vfsd" {
                    debug_write_bytes(b"init: route vfsd DENIED by policy\n");
                }
                if let Some(nonce) = route_nonce {
                    let base = nexus_abi::routing::encode_route_rsp(
                        nexus_abi::routing::STATUS_DENIED,
                        0,
                        0,
                    );
                    let mut rsp = [0u8; 17];
                    rsp[..13].copy_from_slice(&base);
                    rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = nexus_abi::routing::encode_route_rsp(
                        nexus_abi::routing::STATUS_DENIED,
                        0,
                        0,
                    );
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                }
                continue;
            }

            let (status, send_slot, recv_slot) = if name == b"vfsd" {
                // Debug: log vfsd routing lookup.
                debug_write_bytes(b"init: route vfsd lookup svc=");
                debug_write_str(chan.svc_name);
                debug_write_bytes(b" has_slots=");
                debug_write_byte(if chan.vfs_send_slot.is_some() && chan.vfs_recv_slot.is_some() {
                    b'Y'
                } else {
                    b'N'
                });
                debug_write_byte(b'\n');
                match (chan.vfs_send_slot, chan.vfs_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"packagefsd" {
                match (chan.pkg_send_slot, chan.pkg_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"policyd" {
                debug_write_bytes(b"init: route policyd from ");
                debug_write_str(chan.svc_name);
                debug_write_bytes(b" pol_send=");
                if let Some(s) = chan.pol_send_slot {
                    debug_write_hex(s as usize);
                } else {
                    debug_write_bytes(b"None");
                }
                debug_write_bytes(b" pol_recv=");
                if let Some(r) = chan.pol_recv_slot {
                    debug_write_hex(r as usize);
                } else {
                    debug_write_bytes(b"None");
                }
                debug_write_byte(b'\n');
                match (chan.pol_send_slot, chan.pol_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"bundlemgrd" {
                match (chan.bnd_send_slot, chan.bnd_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"logd" {
                match (chan.log_send_slot, chan.log_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"updated" {
                match (chan.upd_send_slot, chan.upd_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"samgrd" {
                match (chan.sam_send_slot, chan.sam_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"execd" {
                match (chan.exe_send_slot, chan.exe_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"keystored" {
                match (chan.key_send_slot, chan.key_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"statefsd" {
                match (chan.state_send_slot, chan.state_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"netstackd" {
                match (chan.net_send_slot, chan.net_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"logd" {
                match (chan.log_send_slot, chan.log_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"dsoftbusd" {
                match (chan.dsoft_send_slot, chan.dsoft_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"rngd" {
                match (chan.rng_send_slot, chan.rng_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else if name == b"timed" {
                match (chan.timed_send_slot, chan.timed_recv_slot) {
                    (Some(send), Some(recv)) => (nexus_abi::routing::STATUS_OK, send, recv),
                    _ => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                }
            } else {
                (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32)
            };
            if name == b"samgrd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route samgrd rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if name == b"rngd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route rngd rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if name == b"logd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route logd rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if name == b"updated" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route updated rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if let Some(nonce) = route_nonce {
                let base = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                let mut rsp = [0u8; 17];
                rsp[..13].copy_from_slice(&base);
                rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                let _ = nexus_abi::ipc_send_v1(
                    chan.ctrl_rsp_parent_slot,
                    &rh,
                    &rsp,
                    nexus_abi::IPC_SYS_NONBLOCK,
                    0,
                );
            } else {
                let rsp = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                let _ = nexus_abi::ipc_send_v1(
                    chan.ctrl_rsp_parent_slot,
                    &rh,
                    &rsp,
                    nexus_abi::IPC_SYS_NONBLOCK,
                    0,
                );
            }
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

fn policyd_route_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    requester: &str,
    target: &[u8],
) -> Option<bool> {
    // policyd OP_ROUTE request (v3, nonce-correlated, ID-based):
    // [P,O,ver=3,OP_ROUTE=2, nonce:u32le, requester_id:u64le, target_id:u64le]
    if requester.len() > 48 || target.is_empty() || target.len() > 48 {
        return None;
    }
    let nonce = POLICY_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = [0u8; 10 + 48 + 48];
    let requester_id = nexus_abi::service_id_from_name(requester.as_bytes());
    let target_id = nexus_abi::service_id_from_name(target);
    let n = nexus_abi::policyd::encode_route_v3_id(nonce, requester_id, target_id, &mut frame)?;

    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(200_000_000),
        Err(_) => 0,
    };

    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(pol_send_slot, &hdr, &frame[..n], 0, deadline).is_err() {
        return None;
    }
    // Wait for the matching nonce. If a stale reply is queued, we'll consume and ignore it.
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    loop {
        let got = nexus_abi::ipc_recv_v1(
            pol_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        )
        .ok()? as usize;
        // IPC_SYS_TRUNCATE can report a length larger than our local buffer.
        // Never slice past the buffer (would panic and destabilize bring-up).
        let got = core::cmp::min(got, buf.len());
        let (_ver, op, got_nonce, status) = nexus_abi::policyd::decode_rsp_v2_or_v3(&buf[..got])?;
        if op != nexus_abi::policyd::OP_ROUTE || got_nonce != nonce {
            continue;
        }
        // Deterministic debug (once) for the bundlemgrd->execd denial gate.
        if requester == "bundlemgrd" && target == b"execd" {
            debug_write_bytes(b"init: policyd route bundlemgrd->execd status=0x");
            debug_write_hex(status as usize);
            debug_write_byte(b'\n');
        }
        return match status {
            nexus_abi::policyd::STATUS_ALLOW => Some(true),
            nexus_abi::policyd::STATUS_DENY => Some(false),
            _ => None,
        };
    }
}

fn policyd_cap_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    subject_id: u64,
    cap: &[u8],
) -> Option<bool> {
    if cap.is_empty() || cap.len() > 48 {
        return None;
    }
    // policyd OP_CHECK_CAP request (v1):
    // [P,O,ver=1,OP_CHECK_CAP, subject_id:u64le, cap_len:u8, cap...]
    let mut frame = [0u8; 13 + 48];
    frame[0] = b'P';
    frame[1] = b'O';
    frame[2] = nexus_abi::policyd::VERSION_V1;
    frame[3] = nexus_abi::policyd::OP_CHECK_CAP;
    frame[4..12].copy_from_slice(&subject_id.to_le_bytes());
    frame[12] = cap.len() as u8;
    frame[13..13 + cap.len()].copy_from_slice(cap);
    let n = 13 + cap.len();

    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(1_000_000_000),
        Err(_) => 0,
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(pol_send_slot, &hdr, &frame[..n], 0, deadline).is_err() {
        return None;
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let got = nexus_abi::ipc_recv_v1(
        pol_recv_slot,
        &mut rh,
        &mut buf,
        nexus_abi::IPC_SYS_TRUNCATE,
        deadline,
    )
    .ok()? as usize;
    let got = core::cmp::min(got, buf.len());
    if got < 6 || buf[0] != b'P' || buf[1] != b'O' || buf[2] != nexus_abi::policyd::VERSION_V1 {
        return None;
    }
    if buf[3] != (nexus_abi::policyd::OP_CHECK_CAP | 0x80) {
        return None;
    }
    match buf[4] {
        nexus_abi::policyd::STATUS_ALLOW => Some(true),
        nexus_abi::policyd::STATUS_DENY => Some(false),
        _ => None,
    }
}

fn grant_mmio_cap(
    pid: u32,
    svc_name: &str,
    cap_name: &str,
    base: usize,
    len: usize,
    pol_send: u32,
    pol_recv: u32,
    expected_slot: u32,
) -> Result<Option<bool>> {
    // #region agent log (mmio grant tracing)
    debug_write_bytes(b"init: mmio grant begin svc=");
    debug_write_str(svc_name);
    debug_write_bytes(b" pid=0x");
    debug_write_hex(pid as usize);
    debug_write_bytes(b" slot=0x");
    debug_write_hex(expected_slot as usize);
    debug_write_bytes(b" base=0x");
    debug_write_hex(base);
    debug_write_bytes(b" len=0x");
    debug_write_hex(len);
    debug_write_bytes(b" cap=");
    debug_write_str(cap_name);
    debug_write_byte(b'\n');
    // #endregion agent log

    let subject_id = nexus_abi::service_id_from_name(svc_name.as_bytes());
    let allowed = match policyd_cap_allowed(pol_send, pol_recv, subject_id, cap_name.as_bytes()) {
        Some(value) => value,
        None => return Ok(None),
    };
    if !allowed {
        debug_write_bytes(b"init: mmio grant DENIED svc=");
        debug_write_str(svc_name);
        debug_write_bytes(b" cap=");
        debug_write_str(cap_name);
        debug_write_byte(b'\n');
        return Ok(Some(false));
    }

    // #region agent log (mmio cap create/transfer tracing)
    debug_write_bytes(b"init: mmio cap_create svc=");
    debug_write_str(svc_name);
    debug_write_byte(b'\n');
    // #endregion agent log

    let cap = match nexus_abi::device_mmio_cap_create(base, len, usize::MAX) {
        Ok(slot) => {
            // #region agent log (mmio cap create result)
            debug_write_bytes(b"init: mmio cap_create ok svc=");
            debug_write_str(svc_name);
            debug_write_bytes(b" cap_slot=0x");
            debug_write_hex(slot as usize);
            debug_write_byte(b'\n');
            // #endregion agent log
            slot
        }
        Err(e) => {
            // #region agent log (mmio cap create error)
            debug_write_bytes(b"init: mmio cap_create err svc=");
            debug_write_str(svc_name);
            debug_write_bytes(b" err=abi:");
            debug_write_str(abi_error_label(e.clone()));
            debug_write_byte(b'\n');
            // #endregion agent log
            return Err(InitError::Abi(e));
        }
    };

    // #region agent log (mmio cap transfer begin)
    debug_write_bytes(b"init: mmio xfer_to_slot svc=");
    debug_write_str(svc_name);
    debug_write_bytes(b" dst_slot=0x");
    debug_write_hex(expected_slot as usize);
    debug_write_byte(b'\n');
    // #endregion agent log

    let slot = match nexus_abi::cap_transfer_to_slot(pid, cap, Rights::MAP, expected_slot) {
        Ok(slot) => {
            // #region agent log (mmio cap transfer ok)
            debug_write_bytes(b"init: mmio xfer_to_slot ok svc=");
            debug_write_str(svc_name);
            debug_write_bytes(b" got=0x");
            debug_write_hex(slot as usize);
            debug_write_byte(b'\n');
            // #endregion agent log
            slot
        }
        Err(e) => {
            // #region agent log (mmio cap transfer error)
            debug_write_bytes(b"init: mmio xfer_to_slot err svc=");
            debug_write_str(svc_name);
            debug_write_bytes(b" err=abi:");
            debug_write_str(abi_error_label(e.clone()));
            debug_write_byte(b'\n');
            // #endregion agent log
            let _ = nexus_abi::cap_close(cap);
            return Err(InitError::Abi(e));
        }
    };

    let _ = nexus_abi::cap_close(cap);
    if slot != expected_slot {
        debug_write_bytes(b"init: mmio grant slot mismatch svc=");
        debug_write_str(svc_name);
        debug_write_bytes(b" got=0x");
        debug_write_hex(slot as usize);
        debug_write_byte(b'\n');
        return Err(InitError::Map("mmio slot mismatch"));
    }
    debug_write_bytes(b"init: mmio grant svc=");
    debug_write_str(svc_name);
    debug_write_bytes(b" slot=0x");
    debug_write_hex(slot as usize);
    debug_write_byte(b'\n');
    Ok(Some(true))
}

fn updated_boot_attempt(
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    upd_req: u32,
    reply_send: u32,
    reply_recv: u32,
) -> Result<Option<u8>> {
    let mut req = [0u8; 4];
    let len = nexus_abi::updated::encode_boot_attempt_req(&mut req)
        .ok_or(InitError::Map("updated boot attempt encode failed"))?;
    let mut attempts = 0u8;
    let max_attempts: u8 = 20;
    loop {
        attempts = attempts.saturating_add(1);
        let reply_send_clone = nexus_abi::cap_clone(reply_send).map_err(InitError::Abi)?;
        let hdr = nexus_abi::MsgHeader::new(
            reply_send_clone,
            0,
            0,
            nexus_abi::ipc_hdr::CAP_MOVE,
            len as u32,
        );
        let deadline = match nexus_abi::nsec() {
            Ok(now) => now.saturating_add(500_000_000),
            Err(_) => 0,
        };
        let send = nexus_abi::ipc_send_v1(upd_req, &hdr, &req[..len], 0, deadline);
        if send.is_err() {
            if attempts < max_attempts {
                let _ = nexus_abi::yield_();
                continue;
            }
            return Ok(None);
        }

        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        loop {
            let now = match nexus_abi::nsec() {
                Ok(now) => now,
                Err(_) => break,
            };
            if now >= deadline {
                break;
            }
            // Deterministic shared-inbox handling: first consume any previously stashed replies.
            if let Some(n) = pending.take_into_where(&mut buf, |f| {
                nexus_abi::updated::decode_boot_attempt_rsp(f).is_some()
            }) {
                if let Some((status, slot)) = nexus_abi::updated::decode_boot_attempt_rsp(&buf[..n])
                {
                    if status != nexus_abi::updated::STATUS_OK {
                        return Err(InitError::Map("updated boot attempt failed"));
                    }
                    if slot == 0 {
                        return Ok(None);
                    }
                    return Ok(Some(slot));
                }
            }
            match nexus_abi::ipc_recv_v1(
                reply_recv,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    // IPC_SYS_TRUNCATE can return a length larger than our local buffer.
                    // Never slice past the buffer (would panic and kill init-lite).
                    let n = core::cmp::min(n as usize, buf.len());
                    if let Some((status, slot)) =
                        nexus_abi::updated::decode_boot_attempt_rsp(&buf[..n])
                    {
                        if status != nexus_abi::updated::STATUS_OK {
                            return Err(InitError::Map("updated boot attempt failed"));
                        }
                        if slot == 0 {
                            return Ok(None);
                        }
                        return Ok(Some(slot));
                    }
                    // Stash unrelated replies deterministically for the next consumer of this inbox.
                    let _ = pending.push(&buf[..n]);
                    continue;
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = nexus_abi::yield_();
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
        }
        if attempts < max_attempts {
            let _ = nexus_abi::yield_();
            continue;
        }
        // Updated not ready yet; skip boot attempt for this cycle.
        return Ok(None);
    }
}

fn bundlemgrd_set_active_slot(
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    bnd_req: u32,
    reply_send: u32,
    reply_recv: u32,
    slot: u8,
) -> bool {
    let mut req = [0u8; 5];
    nexus_abi::bundlemgrd::encode_set_active_slot_req(slot, &mut req);
    let reply_send_clone = match nexus_abi::cap_clone(reply_send) {
        Ok(slot) => slot,
        Err(_) => return false,
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );
    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(200_000_000),
        Err(_) => 0,
    };
    if nexus_abi::ipc_send_v1(bnd_req, &hdr, &req, 0, deadline).is_err() {
        return false;
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];

    // First: check if we already buffered the expected response.
    if let Some(n) = pending.take_into_where(&mut buf, |f| {
        nexus_abi::bundlemgrd::decode_set_active_slot_rsp(f).is_some()
    }) {
        return match nexus_abi::bundlemgrd::decode_set_active_slot_rsp(&buf[..n]) {
            Some((status, rsp_slot)) => {
                status == nexus_abi::bundlemgrd::STATUS_OK && rsp_slot == slot
            }
            None => false,
        };
    }

    // Deterministic NONBLOCK receive loop so we can stash unrelated frames.
    let mut spins: usize = 0;
    loop {
        if (spins & 0x7f) == 0 && nexus_abi::nsec().ok().unwrap_or(0) >= deadline {
            return false;
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if let Some((status, rsp_slot)) =
                    nexus_abi::bundlemgrd::decode_set_active_slot_rsp(&buf[..n])
                {
                    return status == nexus_abi::bundlemgrd::STATUS_OK && rsp_slot == slot;
                }
                let _ = pending.push(&buf[..n]);
                let _ = nexus_abi::yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return false,
        }
        spins = spins.wrapping_add(1);
    }
}

fn decode_init_health_ok_req(frame: &[u8]) -> bool {
    decode_init_health_ok_req_with_optional_nonce(frame).is_some()
}

fn encode_init_health_ok_rsp(status: u8) -> [u8; 5] {
    [INIT_HEALTH_MAGIC0, INIT_HEALTH_MAGIC1, INIT_HEALTH_VERSION, INIT_HEALTH_OP_OK | 0x80, status]
}

fn decode_init_health_ok_req_with_optional_nonce(frame: &[u8]) -> Option<Option<u32>> {
    // v1 request: [I,H,1,OP_OK]
    // v1+nonce extension: [I,H,1,OP_OK, nonce:u32le]
    if frame.len() == 4 {
        if frame[0] == INIT_HEALTH_MAGIC0
            && frame[1] == INIT_HEALTH_MAGIC1
            && frame[2] == INIT_HEALTH_VERSION
            && frame[3] == INIT_HEALTH_OP_OK
        {
            return Some(None);
        }
        return None;
    }
    if frame.len() == 8 {
        if frame[0] != INIT_HEALTH_MAGIC0
            || frame[1] != INIT_HEALTH_MAGIC1
            || frame[2] != INIT_HEALTH_VERSION
            || frame[3] != INIT_HEALTH_OP_OK
        {
            return None;
        }
        let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        return Some(Some(nonce));
    }
    None
}

fn encode_init_health_ok_rsp_with_optional_nonce(status: u8, nonce: Option<u32>) -> [u8; 9] {
    // v1+nonce response: [I,H,1,OP_OK|0x80, status, nonce:u32le]
    let mut out = [0u8; 9];
    out[0] = INIT_HEALTH_MAGIC0;
    out[1] = INIT_HEALTH_MAGIC1;
    out[2] = INIT_HEALTH_VERSION;
    out[3] = INIT_HEALTH_OP_OK | 0x80;
    out[4] = status;
    let n = nonce.unwrap_or(0);
    out[5..9].copy_from_slice(&n.to_le_bytes());
    out
}

fn updated_health_ok(
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    upd_req: u32,
    reply_send: u32,
    reply_recv: u32,
) -> Result<u8> {
    let mut req = [0u8; 4];
    let len = nexus_abi::updated::encode_health_ok_req(&mut req)
        .ok_or(InitError::Map("updated health_ok encode failed"))?;
    let reply_send_clone = nexus_abi::cap_clone(reply_send).map_err(InitError::Abi)?;
    let hdr =
        nexus_abi::MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, len as u32);
    // Avoid deadline-based blocking IPC in bring-up; use explicit nsec()-bounded NONBLOCK loops.
    let start = nexus_abi::nsec().map_err(InitError::Abi)?;
    let deadline = start.saturating_add(20_000_000_000); // 20s (can contend with stage work under QEMU)
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(upd_req, &hdr, &req[..len], nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(InitError::Abi)?;
                    if now >= deadline {
                        return Err(InitError::Map("updated health_ok send timeout"));
                    }
                }
                let _ = nexus_abi::yield_();
            }
            Err(e) => return Err(InitError::Ipc(e)),
        }
        i = i.wrapping_add(1);
    }

    // Receive the HealthOk response before issuing GetStatus on the same reply inbox.
    // IMPORTANT: reply inbox is shared; stash unrelated replies deterministically.
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    let mut logged_other = false;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(InitError::Abi)?;
            if now >= deadline {
                return Err(InitError::Map("updated health_ok timeout"));
            }
        }
        if let Some(_n) = pending.take_into_where(&mut buf, |f| {
            f.len() >= 7
                && f[0] == nexus_abi::updated::MAGIC0
                && f[1] == nexus_abi::updated::MAGIC1
                && f[2] == nexus_abi::updated::VERSION
                && f[3] == (nexus_abi::updated::OP_HEALTH_OK | 0x80)
        }) {
            if buf[4] != nexus_abi::updated::STATUS_OK {
                return Err(InitError::Map("updated health_ok failed"));
            }
            break;
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n >= 7
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                {
                    if buf[3] == (nexus_abi::updated::OP_HEALTH_OK | 0x80) {
                        if buf[4] != nexus_abi::updated::STATUS_OK {
                            return Err(InitError::Map("updated health_ok failed"));
                        }
                        break;
                    }
                    if !logged_other {
                        logged_other = true;
                        debug_write_bytes(b"init: health recv other op=0x");
                        debug_write_hex(buf[3] as usize);
                        debug_write_byte(b'\n');
                    }
                }
                let _ = pending.push(&buf[..n]);
                continue;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(e) => return Err(InitError::Ipc(e)),
        }
        j = j.wrapping_add(1);
    }

    updated_get_status(pending, upd_req, reply_send, reply_recv)
}

fn updated_get_status(
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    upd_req: u32,
    reply_send: u32,
    reply_recv: u32,
) -> Result<u8> {
    let mut req = [0u8; 4];
    let len = nexus_abi::updated::encode_get_status_req(&mut req)
        .ok_or(InitError::Map("updated status encode failed"))?;
    let reply_send_clone = nexus_abi::cap_clone(reply_send).map_err(InitError::Abi)?;
    let hdr =
        nexus_abi::MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, len as u32);
    let start = nexus_abi::nsec().map_err(InitError::Abi)?;
    let deadline = start.saturating_add(20_000_000_000); // 20s (can contend with stage work under QEMU)
    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(upd_req, &hdr, &req[..len], nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(InitError::Abi)?;
                    if now >= deadline {
                        return Err(InitError::Map("updated status send timeout"));
                    }
                }
                let _ = nexus_abi::yield_();
            }
            Err(e) => return Err(InitError::Ipc(e)),
        }
        i = i.wrapping_add(1);
    }

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(InitError::Abi)?;
            if now >= deadline {
                return Err(InitError::Map("updated status timeout"));
            }
        }
        if let Some(n) = pending.take_into_where(&mut buf, |f| {
            f.len() >= 7
                && f[0] == nexus_abi::updated::MAGIC0
                && f[1] == nexus_abi::updated::MAGIC1
                && f[2] == nexus_abi::updated::VERSION
                && f[3] == (nexus_abi::updated::OP_GET_STATUS | 0x80)
        }) {
            // Parse exactly as below.
            let got_n = n;
            if buf[4] != nexus_abi::updated::STATUS_OK {
                return Err(InitError::Map("updated status failed"));
            }
            let payload_len = u16::from_le_bytes([buf[5], buf[6]]) as usize;
            if payload_len < 1 || got_n < 7 + payload_len {
                return Err(InitError::Map("updated status payload missing"));
            }
            let active = buf[7];
            return match active {
                1 => Ok(b'a'),
                2 => Ok(b'b'),
                _ => Err(InitError::Map("updated status slot invalid")),
            };
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let got_n = core::cmp::min(n as usize, buf.len());
                if got_n >= 7
                    && buf[0] == nexus_abi::updated::MAGIC0
                    && buf[1] == nexus_abi::updated::MAGIC1
                    && buf[2] == nexus_abi::updated::VERSION
                    && buf[3] == (nexus_abi::updated::OP_GET_STATUS | 0x80)
                {
                    if buf[4] != nexus_abi::updated::STATUS_OK {
                        return Err(InitError::Map("updated status failed"));
                    }
                    let payload_len = u16::from_le_bytes([buf[5], buf[6]]) as usize;
                    if payload_len < 1 || got_n < 7 + payload_len {
                        return Err(InitError::Map("updated status payload missing"));
                    }
                    let active = buf[7];
                    return match active {
                        1 => Ok(b'a'),
                        2 => Ok(b'b'),
                        _ => Err(InitError::Map("updated status slot invalid")),
                    };
                }
                let _ = pending.push(&buf[..got_n]);
                continue;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if (j & 0x7f) == 0 {
                    let now = nexus_abi::nsec().map_err(InitError::Abi)?;
                    if now >= deadline {
                        return Err(InitError::Map("updated status timeout"));
                    }
                }
                let _ = nexus_abi::yield_();
            }
            Err(e) => return Err(InitError::Ipc(e)),
        }
        j = j.wrapping_add(1);
    }
}

fn policyd_exec_allowed(
    pol_send_slot: u32,
    pol_recv_slot: u32,
    requester: &[u8],
    image_id: u8,
) -> Option<bool> {
    // policyd OP_EXEC request (v3, nonce-correlated, ID-based):
    // [P,O,ver=3,OP_EXEC=3, nonce:u32le, requester_id:u64le, image_id]
    if requester.is_empty() || requester.len() > 48 {
        return None;
    }
    let nonce = POLICY_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = [0u8; 10 + 48];
    let requester_id = nexus_abi::service_id_from_name(requester);
    let n = nexus_abi::policyd::encode_exec_v3_id(nonce, requester_id, image_id, &mut frame)?;

    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(1_000_000_000),
        Err(_) => 0,
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    if nexus_abi::ipc_send_v1(pol_send_slot, &hdr, &frame[..n], 0, deadline).is_err() {
        return None;
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    loop {
        let got = nexus_abi::ipc_recv_v1(
            pol_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        )
        .ok()? as usize;
        let (_ver, op, got_nonce, status) = nexus_abi::policyd::decode_rsp_v2_or_v3(&buf[..got])?;
        if op != nexus_abi::policyd::OP_EXEC || got_nonce != nonce {
            continue;
        }
        return match status {
            nexus_abi::policyd::STATUS_ALLOW => Some(true),
            nexus_abi::policyd::STATUS_DENY => Some(false),
            _ => None,
        };
    }
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
            if *code == AbiError::SpawnFailed {
                if let Ok(reason) = nexus_abi::spawn_last_error() {
                    line.text(" reason=");
                    line.text(spawn_fail_reason_label(reason));
                }
            }
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

fn spawn_fail_reason_label(reason: nexus_abi::SpawnFailReason) -> &'static str {
    match reason {
        nexus_abi::SpawnFailReason::Unknown => "unknown",
        nexus_abi::SpawnFailReason::OutOfMemory => "oom",
        nexus_abi::SpawnFailReason::CapTableFull => "cap-table-full",
        nexus_abi::SpawnFailReason::EndpointQuota => "endpoint-quota",
        nexus_abi::SpawnFailReason::MapFailed => "map-failed",
        nexus_abi::SpawnFailReason::InvalidPayload => "invalid-payload",
        nexus_abi::SpawnFailReason::DeniedByPolicy => "denied-by-policy",
    }
}

fn ipc_error_label(err: IpcError) -> &'static str {
    match err {
        IpcError::NoSuchEndpoint => "no-such-endpoint",
        IpcError::QueueFull => "queue-full",
        IpcError::QueueEmpty => "queue-empty",
        IpcError::PermissionDenied => "permission-denied",
        IpcError::TimedOut => "timed-out",
        IpcError::NoSpace => "no-space",
        IpcError::Unsupported => "unsupported",
    }
}
