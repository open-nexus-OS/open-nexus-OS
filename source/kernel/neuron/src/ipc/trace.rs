// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Low-noise, bounded IPC trace ring for bring-up triage
//! OWNERS: @kernel-ipc-team
//! STATUS: Experimental (debug feature only)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! This module is intentionally tiny:
//! - Records a fixed number of IPC events in-memory (no heap).
//! - Emits no UART output unless explicitly dumped.

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::ipc::IpcError;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TraceEvent {
    /// Monotonic sequence number (wraps).
    pub seq: u32,
    /// Event kind.
    pub kind: u8,
    /// Result/status (0=ok, otherwise IpcError code).
    pub status: u8,
    /// Reserved.
    pub _rsv: u16,
    /// Endpoint involved (if any).
    pub ep: u32,
    /// Flags (ipc header flags or 0).
    pub flags: u16,
    /// Payload length (bytes) or 0.
    pub len: u16,
    /// Optional extra (e.g. moved endpoint id low 32 bits).
    pub extra: u32,
}

impl TraceEvent {
    pub const fn empty() -> Self {
        Self {
            seq: 0,
            kind: 0,
            status: 0,
            _rsv: 0,
            ep: 0,
            flags: 0,
            len: 0,
            extra: 0,
        }
    }
}

const KIND_SEND: u8 = 1;
const KIND_RECV: u8 = 2;
const KIND_CAPMOVE_ALLOC: u8 = 3;
const KIND_EP_CREATE: u8 = 4;
const KIND_EP_CLOSE: u8 = 5;
const KIND_CAP_XFER: u8 = 6;
const KIND_CAPMOVE_SEND: u8 = 7;

// Power-of-two ring size for cheap masking.
// Keep this large enough to span "bring-up -> failure" without overwriting,
// but keep the dump output bounded (see DUMP_COUNT).
const RING_SIZE: usize = 8192;
const RING_MASK: usize = RING_SIZE - 1;
const DUMP_COUNT: usize = 1024;

static WRITE_SEQ: AtomicUsize = AtomicUsize::new(0);
static mut RING: [TraceEvent; RING_SIZE] = [TraceEvent::empty(); RING_SIZE];

// Low-noise guard: avoid dumping the full trace ring repeatedly for the same missing endpoint.
// The first dump is usually enough to diagnose the lifecycle; repeated dumps can drown UART and
// perturb timing-sensitive bring-up tests.
static LAST_NOSUCH_EP_DUMP: AtomicUsize = AtomicUsize::new(usize::MAX);

// One-shot dump trigger for "large CAP_MOVE send" triage (e.g. OTA stage request).
static CAPMOVE_BIG_DUMPED: AtomicUsize = AtomicUsize::new(0);
static CAPMOVE_BIG_RECV_DUMPED: AtomicUsize = AtomicUsize::new(0);

pub fn maybe_dump_capmove_big(tag: &str) {
    if CAPMOVE_BIG_DUMPED.swap(1, Ordering::Relaxed) == 0 {
        dump_uart(tag);
    }
}

pub fn maybe_dump_capmove_big_recv(tag: &str) {
    if CAPMOVE_BIG_RECV_DUMPED.swap(1, Ordering::Relaxed) == 0 {
        dump_uart(tag);
    }
}

#[inline]
fn err_code(err: Option<IpcError>) -> u8 {
    match err {
        None => 0,
        Some(IpcError::NoSuchEndpoint) => 1,
        Some(IpcError::QueueFull) => 2,
        Some(IpcError::QueueEmpty) => 3,
        Some(IpcError::PermissionDenied) => 4,
        Some(IpcError::TimedOut) => 5,
        Some(IpcError::NoSpace) => 6,
    }
}

#[inline]
fn push(mut ev: TraceEvent) {
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    ev.seq = seq as u32;
    let idx = seq & RING_MASK;
    unsafe {
        RING[idx] = ev;
    }
}

pub fn record_send(
    pid: u32,
    cap_slot: u16,
    ep: u32,
    flags: u16,
    len: u16,
    err: Option<IpcError>,
) {
    push(TraceEvent {
        kind: KIND_SEND,
        status: err_code(err),
        _rsv: cap_slot,
        ep,
        flags,
        len,
        extra: pid,
        ..TraceEvent::empty()
    });
}

pub fn record_recv(
    pid: u32,
    cap_slot: u16,
    ep: u32,
    flags: u16,
    len: u16,
    err: Option<IpcError>,
) {
    push(TraceEvent {
        kind: KIND_RECV,
        status: err_code(err),
        _rsv: cap_slot,
        ep,
        flags,
        len,
        extra: pid,
        ..TraceEvent::empty()
    });
}

pub fn record_capmove_alloc(pid: u32, ep: u32, allocated_slot: u32, moved_ep: u32) {
    push(TraceEvent {
        kind: KIND_CAPMOVE_ALLOC,
        status: 0,
        ep,
        // Encode receiver PID low16 to correlate alloc events with service tasks.
        flags: pid as u16,
        len: 0,
        extra: moved_ep,
        _rsv: allocated_slot as u16,
        ..TraceEvent::empty()
    });
}

pub fn record_ep_create(pid: u32, ep: u32, depth: u16, owner_pid: u16) {
    push(TraceEvent {
        kind: KIND_EP_CREATE,
        status: 0,
        ep,
        flags: depth,
        len: owner_pid,
        extra: pid,
        ..TraceEvent::empty()
    });
}

pub fn record_ep_close(pid: u32, ep: u32) {
    push(TraceEvent {
        kind: KIND_EP_CLOSE,
        status: 0,
        ep,
        flags: 0,
        len: 0,
        extra: pid,
        ..TraceEvent::empty()
    });
}

pub fn record_cap_xfer(src_pid: u32, dst_pid: u32, ep: u32, rights: u16) {
    push(TraceEvent {
        kind: KIND_CAP_XFER,
        status: 0,
        ep,
        flags: rights,
        len: dst_pid as u16,
        extra: src_pid,
        ..TraceEvent::empty()
    });
}

pub fn record_capmove_send(pid: u32, send_slot: u16, moved_slot: u16, dst_ep: u32, moved_ep: u32) {
    push(TraceEvent {
        kind: KIND_CAPMOVE_SEND,
        status: 0,
        _rsv: send_slot,
        ep: dst_ep,
        // Encode sender PID low16 for quick correlation in dumps.
        flags: pid as u16,
        len: moved_slot,
        extra: moved_ep,
        ..TraceEvent::empty()
    });
}

pub fn dump_uart(tag: &str) {
    use core::fmt::Write as _;
    let mut u = crate::uart::raw_writer();
    let _ = writeln!(u, "IPC-TRACE dump tag={}", tag);
    let end = WRITE_SEQ.load(Ordering::Relaxed);
    let start = end.saturating_sub(DUMP_COUNT);
    for seq in start..end {
        let idx = seq & RING_MASK;
        let ev = unsafe { RING[idx] };
        if ev.seq != seq as u32 {
            continue;
        }
        let kind = match ev.kind {
            KIND_SEND => "send",
            KIND_RECV => "recv",
            KIND_CAPMOVE_ALLOC => "capalloc",
            KIND_EP_CREATE => "epnew",
            KIND_EP_CLOSE => "epclose",
            KIND_CAP_XFER => "capxfer",
            KIND_CAPMOVE_SEND => "capmove",
            _ => "unk",
        };
        // status: 0=ok, otherwise code
        let _ = writeln!(
            u,
            "IPC-TRACE {} seq=0x{:x} slot=0x{:x} ep=0x{:x} flags=0x{:x} len=0x{:x} st=0x{:x} x=0x{:x}",
            kind,
            ev.seq,
            ev._rsv,
            ev.ep,
            ev.flags,
            ev.len,
            ev.status,
            ev.extra
        );
    }
}

pub fn dump_uart_send_nosuch(ep: u32) {
    let prev = LAST_NOSUCH_EP_DUMP.swap(ep as usize, Ordering::Relaxed);
    if prev == ep as usize {
        return;
    }
    use core::fmt::Write as _;
    let mut u = crate::uart::raw_writer();
    let _ = writeln!(u, "IPC-TRACE nosuch ep=0x{:x}", ep);
    dump_uart("send-nosuch");
    let end = WRITE_SEQ.load(Ordering::Relaxed);
    // Scan the full ring for lifecycle events related to this endpoint.
    let start = end.saturating_sub(RING_SIZE);
    for seq in start..end {
        let idx = seq & RING_MASK;
        let ev = unsafe { RING[idx] };
        if ev.seq != seq as u32 {
            continue;
        }
        if ev.ep != ep {
            continue;
        }
        match ev.kind {
            KIND_EP_CREATE | KIND_EP_CLOSE | KIND_CAP_XFER => {}
            _ => continue,
        }
        let kind = match ev.kind {
            KIND_EP_CREATE => "epnew",
            KIND_EP_CLOSE => "epclose",
            KIND_CAP_XFER => "capxfer",
            _ => "unk",
        };
        let _ = writeln!(
            u,
            "IPC-TRACE match {} seq=0x{:x} slot=0x{:x} ep=0x{:x} flags=0x{:x} len=0x{:x} st=0x{:x} x=0x{:x}",
            kind,
            ev.seq,
            ev._rsv,
            ev.ep,
            ev.flags,
            ev.len,
            ev.status,
            ev.extra
        );
    }
}
