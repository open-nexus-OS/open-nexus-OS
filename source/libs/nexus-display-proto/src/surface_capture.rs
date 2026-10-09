// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: screen capture on windowd's surface endpoint (RFC-0095, TASK-0068, ADR-0071).
//!
//! - `OP_SURFACE_CAPTURE = 32` (screencapd → windowd, accepted from screencapd's kernel sid
//!   only): `[hdr:4][cmd][nonce:u32][x:u16][y:u16][w:u16][h:u16]`. `ATTACH` lends screencapd's
//!   frame VMO once (it moves in; fire-and-forget — one capability per message, and a request
//!   with a reply cap has no slot left for it). `FREEZE` reads the screen into that VMO and
//!   freezes on the frame; `THAW` ends the freeze; `PROBE` reads the rectangle without freezing
//!   (the selftest's path).
//! - The reply rides the request's reply cap: `[hdr:4 (op | 0x80)][status][nonce:u32][w:u16]
//!   [h:u16][pointer x:i16][pointer y:i16][hot x:u8][hot y:u8][sprite w:u8][sprite h:u8][n:u8]`
//!   then `n` windows `{id:u32, x:i16, y:i16, w:u16, h:u16}` (front to back, `n ≤
//!   CAPTURE_WINDOWS_MAX`). After a freeze the VMO holds the frame (`w × h`, rows tight, BGRA)
//!   and right behind it the pointer's sprite (`sprite w × sprite h`, rows tight) — the
//!   compositor's own sprite, so the capture service never keeps a second copy of the cursors.
//! - `OP_SURFACE_CAPTURE_KEY = 30` (windowd → the desktop surface): `[hdr:4][kind][seq:u32]` —
//!   Print (`KEY_UI`), Shift+Print (`KEY_SCREEN`), Alt+Print (`KEY_WINDOW`). Retained and re-sent
//!   until delivered; `seq` grows per key press so a re-send is recognised.
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: unit tests below

use crate::client_surface::{has_op, header, HEADER_LEN};

/// screencapd → windowd: the capture verb.
pub const OP_SURFACE_CAPTURE: u8 = 32;
/// windowd → screencapd: the capture reply (`OP_SURFACE_CAPTURE | 0x80`).
pub const OP_SURFACE_CAPTURE_REPLY: u8 = OP_SURFACE_CAPTURE | 0x80;
/// windowd → the desktop surface: a capture key was pressed.
pub const OP_SURFACE_CAPTURE_KEY: u8 = 30;

/// Read the screen into the attached VMO and freeze it on that frame.
pub const CAPTURE_FREEZE: u8 = 1;
/// End the freeze.
pub const CAPTURE_THAW: u8 = 2;
/// Read a rectangle without freezing (into the attached VMO).
pub const CAPTURE_PROBE: u8 = 3;
/// Lend screencapd's frame VMO (it moves in, windowd keeps it; a later attach replaces it).
/// Fire-and-forget: one capability per message, so the VMO cannot ride a request whose reply
/// cap already takes the slot — the freeze and the probe read into the VMO attached here.
pub const CAPTURE_ATTACH: u8 = 4;

/// Done.
pub const CAPTURE_OK: u8 = 0;
/// Refused: not screencapd, or the greeter is up.
pub const CAPTURE_DENIED: u8 = 1;
/// A freeze while frozen, or a thaw while not frozen.
pub const CAPTURE_BUSY: u8 = 2;
/// The readback failed (no device, a ring timeout, a too-small VMO).
pub const CAPTURE_FAILED: u8 = 3;
/// The frame broke a bound.
pub const CAPTURE_MALFORMED: u8 = 4;
/// windowd runs display-less (no framebuffer was granted): nothing to read.
pub const CAPTURE_NO_DISPLAY: u8 = 5;

/// The most windows a freeze reply names (front to back).
pub const CAPTURE_WINDOWS_MAX: usize = 8;

/// Print: open the screenshot UI.
pub const CAPTURE_KEY_UI: u8 = 1;
/// Shift+Print: save the screen, no UI.
pub const CAPTURE_KEY_SCREEN: u8 = 2;
/// Alt+Print: save the focused window, no UI.
pub const CAPTURE_KEY_WINDOW: u8 = 3;

/// The largest pointer sprite a freeze writes behind the frame (side, pixels).
pub const CAPTURE_SPRITE_MAX: usize = 64;

/// Request length.
pub const SURFACE_CAPTURE_FRAME_LEN: usize = HEADER_LEN + 1 + 4 + 8;
/// Reply length without windows.
pub const CAPTURE_REPLY_HEAD_LEN: usize = HEADER_LEN + 1 + 4 + 4 + 4 + 4 + 1;
/// One window entry in the reply.
pub const CAPTURE_WINDOW_LEN: usize = 12;
/// The longest reply.
pub const CAPTURE_REPLY_MAX_LEN: usize =
    CAPTURE_REPLY_HEAD_LEN + CAPTURE_WINDOWS_MAX * CAPTURE_WINDOW_LEN;
/// Capture-key push length.
pub const SURFACE_CAPTURE_KEY_FRAME_LEN: usize = HEADER_LEN + 1 + 4;

/// A rectangle in display pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// A capture request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureRequest {
    pub cmd: u8,
    pub nonce: u32,
    pub rect: CaptureRect,
}

/// A window on screen at the frozen moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureWindow {
    pub id: u32,
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

/// The pointer at the frozen moment: position, hotspot and the size of the sprite written
/// behind the frame (`w == 0`: none — the pointer is not drawn into a capture).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapturePointer {
    pub x: i16,
    pub y: i16,
    pub hot_x: u8,
    pub hot_y: u8,
    pub w: u8,
    pub h: u8,
}

impl CapturePointer {
    /// The sprite's bytes behind the frame (rows tight, BGRA).
    #[must_use]
    pub const fn sprite_bytes(&self) -> usize {
        self.w as usize * self.h as usize * 4
    }
}

/// A capture reply (the windows ride separately, see [`decode_capture_reply`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureReply {
    pub status: u8,
    pub nonce: u32,
    pub w: u16,
    pub h: u16,
    pub pointer: CapturePointer,
}

/// Encodes a request.
#[must_use]
pub fn encode_capture_request(req: &CaptureRequest) -> [u8; SURFACE_CAPTURE_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CAPTURE_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CAPTURE));
    f[HEADER_LEN] = req.cmd;
    f[5..9].copy_from_slice(&req.nonce.to_le_bytes());
    f[9..11].copy_from_slice(&req.rect.x.to_le_bytes());
    f[11..13].copy_from_slice(&req.rect.y.to_le_bytes());
    f[13..15].copy_from_slice(&req.rect.w.to_le_bytes());
    f[15..17].copy_from_slice(&req.rect.h.to_le_bytes());
    f
}

/// Fail-closed decode: exact length, a known command.
#[must_use]
pub fn decode_capture_request(frame: &[u8]) -> Option<CaptureRequest> {
    if !has_op(frame, OP_SURFACE_CAPTURE) || frame.len() != SURFACE_CAPTURE_FRAME_LEN {
        return None;
    }
    let cmd = frame[HEADER_LEN];
    if !matches!(cmd, CAPTURE_FREEZE | CAPTURE_THAW | CAPTURE_PROBE | CAPTURE_ATTACH) {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([frame[i], frame[i + 1]]);
    Some(CaptureRequest {
        cmd,
        nonce: u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]),
        rect: CaptureRect { x: u16_at(9), y: u16_at(11), w: u16_at(13), h: u16_at(15) },
    })
}

/// Encodes a reply into `out`; returns its length, `None` when `out` is short, there are more
/// than [`CAPTURE_WINDOWS_MAX`] windows or the sprite is wider or taller than
/// [`CAPTURE_SPRITE_MAX`].
#[must_use]
pub fn encode_capture_reply(
    reply: &CaptureReply,
    windows: &[CaptureWindow],
    out: &mut [u8],
) -> Option<usize> {
    let sprite_max = CAPTURE_SPRITE_MAX as u8;
    if windows.len() > CAPTURE_WINDOWS_MAX
        || reply.pointer.w > sprite_max
        || reply.pointer.h > sprite_max
    {
        return None;
    }
    let len = CAPTURE_REPLY_HEAD_LEN + windows.len() * CAPTURE_WINDOW_LEN;
    let f = out.get_mut(..len)?;
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CAPTURE_REPLY));
    f[4] = reply.status;
    f[5..9].copy_from_slice(&reply.nonce.to_le_bytes());
    f[9..11].copy_from_slice(&reply.w.to_le_bytes());
    f[11..13].copy_from_slice(&reply.h.to_le_bytes());
    f[13..15].copy_from_slice(&reply.pointer.x.to_le_bytes());
    f[15..17].copy_from_slice(&reply.pointer.y.to_le_bytes());
    f[17] = reply.pointer.hot_x;
    f[18] = reply.pointer.hot_y;
    f[19] = reply.pointer.w;
    f[20] = reply.pointer.h;
    f[21] = windows.len() as u8;
    for (i, w) in windows.iter().enumerate() {
        let at = CAPTURE_REPLY_HEAD_LEN + i * CAPTURE_WINDOW_LEN;
        f[at..at + 4].copy_from_slice(&w.id.to_le_bytes());
        f[at + 4..at + 6].copy_from_slice(&w.x.to_le_bytes());
        f[at + 6..at + 8].copy_from_slice(&w.y.to_le_bytes());
        f[at + 8..at + 10].copy_from_slice(&w.w.to_le_bytes());
        f[at + 10..at + 12].copy_from_slice(&w.h.to_le_bytes());
    }
    Some(len)
}

/// Fail-closed decode: the window count must match the length exactly. Fills `windows` and
/// returns the reply and the window count.
#[must_use]
pub fn decode_capture_reply(
    frame: &[u8],
    windows: &mut [CaptureWindow; CAPTURE_WINDOWS_MAX],
) -> Option<(CaptureReply, usize)> {
    if !has_op(frame, OP_SURFACE_CAPTURE_REPLY) || frame.len() < CAPTURE_REPLY_HEAD_LEN {
        return None;
    }
    let n = usize::from(frame[21]);
    if n > CAPTURE_WINDOWS_MAX || frame.len() != CAPTURE_REPLY_HEAD_LEN + n * CAPTURE_WINDOW_LEN {
        return None;
    }
    let (sprite_w, sprite_h) = (frame[19], frame[20]);
    if usize::from(sprite_w) > CAPTURE_SPRITE_MAX || usize::from(sprite_h) > CAPTURE_SPRITE_MAX {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([frame[i], frame[i + 1]]);
    let i16_at = |i: usize| i16::from_le_bytes([frame[i], frame[i + 1]]);
    let reply = CaptureReply {
        status: frame[4],
        nonce: u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]),
        w: u16_at(9),
        h: u16_at(11),
        pointer: CapturePointer {
            x: i16_at(13),
            y: i16_at(15),
            hot_x: frame[17],
            hot_y: frame[18],
            w: sprite_w,
            h: sprite_h,
        },
    };
    for (i, slot) in windows.iter_mut().take(n).enumerate() {
        let at = CAPTURE_REPLY_HEAD_LEN + i * CAPTURE_WINDOW_LEN;
        *slot = CaptureWindow {
            id: u32::from_le_bytes([frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]),
            x: i16_at(at + 4),
            y: i16_at(at + 6),
            w: u16_at(at + 8),
            h: u16_at(at + 10),
        };
    }
    Some((reply, n))
}

/// Encodes the capture-key push.
#[must_use]
pub fn encode_capture_key(kind: u8, seq: u32) -> [u8; SURFACE_CAPTURE_KEY_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CAPTURE_KEY_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CAPTURE_KEY));
    f[HEADER_LEN] = kind;
    f[5..9].copy_from_slice(&seq.to_le_bytes());
    f
}

/// Fail-closed decode: exact length, a known key. Returns `(kind, seq)`.
#[must_use]
pub fn decode_capture_key(frame: &[u8]) -> Option<(u8, u32)> {
    if !has_op(frame, OP_SURFACE_CAPTURE_KEY) || frame.len() != SURFACE_CAPTURE_KEY_FRAME_LEN {
        return None;
    }
    let kind = frame[HEADER_LEN];
    if !matches!(kind, CAPTURE_KEY_UI | CAPTURE_KEY_SCREEN | CAPTURE_KEY_WINDOW) {
        return None;
    }
    Some((kind, u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let req = CaptureRequest {
            cmd: CAPTURE_PROBE,
            nonce: 0xA1B2_C3D4,
            rect: CaptureRect { x: 600, y: 380, w: 64, h: 4 },
        };
        let f = encode_capture_request(&req);
        assert_eq!(decode_capture_request(&f), Some(req));
        let freeze = CaptureRequest { cmd: CAPTURE_FREEZE, nonce: 1, rect: CaptureRect::default() };
        assert_eq!(decode_capture_request(&encode_capture_request(&freeze)), Some(freeze));
    }

    #[test]
    fn replies_round_trip_with_windows() {
        let reply = CaptureReply {
            status: CAPTURE_OK,
            nonce: 7,
            w: 1280,
            h: 800,
            pointer: CapturePointer { x: -3, y: 799, hot_x: 4, hot_y: 2, w: 32, h: 32 },
        };
        let wins = [
            CaptureWindow { id: 2, x: -20, y: 40, w: 640, h: 480 },
            CaptureWindow { id: 5, x: 700, y: 36, w: 580, h: 764 },
        ];
        let mut out = [0u8; CAPTURE_REPLY_MAX_LEN];
        let n = encode_capture_reply(&reply, &wins, &mut out).expect("encodes");
        let mut got = [CaptureWindow::default(); CAPTURE_WINDOWS_MAX];
        let (back, count) = decode_capture_reply(&out[..n], &mut got).expect("decodes");
        assert_eq!((back, count), (reply, 2));
        assert_eq!(&got[..2], &wins);
    }

    #[test]
    fn capture_keys_round_trip() {
        for kind in [CAPTURE_KEY_UI, CAPTURE_KEY_SCREEN, CAPTURE_KEY_WINDOW] {
            assert_eq!(decode_capture_key(&encode_capture_key(kind, 42)), Some((kind, 42)));
        }
    }

    #[test]
    fn test_reject_unknown_commands_keys_and_lengths() {
        let mut f = encode_capture_request(&CaptureRequest {
            cmd: CAPTURE_THAW,
            nonce: 0,
            rect: CaptureRect::default(),
        });
        f[HEADER_LEN] = 9;
        assert_eq!(decode_capture_request(&f), None, "unknown command");
        let ok = encode_capture_request(&CaptureRequest {
            cmd: CAPTURE_THAW,
            nonce: 0,
            rect: CaptureRect::default(),
        });
        assert_eq!(decode_capture_request(&ok[..ok.len() - 1]), None, "truncated");
        let mut key = encode_capture_key(CAPTURE_KEY_UI, 1);
        key[HEADER_LEN] = 0;
        assert_eq!(decode_capture_key(&key), None, "kind 0 is no key");
        assert_eq!(decode_capture_key(&encode_capture_key(CAPTURE_KEY_UI, 1)[..8]), None);
    }

    #[test]
    fn test_reject_lying_window_counts_and_overfull_replies() {
        let reply = CaptureReply::default();
        let mut out = [0u8; CAPTURE_REPLY_MAX_LEN];
        let n = encode_capture_reply(&reply, &[CaptureWindow::default()], &mut out).expect("one");
        out[21] = 2; // claims two windows, carries one
        let mut got = [CaptureWindow::default(); CAPTURE_WINDOWS_MAX];
        assert_eq!(decode_capture_reply(&out[..n], &mut got), None);
        out[21] = 1;
        out[19] = (CAPTURE_SPRITE_MAX + 1) as u8; // a sprite wider than any cursor
        assert_eq!(decode_capture_reply(&out[..n], &mut got), None);
        let nine = [CaptureWindow::default(); CAPTURE_WINDOWS_MAX + 1];
        assert_eq!(encode_capture_reply(&reply, &nine, &mut out), None, "over the window bound");
        let mut short = [0u8; CAPTURE_REPLY_HEAD_LEN - 1];
        assert_eq!(encode_capture_reply(&reply, &[], &mut short), None, "short buffer");
    }
}
