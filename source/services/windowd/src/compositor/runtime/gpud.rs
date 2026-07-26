// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — gpud IPC client (connect/present/drain).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization): the
//! `DisplayServerRuntime` methods that own the gpud route — connect/fallback,
//! fire-and-forget present + reply drain (bump-heap-safe `recv_into`), the
//! blocking handoff status request, and the GPU-blur present. A child module of
//! `runtime`, so it reads the runtime's private fields directly; methods are
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// windowd's OWN server recv slot, published by `compositor::run` right after
/// the server binds (`KernelServer::slots().0`). `u32::MAX` = not yet known.
///
/// The gpud reply drain needs it to answer one question: is the endpoint I am
/// about to drain exclusively MINE? The ctrl-plane can answer a
/// `new_for("gpud")` route query with a recv slot that aliases this very inbox,
/// and a non-blocking drain on it consumes CLIENT requests — on 2026-07-25 a
/// 4-hart virgl `just start` (`build/logs/manual--2026-07-25T15-57-20`) ate 29
/// of them: the app-host's events-attach (len=12) — hence `desktop bind
/// deferred` with no attach left to complete it — its geometry intent (hence
/// the 8 s content-rect timeout and a 320x240 "desktop") and inputd's batches
/// (hence the `push backpressure` flood and a session that never routed input).
static SERVER_RECV_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);
/// One-shot latch for the alias FAIL line (the drain runs per present).
static ALIAS_REPORTED: AtomicBool = AtomicBool::new(false);

/// Wall-clock of the last credited present ack, and a one-shot latch for the
/// lease-expiry line. See [`PRESENT_ACK_LEASE_NS`].
static LAST_ACK_NS: AtomicU64 = AtomicU64::new(0);
static LEASE_REPORTED: AtomicBool = AtomicBool::new(false);

/// The in-flight present bound is a LEASE, not an unbounded promise.
///
/// `MAX_IN_FLIGHT` throttles presents until gpud credits them back. If credits
/// stop arriving the display freezes permanently — and on 2026-07-26 that
/// froze a BOOT: an aliased gpud reply endpoint meant no ack was ever credited,
/// windowd stopped presenting after 2 frames, and gpud never re-evaluated its
/// reveal gate (which is computed inside the present path, so even its 1.2 s
/// hard cap never ran) — the splash held forever
/// (`build/logs/manual--2026-07-26T10-31-30`).
///
/// So: after this long without a credited ack, presenting resumes anyway. A
/// stale credit costs at most a redundant frame; withholding presents costs the
/// whole session. Every precondition must terminate in a decision.
const PRESENT_ACK_LEASE_NS: u64 = 500_000_000;

/// Publishes windowd's server recv slot for the alias check above.
pub(crate) fn note_server_recv_slot(slot: u32) {
    SERVER_RECV_SLOT.store(slot, Ordering::Relaxed);
}

impl DisplayServerRuntime {
    /// Binds the gpud route, REJECTING a provably wrong answer.
    ///
    /// Routing v1 carries no reply nonce (`nexus-ipc::query_route` even drains
    /// stale `ROUTE_RSP`s to compensate), so a route query can hand back slots
    /// that are not ours. One wrong answer is detectable and catastrophic: a
    /// recv slot equal to windowd's OWN server inbox. Every gpud round-trip
    /// then reads client requests instead of gpud replies — the reply drain
    /// refuses to run, the cursor-upload ack never matches
    /// (`windowd: cursor upload failed`) and the framebuffer handoff never
    /// acks, so gpud waits for "plane0 + cursor ready" forever and the boot
    /// stays on the splash (`build/logs/manual--2026-07-26T10-31-30`; the
    /// alias appeared in ~1 of 2 interactive boots).
    ///
    /// The query is still ISSUED — `query_route` drains stale `ROUTE_RSP`s as a
    /// side effect, and dropping the call entirely is a change with no evidence
    /// behind it — but an aliased answer is discarded in favour of the pair init
    /// declared. (An earlier note here claimed an unconditional wired bind had
    /// regressed the headless framebuffer handoff; that was a mis-attribution.
    /// Every headless run binds the wired pair anyway — the G4 failures in that
    /// lane are an intermittent `gpud: resource vmo_map_page fail` on the fb
    /// attach, unrelated to route selection.)
    pub(super) fn ensure_gpud_client(&mut self) -> bool {
        if self.gpud_client.is_some() {
            return true;
        }
        if let Ok(client) = KernelClient::new_for("gpud") {
            let own_inbox = SERVER_RECV_SLOT.load(Ordering::Relaxed);
            if client.slots().1 != own_inbox {
                let _ = debug_println("windowd: gpud route connected");
                self.gpud_client = Some(client);
                return true;
            }
            let _ = debug_println(&alloc::format!(
                "windowd: FAIL gpud route answered our own inbox slot={own_inbox} — using the wired pair"
            ));
        }
        if let Ok(client) = KernelClient::new_with_slots(GPUD_WIRED_SEND_SLOT, GPUD_WIRED_RECV_SLOT)
        {
            let _ = debug_println("windowd: gpud route wired slots");
            self.gpud_client = Some(client);
            return true;
        }
        false
    }

    /// Fire-and-forget present to gpud. Pixel data is already in the VMO;
    /// gpud picks up the damage rect on its next recv iteration.
    /// Non-blocking: windowd continues processing input immediately.
    pub(super) fn send_gpud_present(&mut self, frame: &[u8]) -> bool {
        if !self.ensure_gpud_client() {
            return false;
        }
        // Drain completed present replies first so queue pressure and in-flight accounting
        // stay bounded during sustained cursor/input traffic.
        self.drain_gpud_replies();
        // Phase 6d: in-flight bound — if 2+ frames outstanding, skip this present.
        // Damage accumulates; the next successful present covers the merged region.
        const MAX_IN_FLIGHT: u32 = 2;
        if self.frames_in_flight >= MAX_IN_FLIGHT && !self.present_lease_expired() {
            return false;
        }
        let send_result = {
            let Some(client) = self.gpud_client.as_ref() else {
                return false;
            };
            client.send(frame, Wait::NonBlocking)
        };
        match send_result {
            Ok(()) => {
                self.present_seq = self.present_seq.wrapping_add(1);
                self.frames_in_flight = self.frames_in_flight.saturating_add(1);
                true
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::NoSpace) => {
                // gpud queue is currently full; caller keeps damage pending for retry.
                false
            }
            Err(err) => {
                let send_slot = self.gpud_client.as_ref().map(|c| c.slots().0).unwrap_or(0);
                log_gpud_cap_error("windowd: gpud present send failed", err, send_slot);
                self.reset_gpud_client();
                false
            }
        }
    }

    /// True when no present ack has been credited for [`PRESENT_ACK_LEASE_NS`],
    /// i.e. the in-flight credits can no longer be trusted. Clears the counter
    /// so presenting resumes (loudly, once) instead of freezing the display.
    #[cfg(nexus_env = "os")]
    fn present_lease_expired(&mut self) -> bool {
        let now = nexus_abi::nsec().unwrap_or(0);
        let last = LAST_ACK_NS.load(Ordering::Relaxed);
        if last == 0 {
            // No ack yet in this session: start the lease at the first stall so
            // a never-acking reply path is bounded from the first frame on.
            LAST_ACK_NS.store(now, Ordering::Relaxed);
            return false;
        }
        if now.saturating_sub(last) < PRESENT_ACK_LEASE_NS {
            return false;
        }
        if !LEASE_REPORTED.swap(true, Ordering::Relaxed) {
            let _ = debug_println(
                "windowd: FAIL present-ack lease expired — presenting without credits",
            );
        }
        self.frames_in_flight = 0;
        LAST_ACK_NS.store(now, Ordering::Relaxed);
        true
    }

    #[cfg(not(nexus_env = "os"))]
    fn present_lease_expired(&mut self) -> bool {
        false
    }

    /// Records a credited present ack — the lease renews on every one of them.
    pub(super) fn note_present_ack_time(&self) {
        #[cfg(nexus_env = "os")]
        LAST_ACK_NS.store(nexus_abi::nsec().unwrap_or(0), Ordering::Relaxed);
    }

    /// Drop the gpud client and reset in-flight accounting together. A stale
    /// `frames_in_flight` after a client reset would leave the counter pinned at
    /// MAX_IN_FLIGHT, blocking every future present and spinning the flush retry
    /// loop forever. Always reset both as a unit.
    pub(super) fn reset_gpud_client(&mut self) {
        self.gpud_client = None;
        self.frames_in_flight = 0;
    }

    /// Drain non-blocking gpud status replies for OP_PRESENT_DAMAGE so gpud cannot
    /// block on a full reply queue and freeze visible updates.
    ///
    /// Drains ONLY an endpoint that is exclusively ours: see [`SERVER_RECV_SLOT`]
    /// for the boot where it was not, and what that cost. A skipped drain merely
    /// stops crediting present completions (the in-flight bound throttles
    /// presents); consuming a client's request instead loses it forever, because
    /// the capability an events-attach carries cannot be re-delivered.
    pub(crate) fn drain_gpud_replies(&mut self) {
        if self.framebuffer_pending_first_write {
            return;
        }
        let Some(recv_slot) = self.gpud_client.as_ref().map(|c| c.slots().1) else {
            return;
        };
        if recv_slot == SERVER_RECV_SLOT.load(Ordering::Relaxed) {
            if !ALIAS_REPORTED.swap(true, Ordering::Relaxed) {
                let _ = debug_println(&alloc::format!(
                    "windowd: FAIL gpud reply endpoint aliases server inbox slot={recv_slot} — drain skipped"
                ));
            }
            return;
        }
        // Stack-buffer drain: recv_into avoids the per-call Vec<u8> that
        // Client::recv allocates — windowd's bump allocator never frees, so a
        // per-frame reply Vec would slowly exhaust the heap.
        let mut reply_buf = [0u8; 32];
        loop {
            let recv_result = {
                let Some(client) = self.gpud_client.as_ref() else {
                    return;
                };
                client.recv_into(Wait::NonBlocking, &mut reply_buf)
            };
            match recv_result {
                Ok(n) => {
                    let status = reply_buf.get(..n).and_then(|r| r.first()).copied();
                    if status == Some(GPUD_STATUS_OK) {
                        // Present/attach replies carry a 5-byte [status, handoff_id]
                        // payload; fire-and-forget acks (cursor move) are a single
                        // status byte and must NOT be counted as present completions
                        // or they corrupt the frames-in-flight accounting.
                        if n >= 5 {
                            self.note_present_completed();
                            self.note_present_acked_clean();
                        }
                    } else if n >= 5
                        && matches!(
                            status,
                            Some(nexus_display_proto::STATUS_MALFORMED)
                                | Some(nexus_display_proto::STATUS_DEVICE_ERROR)
                        )
                    {
                        // Present NACK (P0.3): gpud measured a failed/deadline-missed
                        // present (`gpud: FAIL present deadline`). The ROUTE is
                        // healthy — the FRAME failed. Requeue the damage (bounded,
                        // self-heal) instead of resetting the client: a reset never
                        // re-presented anything, leaving the stale/black RT on
                        // screen until unrelated damage arrived.
                        if let Some(status) = status {
                            let _ = debug_println(&alloc::format!(
                                "windowd: gpud present nack status=0x{status:02x}"
                            ));
                        }
                        self.note_present_nacked();
                    } else if nexus_display_proto::client_surface::is_client_envelope(
                        &reply_buf[..n],
                    ) {
                        // Belt to the bind-time check's braces: a CLIENT frame here
                        // means this endpoint carries someone else's requests after
                        // all (a slot the alias check could not see). Stop at once —
                        // one lost frame, loudly, instead of a silent stream.
                        let op = reply_buf.get(3).copied().unwrap_or(0);
                        let _ = debug_println(&alloc::format!(
                            "windowd: FAIL gpud reply ate client frame op={op} len={n}"
                        ));
                        self.reset_gpud_client();
                        return;
                    } else if n >= 5 {
                        // Foreign, non-client frame on the reply channel — not a
                        // gpud present verdict (real stati are 0/1/2; observed
                        // 0x30/OP_TIMER_FIRED at boot). Treating these as NACKs
                        // triggered full-recompose retry bursts during the very
                        // bring-up window where the desktop-bind handshake runs.
                        // Log (the storm is the diagnosis) + skip — no accounting
                        // change, no requeue.
                        if let Some(status) = status {
                            let _ = debug_println(&alloc::format!(
                                "windowd: gpud reply foreign frame op=0x{status:02x} len={n}"
                            ));
                        }
                    } else if n == 1 {
                        // Failed fire-and-forget op (cursor move). Soft-fail: drop to
                        // the software cursor path but keep the present pipeline alive.
                        if self.hw_cursor_active {
                            self.hw_cursor_active = false;
                            let _ = debug_println("windowd: hw cursor move rejected, sw fallback");
                        }
                    } else {
                        if let Some(status) = status {
                            let _ = debug_println(&alloc::format!(
                                "windowd: gpud present bad-status=0x{status:02x}"
                            ));
                        } else {
                            let _ = debug_println("windowd: gpud present bad-status=empty");
                        }
                        self.reset_gpud_client();
                        return;
                    }
                }
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                    return;
                }
                Err(err) => {
                    log_gpud_ipc_error("windowd: gpud present recv failed", err);
                    self.reset_gpud_client();
                    return;
                }
            }
        }
    }

    /// Blocking status request (used only for handoff/bootstrap where
    /// we must confirm gpud accepted the framebuffer VMO).
    /// (Companion of the blocking handoff variant above — kept with it.)
    #[allow(dead_code)]
    pub(super) fn send_gpud_status_request(&mut self, frame: &[u8]) -> Result<(), WindowdError> {
        // Drain any stale responses from previous non-blocking presents before
        // sending. Without this, client.recv(Blocking) below may pick up a
        // response meant for a different request, causing a chain of misrouted
        // status codes that corrupt the present pipeline.
        self.drain_gpud_replies();

        if !self.ensure_gpud_client() {
            return Err(WindowdError::InvalidDamage);
        }
        let send_result = {
            let client = self.gpud_client.as_ref().ok_or(WindowdError::InvalidDamage)?;
            client.send(frame, Wait::Blocking)
        };
        if let Err(err) = send_result {
            let send_slot = self.gpud_client.as_ref().map(|c| c.slots().0).unwrap_or(0);
            log_gpud_cap_error("windowd: gpud request send failed", err, send_slot);
            self.gpud_client = None;
            return Err(WindowdError::InvalidDamage);
        }
        let recv_result = {
            let client = self.gpud_client.as_ref().ok_or(WindowdError::InvalidDamage)?;
            client.recv(Wait::Blocking)
        };
        match recv_result {
            Ok(reply) if reply.first().copied() == Some(GPUD_STATUS_OK) => Ok(()),
            Ok(reply) => {
                if let Some(status) = reply.first().copied() {
                    let _ = debug_println(&alloc::format!(
                        "windowd: gpud request bad-status=0x{status:02x}"
                    ));
                } else {
                    let _ = debug_println("windowd: gpud request bad-status=empty");
                }
                self.gpud_client = None;
                Err(WindowdError::InvalidDamage)
            }
            Err(err) => {
                log_gpud_ipc_error("windowd: gpud request recv failed", err);
                self.gpud_client = None;
                Err(WindowdError::InvalidDamage)
            }
        }
    }

    /// Fire-and-forget: sends a frame to gpud without waiting or tracking.
    /// Used for non-critical operations (cursor upload) where the response
    /// is drained by drain_gpud_replies() on the next loop iteration.
    /// Does NOT increment frames_in_flight — not a present.
    pub(super) fn send_gpud_fire_forget(&mut self, frame: &[u8]) -> bool {
        self.drain_gpud_replies();
        if !self.ensure_gpud_client() {
            return false;
        }
        let Some(client) = self.gpud_client.as_ref() else {
            return false;
        };
        client.send(frame, Wait::NonBlocking).is_ok()
    }

    /// Non-blocking: sends damage rect to gpud and returns immediately.
    /// Pixel data is already written to the VMO by CPU compositing.
    /// gpud processes the damage asynchronously — windowd continues its loop.
    /// (Damage-rect present path — superseded by the whole-scene CB batch;
    /// kept for the compositor-scroll/damage track, see
    /// plans/webrender-compositor-scroll.md.)
    #[allow(dead_code)]
    pub(super) fn present_damage_to_gpud(&mut self, rect: DamageRect) -> bool {
        let frame = encode_gpud_damage_frame(rect);
        if self.send_gpud_present(&frame) {
            self.present_fail_reported = false;
            return true;
        }
        // Rate-limited: once per failure episode, not every retry (the retry path
        // runs at ~120 Hz during backpressure and would flood the UART log — the
        // very stall the watchdog reports cleanly).
        if !self.present_fail_reported {
            let _ = debug_println("windowd: gpud present damage failed (non-blocking, will retry)");
            self.present_fail_reported = true;
        }
        false
    }

    /// Build and send a GPU-first frame that includes BlurBackdrop commands
    /// for the glass panel region. gpud executes the blur over the CPU-composited
    /// base scene, replacing the CPU blur path in `backdrop.rs`.
    ///
    /// Phase 2: GPU-first glass panel (Workstreams 1+4).
    /// The BlurBackdrop command samples from the VMO at `DISPLAY_OFFSET_BYTES`,
    /// applies a box blur + saturation, and writes back.
    /// (GPU-first blur present variant — superseded by the virgl in-scene blur;
    /// kept: documents the BlurBackdrop wire usage.)
    #[allow(dead_code)]
    pub(super) fn present_frame_with_gpu_blur(&mut self, bounding: DamageRect) -> bool {
        let mut cmd = CommandBuffer::new();
        {
            let mut encoder = match cmd.try_begin_render_pass(RenderPassDesc {
                color_attachments: alloc::vec![],
                width: self.mode.width,
                height: self.mode.height,
            }) {
                Ok(e) => e,
                Err(_) => return false,
            };
            // Blur the combined glass panel region.
            // gpud reads from the VMO display region (offset DISPLAY_OFFSET_BYTES),
            // applies box blur, and writes the result back.
            let glass_rect =
                TileRect { x: 0, y: 0, width: COMBINED_PANEL_WIDTH as u32, height: PROOF_PANEL_H };
            if encoder
                .try_blur_backdrop(
                    glass_rect,
                    DARK_GLASS_BLUR_RADIUS,
                    DARK_GLASS_SATURATION_PERCENT,
                )
                .is_err()
            {
                // Fall back to simple damage rect if command buffer fails.
                return self.present_damage_to_gpud(bounding);
            }
            encoder.end_encoding();
        }
        let committed = match cmd.try_commit() {
            Ok(c) => c,
            Err(_) => return self.present_damage_to_gpud(bounding),
        };
        let mut frame_buf = [0u8; 256];
        let written = match committed.serialize_into(&mut frame_buf[1..]) {
            Ok(n) => n,
            Err(_) => return self.present_damage_to_gpud(bounding),
        };
        frame_buf[0] = GPU_PRESENT_DAMAGE_OP;
        self.send_gpud_present(&frame_buf[..1 + written])
    }
}
