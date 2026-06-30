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

use core::fmt;
use core::sync::atomic::AtomicBool;

#[cfg(nexus_env = "os")]
use nexus_abi::{self, AbiError, IpcError};

pub(crate) use crate::bootstrap::BootstrapState;

// Re-export items moved to bootstrap/ during RFC-0061 refactoring
// so existing imports from `crate::os_payload` continue to resolve.
pub use crate::bootstrap::helpers::fatal_err;
pub(crate) use crate::bootstrap::helpers::{
    abi_error_label, bundlemgrd_set_active_slot, configure_log_topics, debug_write_byte,
    debug_write_bytes, debug_write_hex, debug_write_str, decode_init_health_ok_req,
    decode_init_health_ok_req_with_optional_nonce, encode_init_health_ok_rsp,
    encode_init_health_ok_rsp_with_optional_nonce, fatal, grant_mmio_cap, ipc_error_label,
    log_str_ptr, probe_debug_write_words, probe_virtio_mmio_slots, probes_enabled, raw_probe_str,
    updated_boot_attempt, updated_health_ok, virtio_mmio_window, watchdog_limit_ticks,
    ServiceNameGuard, DEVICE_MMIO_CAP_SLOT, INPUT_MMIO_CAP_SLOT_BASE, POLICY_NONCE,
    VIRTIO_MMIO_BASE, VIRTIO_MMIO_STRIDE,
};
pub(crate) use crate::bootstrap::policyd::policyd_cap_allowed;
pub(crate) use crate::route_table::RouteTable;
pub(crate) use nexus_abi::Rights;

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

pub(crate) const MAX_LOG_STR_LEN: usize = 512;

extern "C" {
    pub(crate) static __rodata_start: u8;
    pub(crate) static __rodata_end: u8;
    pub(crate) static __data_start: u8;
    pub(crate) static __data_end: u8;
    pub(crate) static __small_data_guard: u8;
    pub(crate) static __image_end: u8;
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

pub(crate) mod log_topics {
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

pub(crate) type Result<T> = core::result::Result<T, InitError>;

pub(crate) static PROBE_ENABLED: AtomicBool = AtomicBool::new(false);

// Phase-2 hardening: init-lite holds an EndpointFactory capability (slot 1) for endpoint_create.
pub(crate) const ENDPOINT_FACTORY_CAP_SLOT: u32 = 1;

// RFC-0005: per-service bootstrap routing protocol (init-lite responder over a private control EP).
pub(crate) const CTRL_EP_DEPTH: usize = 8;
pub(crate) const CTRL_CHILD_SEND_SLOT: u32 = 1; // First cap_transfer into a freshly spawned task (slot 0 is reserved).
pub(crate) const CTRL_CHILD_RECV_SLOT: u32 = 2; // Second cap_transfer (paired reply endpoint).

pub(crate) const INIT_HEALTH_MAGIC0: u8 = b'I';
pub(crate) const INIT_HEALTH_MAGIC1: u8 = b'H';
pub(crate) const INIT_HEALTH_VERSION: u8 = 1;
pub(crate) const INIT_HEALTH_OP_OK: u8 = 1;
pub(crate) const INIT_HEALTH_STATUS_OK: u8 = 0;
pub(crate) const INIT_HEALTH_STATUS_FAILED: u8 = 1;

/// Optional bring-up watchdog to force a panic if init spins forever.

// Thin wrappers and public API — implementation extracted to bootstrap/helpers.rs

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
