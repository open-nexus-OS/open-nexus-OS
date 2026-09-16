// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap helper functions — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md
//!
//! Contains: MMIO probing/grants, OTA boot, health checks, debug helpers, error labels, utils.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::bootstrap::policyd::policyd_cap_allowed;
use crate::os_payload::log_topics;
use crate::os_payload::{
    self, InitError, Result, __data_end, __data_start, __rodata_end, __rodata_start,
    INIT_HEALTH_MAGIC0, INIT_HEALTH_MAGIC1, INIT_HEALTH_OP_OK, INIT_HEALTH_VERSION,
    MAX_LOG_STR_LEN, PROBE_ENABLED,
};
use nexus_abi::{self, AbiError, IpcError, Rights};

// Split out by the structure-gate; re-exported so existing import paths
// (`helpers::abi_error_label` in os_payload) stay valid.
pub(crate) use super::labels::{abi_error_label, spawn_fail_reason_label};
use nexus_log::{LineBuilder, StrRef};

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

pub(crate) fn configure_log_topics() {
    let mask = match option_env!("INIT_LITE_LOG_TOPICS") {
        Some(spec) if !spec.is_empty() => os_payload::log_topics::parse_spec(spec.as_bytes()),
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

pub(crate) fn probes_enabled() -> bool {
    PROBE_ENABLED.load(Ordering::Relaxed)
}

const GUARD_STR_PROBE_LIMIT: usize = 128;
static GUARD_STR_PROBE_COUNT: AtomicUsize = AtomicUsize::new(0);

// Nonce for policyd v2 (correlated) control-plane requests.
pub(crate) static POLICY_NONCE: AtomicU32 = AtomicU32::new(1);
// Deterministic DeviceMmio slot (per-service cap table).
pub(crate) const DEVICE_MMIO_CAP_SLOT: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
pub(crate) const INPUT_MMIO_CAP_SLOT_BASE: u32 = nexus_service_topology::INPUT_MMIO_SLOTS[0];
// QEMU `virt` virtio-mmio layout (per-device windows).
pub(crate) const VIRTIO_MMIO_BASE: usize = 0x1000_1000;
pub(crate) const VIRTIO_MMIO_STRIDE: usize = 0x1000;

pub(crate) fn virtio_mmio_window(slot: usize) -> (usize, usize) {
    (VIRTIO_MMIO_BASE + slot * VIRTIO_MMIO_STRIDE, VIRTIO_MMIO_STRIDE)
}

pub(crate) fn probe_virtio_mmio_slots(
) -> Result<(usize, usize, [Option<usize>; 2], Option<usize>, [Option<usize>; 3])> {
    // Map the supported virtio-mmio window to discover device slots, then mint
    // per-device caps. Scanning past the platform window faults in guest bring-up.
    const MAX_SLOTS: usize = 8;
    const VIRTIO_MMIO_MAGIC: u32 = 0x7472_6976; // "virt"
    const VIRTIO_DEVICE_ID_NET: u32 = 1;
    const VIRTIO_DEVICE_ID_RNG: u32 = 4;
    const VIRTIO_DEVICE_ID_BLK: u32 = 2;
    const VIRTIO_DEVICE_ID_GPU: u32 = 16;
    const VIRTIO_DEVICE_ID_INPUT: u32 = 18;

    let full_len = VIRTIO_MMIO_STRIDE * MAX_SLOTS;
    let cap = nexus_abi::device_mmio_cap_create(VIRTIO_MMIO_BASE, full_len, usize::MAX)
        .map_err(InitError::Abi)?;
    // RFC-0085: ONE kernel-chosen map of the whole probe window — the eight
    // per-slot maps at the shared fixed 0x2000_e000 va (and the
    // AlreadyExists dance around them) are gone.
    let probe_va = nexus_abi::mmio_map_auto(cap, 0, full_len).map_err(InitError::Abi)?;

    let mut net_slot: Option<usize> = None;
    let mut rng_slot: Option<usize> = None;
    // Two virtio-blk devices (ADR-0044): [0] = statefs `/state`, [1] = nxfs
    // `/data`. Captured in slot-scan order (deterministic per QEMU command).
    let mut blk_slots: [Option<usize>; 2] = [None, None];
    let mut gpu_slot: Option<usize> = None;
    let mut input_slots: [Option<usize>; 3] = [None, None, None];
    for slot in 0..MAX_SLOTS {
        let off = slot * VIRTIO_MMIO_STRIDE;
        let va = probe_va + off;
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
            for blk_slot in &mut blk_slots {
                if blk_slot.is_none() {
                    *blk_slot = Some(slot);
                    break;
                }
            }
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
            && blk_slots.iter().all(Option::is_some)
            && gpu_slot.is_some()
            && input_slots.iter().all(Option::is_some)
        {
            break;
        }
    }
    let _ = nexus_abi::cap_close(cap);
    let net_slot = net_slot.ok_or(InitError::Map("virtio-net slot not found"))?;
    let rng_slot = rng_slot.ok_or(InitError::Map("virtio-rng slot not found"))?;
    Ok((net_slot, rng_slot, blk_slots, gpu_slot, input_slots))
}

pub(crate) fn debug_write_byte(byte: u8) {
    let _ = nexus_abi::debug_putc(byte);
}

/// ONE `debug_write` per fragment (chunked at the kernel's 1 KiB cap), never
/// a `putc` per byte: the kernel serializes a whole write under the UART
/// lock, so a single-fragment marker (`SELFTEST: crash-loop cap ok\n`) can
/// never be interleaved with another task's output. The per-byte loop tore
/// init lines whenever a service printed concurrently — deterministic under
/// icount once the TASK-0321 volume spawn pass shifted init's tail, and the
/// evidence assembler reads a torn `SELFTEST:` line as an unknown marker.
pub(crate) fn debug_write_bytes(bytes: &[u8]) {
    const CHUNK: usize = 1024;
    for chunk in bytes.chunks(CHUNK) {
        if nexus_abi::debug_write(chunk).is_err() {
            for &b in chunk {
                debug_write_byte(b);
            }
        }
    }
}

pub(crate) fn debug_write_str(s: &str) {
    debug_write_bytes(s.as_bytes());
}

pub(crate) fn debug_write_hex(value: usize) {
    const NIBBLES: usize = core::mem::size_of::<usize>() * 2;
    for shift in (0..NIBBLES).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        debug_write_byte(ch);
    }
}

pub(crate) fn probe_debug_write_words() {
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

pub(crate) fn raw_probe_str(tag: &str, value: &str) {
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

pub(crate) fn log_str_ptr(tag: &str, value: &str) {
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

pub(crate) struct ServiceNameGuard<'a> {
    pub(crate) value: Option<&'a str>,
    pub(crate) ptr: usize,
    pub(crate) len: usize,
}

impl<'a> ServiceNameGuard<'a> {
    pub(crate) fn new(raw: &'a str) -> Self {
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

    pub(crate) fn trace_metadata(&self) {
        if !probes_enabled() {
            return;
        }
        debug_write_bytes(b"!svc-meta\n");
    }
}

pub(crate) fn grant_mmio_cap(
    pid: u32,
    svc_name: &str,
    cap_name: &str,
    base: usize,
    len: usize,
    pol_send: u32,
    pol_recv: u32,
    expected_slot: u32,
) -> Result<Option<bool>> {
    // Success-path grant tracing: off by default (probe topic). DENIED/err lines below are
    // ALWAYS shown. Re-enable detail via `INIT_LITE_LOG_TOPICS=probe`.
    if probes_enabled() {
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
    }

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

    if probes_enabled() {
        debug_write_bytes(b"init: mmio cap_create svc=");
        debug_write_str(svc_name);
        debug_write_byte(b'\n');
    }

    let cap = match nexus_abi::device_mmio_cap_create(base, len, usize::MAX) {
        Ok(slot) => {
            if probes_enabled() {
                debug_write_bytes(b"init: mmio cap_create ok svc=");
                debug_write_str(svc_name);
                debug_write_bytes(b" cap_slot=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
            }
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

    if probes_enabled() {
        debug_write_bytes(b"init: mmio xfer_to_slot svc=");
        debug_write_str(svc_name);
        debug_write_bytes(b" dst_slot=0x");
        debug_write_hex(expected_slot as usize);
        debug_write_byte(b'\n');
    }

    let slot = match nexus_abi::cap_transfer_to_slot(pid, cap, Rights::MAP, expected_slot) {
        Ok(slot) => {
            if probes_enabled() {
                debug_write_bytes(b"init: mmio xfer_to_slot ok svc=");
                debug_write_str(svc_name);
                debug_write_bytes(b" got=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
            }
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
    if probes_enabled() {
        debug_write_bytes(b"init: mmio grant svc=");
        debug_write_str(svc_name);
        debug_write_bytes(b" slot=0x");
        debug_write_hex(slot as usize);
        debug_write_byte(b'\n');
    }
    Ok(Some(true))
}

pub(crate) fn bundlemgrd_set_active_slot(bnd_req: u32, ask: nexus_ipc::SlotPair, slot: u8) -> bool {
    let mut req = [0u8; 5];
    nexus_abi::bundlemgrd::encode_set_active_slot_req(slot, &mut req);
    // ONE waited exchange on init's private ask inbox (TASK-0054C P2-d): queue space, then
    // bundlemgrd's answer or its death. The stash that parked foreign frames is gone with the
    // shared inbox — nothing else answers here.
    let mut buf = [0u8; 16];
    nexus_ipc::exchange::call_matching(bnd_req, ask, &req, &mut buf, |f| {
        nexus_abi::bundlemgrd::decode_set_active_slot_rsp(f)
    })
    .is_ok_and(|(status, rsp_slot)| status == nexus_abi::bundlemgrd::STATUS_OK && rsp_slot == slot)
}

pub(crate) fn decode_init_health_ok_req(frame: &[u8]) -> bool {
    decode_init_health_ok_req_with_optional_nonce(frame).is_some()
}

pub(crate) fn encode_init_health_ok_rsp(status: u8) -> [u8; 5] {
    [INIT_HEALTH_MAGIC0, INIT_HEALTH_MAGIC1, INIT_HEALTH_VERSION, INIT_HEALTH_OP_OK | 0x80, status]
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

pub(crate) fn encode_init_health_ok_rsp_with_optional_nonce(
    status: u8,
    nonce: Option<u32>,
) -> [u8; 9] {
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

pub(crate) fn updated_health_ok(upd_req: u32, ask: nexus_ipc::SlotPair) -> Result<u8> {
    let mut req = [0u8; 4];
    let len = nexus_abi::updated::encode_health_ok_req(&mut req)
        .ok_or(InitError::Map("updated health_ok encode failed"))?;
    // ONE waited exchange on init's private ask inbox (TASK-0054C P2-d). The `init: health recv
    // other op=` line this used to print was the shared inbox announcing itself; updated is the
    // only peer that answers here now, and the stash is gone with the sharing.
    let mut buf = [0u8; 16];
    let status = nexus_ipc::exchange::call_matching(upd_req, ask, &req[..len], &mut buf, |f| {
        (f.len() >= 7
            && f[0] == nexus_abi::updated::MAGIC0
            && f[1] == nexus_abi::updated::MAGIC1
            && f[2] == nexus_abi::updated::VERSION
            && f[3] == (nexus_abi::updated::OP_HEALTH_OK | 0x80))
            .then(|| f[4])
    })
    .map_err(|_| InitError::Map("updated health_ok unreachable"))?;
    if status != nexus_abi::updated::STATUS_OK {
        return Err(InitError::Map("updated health_ok failed"));
    }
    updated_get_status(upd_req, ask)
}

fn updated_get_status(upd_req: u32, ask: nexus_ipc::SlotPair) -> Result<u8> {
    let mut req = [0u8; 4];
    let len = nexus_abi::updated::encode_get_status_req(&mut req)
        .ok_or(InitError::Map("updated status encode failed"))?;
    let mut buf = [0u8; 16];
    // ADDITIVE-TAIL DISCIPLINE: this reader consumes exactly ONE byte (the active slot), so it
    // requires exactly that. Demanding the WHOLE declared payload made every additive growth of
    // bootctld's status a hard failure here.
    let (status, payload_len, n) =
        nexus_ipc::exchange::call_matching(upd_req, ask, &req[..len], &mut buf, |f| {
            (f.len() >= 7
                && f[0] == nexus_abi::updated::MAGIC0
                && f[1] == nexus_abi::updated::MAGIC1
                && f[2] == nexus_abi::updated::VERSION
                && f[3] == (nexus_abi::updated::OP_GET_STATUS | 0x80))
                .then(|| (f[4], u16::from_le_bytes([f[5], f[6]]) as usize, f.len()))
        })
        .map_err(|_| InitError::Map("updated status unreachable"))?;
    if status != nexus_abi::updated::STATUS_OK {
        return Err(InitError::Map("updated status failed"));
    }
    if payload_len < 1 || n < 8 {
        return Err(InitError::Map("updated status payload missing"));
    }
    match buf[7] {
        1 => Ok(b'a'),
        2 => Ok(b'b'),
        _ => Err(InitError::Map("updated status slot invalid")),
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

pub(crate) fn ipc_error_label(err: IpcError) -> &'static str {
    match err {
        IpcError::NoSuchEndpoint => "no-such-endpoint",
        IpcError::QueueFull => "queue-full",
        IpcError::QueueEmpty => "queue-empty",
        IpcError::PermissionDenied => "permission-denied",
        IpcError::TimedOut => "timed-out",
        IpcError::NoSpace => "no-space",
        IpcError::PeerClosed => "peer-closed",
        IpcError::Unsupported => "unsupported",
    }
}
