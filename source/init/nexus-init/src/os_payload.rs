// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

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

use crate::bootstrap::policyd::{policyd_cap_allowed, policyd_exec_allowed, policyd_route_allowed};
use crate::bootstrap::route_builder;
use crate::bootstrap::{BootstrapState, CtrlChannel};
use crate::route_table::{CapSlot, RouteTable, ServiceId};

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

    pub fn cap_close(_cap: u32) -> SysResult<()> {
        Ok(())
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

pub(crate) const INIT_HEALTH_MAGIC0: u8 = b'I';
pub(crate) const INIT_HEALTH_MAGIC1: u8 = b'H';
pub(crate) const INIT_HEALTH_VERSION: u8 = 1;
pub(crate) const INIT_HEALTH_OP_OK: u8 = 1;
pub(crate) const INIT_HEALTH_STATUS_OK: u8 = 0;
pub(crate) const INIT_HEALTH_STATUS_FAILED: u8 = 1;

/// Optional bring-up watchdog to force a panic if init spins forever.
pub(crate) fn watchdog_limit_ticks() -> Option<usize> {
    match option_env!("INIT_LITE_WATCHDOG_TICKS") {
        Some(val) if !val.is_empty() => usize::from_str_radix(val, 10).ok(),
        _ => None,
    }
}

/// Emit a fatal marker and trap so hangs/errors are visible in UART logs.
pub(crate) fn fatal(msg: &str) -> ! {
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
pub(crate) static POLICY_NONCE: AtomicU32 = AtomicU32::new(1);
// Deterministic DeviceMmio slot (per-service cap table).
const DEVICE_MMIO_CAP_SLOT: u32 = 48;
const FW_CFG_MMIO_CAP_SLOT: u32 = 49;
const INPUT_MMIO_CAP_SLOT_BASE: u32 = 50;
// QEMU `virt` virtio-mmio layout (per-device windows).
const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
const VIRTIO_MMIO_STRIDE: usize = 0x1000;
const FW_CFG_MMIO_BASE: usize = 0x1010_0000;
const FW_CFG_MMIO_LEN: usize = 0x1000;

fn virtio_mmio_window(slot: usize) -> (usize, usize) {
    (
        VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE,
        VIRTIO_MMIO_STRIDE,
    )
}

fn probe_virtio_mmio_slots() -> Result<(
    usize,
    usize,
    Option<usize>,
    Option<usize>,
    [Option<usize>; 3],
)> {
    // Map the supported virtio-mmio window to discover device slots, then mint
    // per-device caps. Scanning past the platform window faults in guest bring-up.
    const MAX_SLOTS: usize = 8;
    const MMIO_VA: usize = 0x2000_e000;
    const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976; // "virt"
    const VIRTIO_DEVICE_ID_NET: u32 = 1;
    const VIRTIO_DEVICE_ID_RNG: u32 = 4;
    const VIRTIO_DEVICE_ID_BLK: u32 = 2;
    const VIRTIO_DEVICE_ID_GPU: u32 = 16;
    const VIRTIO_DEVICE_ID_INPUT: u32 = 18;

    let full_len = VIRTIO_MMIO_STRIDE * MAX_SLOTS;
    let cap = nexus_abi::device_mmio_cap_create(VIRTIO_MMIO_BASE, full_len, usize::MAX)
        .map_err(InitError::Abi)?;

    let mut net_slot: Option<usize> = None;
    let mut rng_slot: Option<usize> = None;
    let mut blk_slot: Option<usize> = None;
    let mut gpu_slot: Option<usize> = None;
    let mut input_slots: [Option<usize>; 3] = [None, None, None];
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
        } else if device_id == VIRTIO_DEVICE_ID_GPU {
            gpu_slot = Some(slot);
        } else if device_id == VIRTIO_DEVICE_ID_INPUT {
            for input_slot in &mut input_slots {
                if input_slot.is_none() {
                    *input_slot = Some(slot);
                    break;
                }
            }
        }
        if net_slot.is_some()
            && rng_slot.is_some()
            && blk_slot.is_some()
            && gpu_slot.is_some()
            && input_slots.iter().all(Option::is_some)
        {
            break;
        }
    }
    let _ = nexus_abi::cap_close(cap);
    let net_slot = net_slot.ok_or(InitError::Map("virtio-net slot not found"))?;
    let rng_slot = rng_slot.ok_or(InitError::Map("virtio-rng slot not found"))?;
    Ok((net_slot, rng_slot, blk_slot, gpu_slot, input_slots))
}

pub(crate) fn debug_write_byte(byte: u8) {
    let _ = nexus_abi::debug_putc(byte);
}

pub(crate) fn debug_write_bytes(bytes: &[u8]) {
    for &b in bytes {
        debug_write_byte(b);
    }
}

pub(crate) fn debug_write_str(s: &str) {
    debug_write_bytes(s.as_bytes());
}

pub(crate) fn debug_write_hex(value: usize) {
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
pub(crate) fn bootstrap_service_images<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<BootstrapState>
where
    F: FnOnce() + Send,
{
    crate::bootstrap::orchestrator::run_bootstrap(images, notifier)
}

pub(crate) fn decode_route_get_with_optional_nonce(frame: &[u8]) -> Option<(&[u8], Option<u32>)> {
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
    crate::bootstrap::responder::run_responder_loop(
        state.ctrl_channels,
        state.route_table,
        state.pol_ctl_route_req,
        state.pol_ctl_route_rsp,
        state.pol_ctl_exec_req,
        state.pol_ctl_exec_rsp,
        state.upd_req,
        state.upd_reply_send,
        state.upd_reply_recv,
        state.upd_pending,
    );
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

pub(crate) fn decode_init_health_ok_req(frame: &[u8]) -> bool {
    decode_init_health_ok_req_with_optional_nonce(frame).is_some()
}

pub(crate) fn encode_init_health_ok_rsp(status: u8) -> [u8; 5] {
    [
        INIT_HEALTH_MAGIC0,
        INIT_HEALTH_MAGIC1,
        INIT_HEALTH_VERSION,
        INIT_HEALTH_OP_OK | 0x80,
        status,
    ]
}

pub(crate) fn decode_init_health_ok_req_with_optional_nonce(frame: &[u8]) -> Option<Option<u32>> {
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

pub(crate) fn encode_init_health_ok_rsp_with_optional_nonce(status: u8, nonce: Option<u32>) -> [u8; 9] {
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

pub(crate) fn updated_health_ok(
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    upd_req: u32,
    reply_send: u32,
    reply_recv: u32,
) -> Result<u8> {
    let mut req = [0u8; 4];
    let len = nexus_abi::updated::encode_health_ok_req(&mut req)
        .ok_or(InitError::Map("updated health_ok encode failed"))?;
    let reply_send_clone = nexus_abi::cap_clone(reply_send).map_err(InitError::Abi)?;
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        len as u32,
    );
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
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        len as u32,
    );
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

pub(crate) fn abi_error_label(err: AbiError) -> &'static str {
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

pub(crate) fn ipc_error_label(err: IpcError) -> &'static str {
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
