// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The window outlives its compositor (RFC-0112, amendment 2026-11-02; TASK-0348
//! Phase 6). The app's event channel goes quiet in one of two ways: the compositor closed the
//! window ON PURPOSE — it says so first (`OP_SURFACE_CLOSED`) and the app saves and ends — or the
//! compositor died: an EOF with no CLOSED before it. Then the app waits in the kernel, without a
//! clock, for its parent's word that a new compositor runs (`OP_COMPOSITOR_BACK`, a fresh SEND of
//! the channel moved along: the app gave its last one to the dead compositor), attaches the
//! channel again under a new nonce and asks for its geometry; the event loop then re-creates the
//! surface (a new buffer from gpud) and presents the whole window. The one attach boot and a
//! rejoin share lives here, and the rule every receive on the channel follows: a capability that
//! moves with a BACK is kept for the rejoin — it is the only way back — any other is closed.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the restart lane (`APPHOST: compositor back (reattached)`)

use core::sync::atomic::{AtomicU32, Ordering};

use nexus_display_proto::surface_restart::is_compositor_back;
use nexus_display_proto::surface_settings::{decode_surface_settings, SURFACE_SETTINGS_FRAME_MAX};

use super::*;

/// No BACK is kept.
const NONE: u32 = u32::MAX;

/// The SEND a BACK moved along, kept until the rejoin takes it (a newer BACK replaces it). While
/// the app holds it the channel has a sender, so a wait that received the BACK must end itself.
static KEPT: AtomicU32 = AtomicU32::new(NONE);

/// Every receive on the event channel hands the capability that moved with `frame` here: a
/// BACK's SEND is kept for the rejoin, any other is closed. `true` when `frame` was a BACK.
pub(super) fn take_cap(frame: &[u8], cap: Option<u32>) -> bool {
    let Some(cap) = cap else { return false };
    if !is_compositor_back(frame) {
        let _ = nexus_abi::cap_close(cap);
        return false;
    }
    let older = KEPT.swap(cap, Ordering::Relaxed);
    if older != NONE {
        let _ = nexus_abi::cap_close(older);
    }
    true
}

/// Whether a BACK waits for the rejoin.
pub(super) fn back_kept() -> bool {
    KEPT.load(Ordering::Relaxed) != NONE
}

fn take_kept() -> Option<u32> {
    let cap = KEPT.swap(NONE, Ordering::Relaxed);
    (cap != NONE).then_some(cap)
}

/// The attach (TASK-0054C P2-d): the SEND at `send_cap` moves to the compositor with the nonce
/// it binds the channel to the surface by; the compositor is the channel's sender from now on.
/// Waits for queue space — a dead compositor ends the wait. `false`: not taken, the capability
/// is still the caller's.
pub(super) fn attach(send_cap: u32, nonce: u64) -> bool {
    let frame = wire::encode_surface_events(nonce);
    nexus_ipc::exchange::send_with_cap(WINDOWD_SEND_SLOT, &frame, send_cap).is_ok()
}

/// One frame the event loop takes as if it had just arrived.
pub(super) struct Replay {
    buf: [u8; SURFACE_SETTINGS_FRAME_MAX],
    len: usize,
}

impl Replay {
    fn of(frame: &[u8]) -> Option<Self> {
        let mut buf = [0u8; SURFACE_SETTINGS_FRAME_MAX];
        buf.get_mut(..frame.len())?.copy_from_slice(frame);
        Some(Self { buf, len: frame.len() })
    }

    /// Copies the frame into `out`; its length.
    pub(super) fn copy_into(&self, out: &mut [u8]) -> usize {
        let n = self.len.min(out.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        n
    }
}

/// What a rejoin brings back: the new compositor's content rect `(inset, w, h)` — the event loop
/// re-creates the surface at it — and the presentation snapshot it pushed at the attach, when
/// one came first (the loop applies it like any other).
pub(super) struct Rejoined {
    pub(super) rect: (u16, u16, u16),
    pub(super) snapshot: Option<Replay>,
}

/// The compositor died: waits for the new one and joins it under a fresh nonce (`win.nonce`).
/// A new compositor that dies before it answered is waited out the same way. `None`: the event
/// channel itself is gone — nothing will ever come back on it.
pub(super) fn rejoin(
    client: &KernelClient,
    events: &KernelClient,
    win: &mut WindowIntent,
) -> Option<Rejoined> {
    loop {
        let send = match take_kept() {
            Some(cap) => cap,
            None => await_back(events)?,
        };
        win.nonce = fresh_nonce(win.nonce);
        if !attach(send, win.nonce) {
            // Gone again before the attach: the next BACK brings the next one.
            let _ = nexus_abi::cap_close(send);
            continue;
        }
        let intent = win.frame();
        if client.send(&intent, Wait::Blocking).is_err() {
            continue;
        }
        if let Some(rejoined) = await_rect(events) {
            return Some(rejoined);
        }
    }
}

/// WAITS for the BACK — no EOF asked (the channel has no sender until it comes) and no clock:
/// the parent sends it once a new compositor is wired. `None`: the channel is gone.
fn await_back(events: &KernelClient) -> Option<u32> {
    let mut frame = [0u8; 16];
    loop {
        let (len, cap) = events.recv_into_with_cap(Wait::Blocking, &mut frame, false).ok()?;
        if take_cap(&frame[..len.min(frame.len())], cap) {
            return take_kept();
        }
    }
}

/// WAITS for the new compositor's content rect (EOF asked: it may die too). The snapshot it
/// pushed at the attach is kept for the loop; anything else is for no window yet. `None`: it
/// died, or a BACK came (kept: the rejoin starts over with it).
fn await_rect(events: &KernelClient) -> Option<Rejoined> {
    let mut frame = [0u8; 96];
    let mut snapshot = None;
    loop {
        let (len, cap) = events.recv_into_with_cap(Wait::Blocking, &mut frame, true).ok()?;
        let f = &frame[..len.min(frame.len())];
        if take_cap(f, cap) {
            return None;
        }
        if let Some((_, inset, w, h)) = wire::decode_surface_rect(f) {
            return Some(Rejoined { rect: (inset, w, h), snapshot });
        }
        if decode_surface_settings(f).is_some() {
            snapshot = Replay::of(f);
        }
    }
}

/// A nonce no earlier attach of this app used.
fn fresh_nonce(old: u64) -> u64 {
    nsec().unwrap_or(0) ^ old.rotate_left(17) ^ 0x9E37_79B9_7F4A_7C15
}
