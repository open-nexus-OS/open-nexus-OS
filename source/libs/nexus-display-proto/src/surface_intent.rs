// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The window intent an app declares to windowd and the content rect windowd answers
//! (`OP_SURFACE_INTENT` / `OP_SURFACE_RECT`, both numbered in `crate::client_surface`), and the
//! app's window size: a default and a minimum (RFC-0117), carried by the intent and by
//! `SURFACE_CREATE`. Fixed-layout, length-guarded codecs — no allocation.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below + windowd `window_size` host tests
//!
//! The sizes are bounded by their type (`u16`); windowd clamps them to the work area (RFC-0117
//! §windowd), so a peer cannot make it allocate a surface larger than the display.

use crate::client_surface::{OP_SURFACE_INTENT, OP_SURFACE_RECT};
use crate::envelope::{has_op, header, HEADER_LEN};

/// The bytes of a [`WindowSize`] on the wire.
pub const WINDOW_SIZE_LEN: usize = 8;

/// The app's window size (RFC-0117), in px at desktop scale; `0` on an axis = absent (the window
/// manager decides). Wire: `width:u16, height:u16, min_width:u16, min_height:u16` (LE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowSize {
    /// The default content width of a freeform window.
    pub width: u16,
    /// The default content height of a freeform window.
    pub height: u16,
    /// The smallest content width the app tolerates.
    pub min_width: u16,
    /// The smallest content height the app tolerates.
    pub min_height: u16,
}

impl WindowSize {
    #[must_use]
    pub fn encode(&self) -> [u8; WINDOW_SIZE_LEN] {
        let mut b = [0u8; WINDOW_SIZE_LEN];
        b[0..2].copy_from_slice(&self.width.to_le_bytes());
        b[2..4].copy_from_slice(&self.height.to_le_bytes());
        b[4..6].copy_from_slice(&self.min_width.to_le_bytes());
        b[6..8].copy_from_slice(&self.min_height.to_le_bytes());
        b
    }

    #[must_use]
    pub fn decode(b: &[u8; WINDOW_SIZE_LEN]) -> Self {
        let at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        Self { width: at(0), height: at(2), min_width: at(4), min_height: at(6) }
    }

    /// The size at `frame[at..at + 8]` (the caller checked the frame's length).
    pub(crate) fn read(frame: &[u8], at: usize) -> Option<Self> {
        let bytes: &[u8; WINDOW_SIZE_LEN] = frame.get(at..at + WINDOW_SIZE_LEN)?.try_into().ok()?;
        Some(Self::decode(bytes))
    }
}

// ------------------------------------------------------------------ intent

/// Intent carries the client's event-channel NONCE (same correlation contract
/// as `OP_SURFACE_EVENTS`/`OP_SURFACE_CREATE`): the composed content-rect
/// REPLY must reach the asking client's own channel — without it, concurrent
/// mounts stole each other's answer and every app fell back to the probe size
/// (boot-proven `apphost: no content rect (fallback)` ×3, 2026-07-10).
/// +8: the app's [`WindowSize`] (RFC-0117).
pub const SURFACE_INTENT_FRAME_LEN: usize = HEADER_LEN + 12 + WINDOW_SIZE_LEN;

/// The window intent of `OP_SURFACE_INTENT` (the `WIN_*` tags of `crate::client_surface`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceIntent {
    pub style: u8,
    pub level: u8,
    pub mode: u8,
    pub resizable: bool,
    pub nonce: u64,
    pub size: WindowSize,
}

#[must_use]
pub fn encode_surface_intent(i: &SurfaceIntent) -> [u8; SURFACE_INTENT_FRAME_LEN] {
    let mut f = [0u8; SURFACE_INTENT_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_INTENT));
    f[4] = i.style;
    f[5] = i.level;
    f[6] = i.mode;
    f[7] = u8::from(i.resizable);
    f[8..16].copy_from_slice(&i.nonce.to_le_bytes());
    f[16..24].copy_from_slice(&i.size.encode());
    f
}

/// Length-exact: a frame without the size (the pre-RFC-0117 form) is malformed.
#[must_use]
pub fn decode_surface_intent(frame: &[u8]) -> Option<SurfaceIntent> {
    if !has_op(frame, OP_SURFACE_INTENT) || frame.len() != SURFACE_INTENT_FRAME_LEN {
        return None;
    }
    Some(SurfaceIntent {
        style: frame[4],
        level: frame[5],
        mode: frame[6],
        resizable: frame[7] != 0,
        nonce: u64::from_le_bytes(frame[8..16].try_into().ok()?),
        size: WindowSize::read(frame, 16)?,
    })
}

// -------------------------------------------------------------------- rect

pub const SURFACE_RECT_FRAME_LEN: usize = HEADER_LEN + 8;

/// Encodes the WM-composed content rect (`x, y, w, h`) for the app's surface.
#[must_use]
pub fn encode_surface_rect(x: u16, y: u16, w: u16, h: u16) -> [u8; SURFACE_RECT_FRAME_LEN] {
    let mut f = [0u8; SURFACE_RECT_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_RECT));
    f[4..6].copy_from_slice(&x.to_le_bytes());
    f[6..8].copy_from_slice(&y.to_le_bytes());
    f[8..10].copy_from_slice(&w.to_le_bytes());
    f[10..12].copy_from_slice(&h.to_le_bytes());
    f
}

/// `(x, y, w, h)`.
#[must_use]
pub fn decode_surface_rect(frame: &[u8]) -> Option<(u16, u16, u16, u16)> {
    if !has_op(frame, OP_SURFACE_RECT) || frame.len() != SURFACE_RECT_FRAME_LEN {
        return None;
    }
    Some((
        u16::from_le_bytes([frame[4], frame[5]]),
        u16::from_le_bytes([frame[6], frame[7]]),
        u16::from_le_bytes([frame[8], frame[9]]),
        u16::from_le_bytes([frame[10], frame[11]]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::{
        OP_SURFACE_CREATE, WIN_LEVEL_DESKTOP, WIN_LEVEL_NORMAL, WIN_MODE_AUTO, WIN_MODE_FULLSCREEN,
        WIN_STYLE_PLAIN, WIN_STYLE_TITLEBAR,
    };

    #[test]
    fn intent_round_trip_and_guards() {
        let i = SurfaceIntent {
            style: WIN_STYLE_PLAIN,
            level: WIN_LEVEL_DESKTOP,
            mode: WIN_MODE_FULLSCREEN,
            resizable: false,
            nonce: 0xDEAD_BEEF,
            size: WindowSize::default(),
        };
        let f = encode_surface_intent(&i);
        assert_eq!(decode_surface_intent(&f), Some(i));
        let d = SurfaceIntent {
            style: WIN_STYLE_TITLEBAR,
            level: WIN_LEVEL_NORMAL,
            mode: WIN_MODE_AUTO,
            resizable: true,
            nonce: 7,
            size: WindowSize::default(),
        };
        assert_eq!(decode_surface_intent(&encode_surface_intent(&d)), Some(d));
        assert_eq!(decode_surface_intent(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_CREATE;
        assert_eq!(decode_surface_intent(&wrong), None);
    }

    #[test]
    fn intent_carries_the_apps_window_size() {
        let size = WindowSize { width: 340, height: 600, min_width: 340, min_height: 600 };
        let i = SurfaceIntent {
            style: WIN_STYLE_PLAIN,
            level: WIN_LEVEL_NORMAL,
            mode: WIN_MODE_AUTO,
            resizable: true,
            nonce: 9,
            size,
        };
        assert_eq!(decode_surface_intent(&encode_surface_intent(&i)).map(|d| d.size), Some(size));
        assert_eq!(WindowSize::decode(&size.encode()), size);
    }

    #[test]
    fn test_reject_an_intent_without_its_size() {
        let f = encode_surface_intent(&SurfaceIntent {
            style: 0,
            level: 0,
            mode: 0,
            resizable: true,
            nonce: 1,
            size: WindowSize { width: 1, height: 1, min_width: 1, min_height: 1 },
        });
        // The pre-RFC-0117 frame (12 payload bytes) is malformed, not read with a zero size.
        assert_eq!(decode_surface_intent(&f[..HEADER_LEN + 12]), None);
    }

    #[test]
    fn rect_round_trip_and_guards() {
        let f = encode_surface_rect(0, 0, 640, 480);
        assert_eq!(decode_surface_rect(&f), Some((0, 0, 640, 480)));
        assert_eq!(decode_surface_rect(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_INTENT;
        assert_eq!(decode_surface_rect(&wrong), None);
    }
}
