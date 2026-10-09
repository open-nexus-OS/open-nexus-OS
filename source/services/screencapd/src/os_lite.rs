// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the screencapd service loop (OS target, RFC-0095). At start it allocates the frame
//! VMO (the largest display plus the pointer sprite) and the PNG encoder's scratch once, lends
//! a clone of the frame VMO to windowd (`CAPTURE_ATTACH`, fire-and-forget) and says
//! `screencapd: ready`. Then one request at a time: `BEGIN` asks windowd to freeze (the reply
//! names the frame's size, the pointer and the windows) and reads the pointer's sprite from
//! behind the frame; `SHOOT` / `SHOT` resolve a rectangle and save it (`save_os`), then thaw;
//! `CANCEL` thaws; `PROBE` reads a rectangle without freezing and answers its brightest pixel.
//! A frame VMO windowd lost (a restarted compositor answers `FAILED`) is lent again and the
//! request retried once. Replies ride the moved reply cap; markers carry kinds and sizes only,
//! the first `LINES_MAX` of each kind per boot.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU — `screencapd: ready`, `SELFTEST: ui v7 screencap ok` (the probe), the
//!   usb-visible lane's `screencapd: freeze ok` / `saved` / `SELFTEST: ui v7 screenshot ok`

use alloc::vec::Vec;

use nexus_display_proto::surface_capture as cap;
use nexus_display_proto::LAYOUT_MAX;
use nexus_ipc::{KernelServer, Wait};
use nexus_service_topology::slots::screencapd as slots;
use nexus_wire::screencapd as wire;

use crate::plan::{self, Frozen, Pointer};

pub type Result<T> = core::result::Result<T, &'static str>;

/// The largest pointer sprite windowd writes behind the frame.
const SPRITE_BYTES: usize = cap::CAPTURE_SPRITE_MAX * cap::CAPTURE_SPRITE_MAX * 4;
/// The frame VMO: the largest display (`LAYOUT_MAX`, BGRA, rows tight) and the sprite.
const FRAME_BYTES: usize = (LAYOUT_MAX.0 as usize) * (LAYOUT_MAX.1 as usize) * 4 + SPRITE_BYTES;
/// Lines of each marker kind per boot (a user taking many screenshots must not flood the log).
const LINES_MAX: u32 = 8;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// What one capture leg answered.
type Captured = (cap::CaptureReply, [cap::CaptureWindow; cap::CAPTURE_WINDOWS_MAX], usize);

pub(crate) struct Service {
    /// The frame VMO (ours; a clone is lent to windowd, gpud writes through another).
    pub(crate) frame: u32,
    pub(crate) encoder: png_encode::Encoder,
    /// The pointer sprite of the frozen frame.
    pub(crate) sprite: Vec<u8>,
    /// One display row (the probe's reads).
    row: Vec<u8>,
    frozen: Option<Frozen>,
    nonce: u32,
    freeze_lines: u32,
    saved_lines: u32,
    deny_lines: u32,
    ui_proof_said: bool,
}

pub fn service_main_loop() -> Result<()> {
    let server =
        KernelServer::new_with_slots(slots::SERVER.recv, slots::SERVER.send).map_err(|_| "bind")?;
    let frame = nexus_abi::vmo_create(FRAME_BYTES).map_err(|_| "frame vmo")?;
    let encoder = png_encode::Encoder::new(LAYOUT_MAX.0).map_err(|_| "encoder scratch")?;
    let mut svc = Service {
        frame,
        encoder,
        sprite: alloc::vec![0u8; SPRITE_BYTES],
        row: alloc::vec![0u8; LAYOUT_MAX.0 as usize * 4],
        frozen: None,
        nonce: 0,
        freeze_lines: 0,
        saved_lines: 0,
        deny_lines: 0,
        ui_proof_said: false,
    };
    if !svc.attach() {
        emit("screencapd: FAIL frame attach");
    }
    let _ = nexus_service_entry::ready("screencapd: ready");
    nexus_abi::service_verdict_flush("screencapd");

    // ONE request and one reply buffer for the service lifetime.
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    let mut out = alloc::vec![0u8; wire::REPLY_MAX_BYTES];
    let mut pending = nexus_ipc::PendingReply::new();
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    loop {
        match server.serve_next(&mut pending, Wait::Blocking, &mut recv_frame) {
            Ok((_hdr, frame_len, _sender, reply)) => {
                breaker.on_success();
                let frame = &recv_frame[..frame_len.min(recv_frame.len())];
                let n = svc.answer(frame, &mut out);
                match reply {
                    Some(reply) if n > 0 => pending.park(reply, &out[..n]),
                    // A cap with nothing to say is dropped (closes); no cap, no reply.
                    _ => {}
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err("disconnected"),
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit("screencapd: recv error (transient)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = nexus_abi::yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        return Err("endpoint defect");
                    }
                }
            }
        }
    }
}

impl Service {
    /// One request → one reply in `out`; returns its length (0: nothing to answer).
    fn answer(&mut self, frame: &[u8], out: &mut [u8]) -> usize {
        let Some(op) = wire::decode_request_op(frame) else {
            return 0;
        };
        let status = |out: &mut [u8], s: u8| {
            let r = wire::encode_status(op, s);
            out[..r.len()].copy_from_slice(&r);
            r.len()
        };
        match op {
            wire::OP_BEGIN if wire::decode_begin(frame).is_some() => self.begin(out),
            wire::OP_SHOOT => match wire::decode_shoot(frame) {
                Some((mode, x, y, w, h, stem)) => {
                    let (kind, pointer) = wire::split_mode(mode);
                    let saved = self.shoot(kind, pointer, (x, u32::from(y)), (w, h), stem);
                    self.saved_reply(op, saved, out)
                }
                None => status(out, wire::STATUS_MALFORMED),
            },
            wire::OP_CANCEL if wire::decode_cancel(frame).is_some() => {
                let s = if self.frozen.is_some() {
                    self.thaw();
                    wire::STATUS_OK
                } else {
                    wire::STATUS_BUSY
                };
                status(out, s)
            }
            wire::OP_SHOT => match wire::decode_shot(frame) {
                Some((mode, stem)) => {
                    let (kind, pointer) = wire::split_mode(mode);
                    let saved = self.shot(kind, pointer, stem);
                    self.saved_reply(op, saved, out)
                }
                None => status(out, wire::STATUS_MALFORMED),
            },
            wire::OP_PROBE => match wire::decode_probe(frame) {
                Some((x, y, w, h)) => {
                    let (s, brightest) = self.probe(cap::CaptureRect { x, y, w, h });
                    let r = wire::encode_probe_reply(op, s, brightest);
                    out[..r.len()].copy_from_slice(&r);
                    r.len()
                }
                None => status(out, wire::STATUS_MALFORMED),
            },
            _ => status(out, wire::STATUS_MALFORMED),
        }
    }

    /// Lends a clone of the frame VMO to windowd (it keeps it; a later attach replaces it).
    fn attach(&mut self) -> bool {
        let Ok(clone) = nexus_abi::cap_clone(self.frame) else {
            return false;
        };
        self.nonce = self.nonce.wrapping_add(1);
        let req = cap::CaptureRequest {
            cmd: cap::CAPTURE_ATTACH,
            nonce: self.nonce,
            rect: cap::CaptureRect::default(),
        };
        let frame = cap::encode_capture_request(&req);
        if nexus_ipc::exchange::send_with_cap(slots::WINDOWD.send, &frame, clone).is_err() {
            let _ = nexus_abi::cap_close(clone);
            return false;
        }
        true
    }

    /// One capture leg to windowd; a lost frame VMO is lent again and the leg retried once.
    fn capture(&mut self, cmd: u8, rect: cap::CaptureRect) -> core::result::Result<Captured, u8> {
        match self.capture_once(cmd, rect) {
            Err(wire::STATUS_FAILED) if cmd != cap::CAPTURE_THAW && self.attach() => {
                self.capture_once(cmd, rect)
            }
            other => other,
        }
    }

    fn capture_once(
        &mut self,
        cmd: u8,
        rect: cap::CaptureRect,
    ) -> core::result::Result<Captured, u8> {
        self.nonce = self.nonce.wrapping_add(1);
        let req = cap::CaptureRequest { cmd, nonce: self.nonce, rect };
        let frame = cap::encode_capture_request(&req);
        let mut rsp = [0u8; cap::CAPTURE_REPLY_MAX_LEN];
        let n = nexus_ipc::exchange::call_into(slots::WINDOWD.send, slots::REPLY, &frame, &mut rsp)
            .map_err(|_| wire::STATUS_FAILED)?;
        let mut windows = [cap::CaptureWindow::default(); cap::CAPTURE_WINDOWS_MAX];
        let (reply, count) = rsp
            .get(..n)
            .and_then(|r| cap::decode_capture_reply(r, &mut windows))
            .ok_or(wire::STATUS_FAILED)?;
        if reply.nonce != self.nonce {
            return Err(wire::STATUS_FAILED);
        }
        match reply.status {
            cap::CAPTURE_OK => Ok((reply, windows, count)),
            cap::CAPTURE_DENIED => Err(wire::STATUS_DENIED),
            cap::CAPTURE_BUSY => Err(wire::STATUS_BUSY),
            cap::CAPTURE_MALFORMED => Err(wire::STATUS_MALFORMED),
            cap::CAPTURE_NO_DISPLAY => Err(wire::STATUS_NO_DISPLAY),
            _ => Err(wire::STATUS_FAILED),
        }
    }

    /// Freezes the screen and keeps what the frozen moment showed.
    fn freeze(&mut self) -> core::result::Result<Frozen, u8> {
        if self.frozen.is_some() {
            return Err(wire::STATUS_BUSY);
        }
        let (reply, windows, count) =
            self.capture(cap::CAPTURE_FREEZE, cap::CaptureRect::default())?;
        let (w, h) = (u32::from(reply.w), u32::from(reply.h));
        let p = reply.pointer;
        let mut pointer = Pointer {
            x: i32::from(p.x),
            y: i32::from(p.y),
            hot_x: u32::from(p.hot_x),
            hot_y: u32::from(p.hot_y),
            w: u32::from(p.w),
            h: u32::from(p.h),
        };
        let sprite_len = p.sprite_bytes();
        let behind = w as usize * h as usize * 4;
        let sprite_read = sprite_len <= self.sprite.len()
            && nexus_abi::vmo_read(self.frame, behind, &mut self.sprite[..sprite_len]).is_ok();
        if !sprite_read {
            pointer.w = 0;
            pointer.h = 0;
        }
        let mut frozen = Frozen { w, h, pointer, ..Frozen::default() };
        // The windows as they show: clipped to the screen (a page outlines them in place), an
        // off-screen one dropped — the same rectangle a window shoot saves.
        let screen = frozen.screen();
        let visible = windows.iter().take(count).filter_map(|win| {
            let (x, y) = (i64::from(win.x), i64::from(win.y));
            let r = plan::clip(x, y, i64::from(win.w), i64::from(win.h), screen)?;
            let pos = |v: u32| i16::try_from(v).ok();
            let len = |v: u32| u16::try_from(v).ok();
            Some(wire::Window {
                id: win.id,
                x: pos(r.x)?,
                y: pos(r.y)?,
                w: len(r.w)?,
                h: len(r.h)?,
            })
        });
        for (slot, win) in frozen.windows.iter_mut().zip(visible) {
            *slot = win;
            frozen.window_count += 1;
        }
        self.frozen = Some(frozen);
        if self.freeze_lines < LINES_MAX {
            self.freeze_lines += 1;
            emit(&alloc::format!(
                "screencapd: freeze ok (w={w} h={h} windows={})",
                frozen.window_count
            ));
        }
        Ok(frozen)
    }

    fn thaw(&mut self) {
        if self.frozen.take().is_some() {
            let _ = self.capture(cap::CAPTURE_THAW, cap::CaptureRect::default());
        }
    }

    fn begin(&mut self, out: &mut [u8]) -> usize {
        let (s, frozen) = match self.freeze() {
            Ok(frozen) => (wire::STATUS_OK, frozen),
            Err(s) => {
                self.say_denial(s);
                (s, Frozen::default())
            }
        };
        let mut packed = [0u8; wire::WINDOWS_MAX * wire::WINDOW_BYTES];
        let mut len = 0;
        for win in frozen.windows.iter().take(frozen.window_count) {
            match wire::pack_window(&mut packed, len, win) {
                Some(next) => len = next,
                None => break,
            }
        }
        let (w, h) = (clamp16(frozen.w), clamp16(frozen.h));
        let count = (len / wire::WINDOW_BYTES) as u8;
        wire::encode_begin_reply(wire::OP_BEGIN, s, w, h, count, &packed[..len], out).unwrap_or(0)
    }

    /// `SHOOT`: the rectangle of the frozen frame, saved, then the thaw. A malformed request
    /// keeps the freeze (the UI can correct it or cancel); a failed save ends it.
    fn shoot(
        &mut self,
        kind: u8,
        pointer: bool,
        (x, y): (u32, u32),
        (w, h): (u16, u16),
        stem: &str,
    ) -> core::result::Result<crate::save_os::Saved, u8> {
        let Some(frozen) = self.frozen else {
            self.say_denial(wire::STATUS_BUSY);
            return Err(wire::STATUS_BUSY);
        };
        if !wire::stem_is_valid(stem) {
            return Err(wire::STATUS_MALFORMED);
        }
        let rect = plan::shoot_rect(kind, x, y, u32::from(w), u32::from(h), &frozen)?;
        let saved = crate::save_os::save(self, &frozen, rect, pointer, stem);
        self.thaw();
        self.note_saved(kind, rect, &saved);
        // The UI's whole path, once per boot: Print froze, a selection was picked, it is a file.
        if saved.is_ok() && kind == wire::KIND_AREA && !self.ui_proof_said {
            self.ui_proof_said = true;
            emit("SELFTEST: ui v7 screenshot ok");
        }
        saved
    }

    /// `SHOT`: freeze, save the screen or the front-most window, thaw.
    fn shot(
        &mut self,
        kind: u8,
        pointer: bool,
        stem: &str,
    ) -> core::result::Result<crate::save_os::Saved, u8> {
        if !wire::stem_is_valid(stem) {
            return Err(wire::STATUS_MALFORMED);
        }
        let frozen = self.freeze().inspect_err(|s| self.say_denial(*s))?;
        let saved = plan::shot_rect(kind, &frozen).and_then(|rect| {
            let saved = crate::save_os::save(self, &frozen, rect, pointer, stem);
            self.note_saved(kind, rect, &saved);
            saved
        });
        self.thaw();
        saved
    }

    /// `PROBE`: the brightest pixel (BGRA, as a little-endian `u32`) of a rectangle read
    /// through the one readback.
    fn probe(&mut self, rect: cap::CaptureRect) -> (u8, u32) {
        if self.frozen.is_some() {
            return (wire::STATUS_BUSY, 0);
        }
        if let Err(s) = self.capture(cap::CAPTURE_PROBE, rect) {
            return (s, 0);
        }
        let row_bytes = usize::from(rect.w) * 4;
        let Some(row) = self.row.get_mut(..row_bytes) else {
            return (wire::STATUS_MALFORMED, 0);
        };
        let mut best = [0u8; 4];
        let mut best_sum = 0u32;
        for y in 0..usize::from(rect.h) {
            if nexus_abi::vmo_read(self.frame, y * row_bytes, row).is_err() {
                return (wire::STATUS_FAILED, 0);
            }
            for px in row.chunks_exact(4) {
                let sum = u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2]);
                if sum > best_sum {
                    best_sum = sum;
                    best.copy_from_slice(px);
                }
            }
        }
        (wire::STATUS_OK, u32::from_le_bytes(best))
    }

    fn saved_reply(
        &mut self,
        op: u8,
        saved: core::result::Result<crate::save_os::Saved, u8>,
        out: &mut [u8],
    ) -> usize {
        let (s, name) = match &saved {
            Ok(saved) => (wire::STATUS_OK, saved.name()),
            Err(s) => (*s, ""),
        };
        wire::encode_saved_reply(op, s, name, out).unwrap_or(0)
    }

    fn note_saved(
        &mut self,
        kind: u8,
        rect: plan::Rect,
        saved: &core::result::Result<crate::save_os::Saved, u8>,
    ) {
        match saved {
            Ok(saved) if self.saved_lines < LINES_MAX => {
                self.saved_lines += 1;
                emit(&alloc::format!(
                    "screencapd: saved (kind={} w={} h={} bytes={})",
                    plan::kind_name(kind),
                    rect.w,
                    rect.h,
                    saved.bytes
                ));
            }
            Ok(_) => {}
            Err(s) => self.say_denial(*s),
        }
    }

    fn say_denial(&mut self, status: u8) {
        if self.deny_lines >= LINES_MAX {
            return;
        }
        self.deny_lines += 1;
        emit(match status {
            wire::STATUS_DENIED => "screencapd: deny (reason=greeter)",
            wire::STATUS_BUSY => "screencapd: deny (reason=busy)",
            wire::STATUS_STORAGE => "screencapd: deny (reason=storage)",
            wire::STATUS_FAILED => "screencapd: deny (reason=readback)",
            _ => "screencapd: deny (reason=malformed)",
        });
    }
}

fn clamp16(v: u32) -> u16 {
    v.min(u32::from(u16::MAX)) as u16
}
