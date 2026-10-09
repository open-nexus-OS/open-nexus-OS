// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: screencapd wire v1 (RFC-0095 "Screen capture v1", TASK-0068) — the ONE capture
//! facade's request/reply frames in the `'S','C'` envelope. The shell's binding
//! (`svc.screencap`) and the harness speak it. Identity is the kernel sender id and the route
//! (granted only to the bundle types `shell` and `settings` through
//! `nexus.permission.SCREENCAP`), never a payload byte. `BEGIN` freezes the screen and answers
//! its size and the on-screen windows; `SHOOT` crops the frozen frame, saves it and thaws;
//! `CANCEL` thaws; `SHOT` grabs and saves without a UI (the screen or the focused window);
//! `PROBE` reports the brightest pixel of a rectangle — the selftest's non-black proof, never
//! pixels. File names travel as a STEM the shell localizes; screencapd appends `.png` and a
//! ` (n)` suffix on a collision, so a saved name is at most [`NAME_MAX_BYTES`].
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable (v1; ops append-only)
//! TEST_COVERAGE: round-trips + reject matrix below
//! RFC: docs/rfcs/RFC-0095-screen-capture-v1-readback-freeze-shell-ui.md

/// Envelope magic byte 0.
pub const MAGIC0: u8 = b'S';
/// Envelope magic byte 1.
pub const MAGIC1: u8 = b'C';
/// Wire version.
pub const VERSION: u8 = 1;

/// Freeze the screen. Reply: [`encode_begin_reply`].
pub const OP_BEGIN: u8 = 1;
/// Crop the frozen frame, save it, thaw. Reply: [`encode_saved_reply`].
pub const OP_SHOOT: u8 = 2;
/// Thaw without saving. Reply: [`encode_status`].
pub const OP_CANCEL: u8 = 3;
/// Grab and save without a UI. Reply: [`encode_saved_reply`].
pub const OP_SHOT: u8 = 4;
/// Read a rectangle without freezing; reply its brightest pixel ([`encode_probe_reply`]).
pub const OP_PROBE: u8 = 5;

/// A selection (`x, y, w, h` in display pixels).
pub const KIND_AREA: u8 = 1;
/// The whole screen.
pub const KIND_SCREEN: u8 = 2;
/// One window (`x` carries its id from the begin reply; `SHOT`: the focused one).
pub const KIND_WINDOW: u8 = 3;
/// `mode` bit: draw the pointer into the image (the low bits are the kind).
pub const MODE_POINTER: u8 = 0x80;

/// The `mode` byte of a shoot or shot: the kind, plus [`MODE_POINTER`] when the pointer is drawn.
#[must_use]
pub const fn mode(kind: u8, pointer: bool) -> u8 {
    if pointer {
        kind | MODE_POINTER
    } else {
        kind
    }
}

/// `(kind, pointer)` of a `mode` byte.
#[must_use]
pub const fn split_mode(mode: u8) -> (u8, bool) {
    (mode & !MODE_POINTER, mode & MODE_POINTER != 0)
}

/// Reply status: served.
pub const STATUS_OK: u8 = 0;
/// Reply status: the request did not decode or broke a bound (a stem, a rectangle, a kind).
pub const STATUS_MALFORMED: u8 = 1;
/// Reply status: refused — the greeter owns the display, or the sender is not allowed.
pub const STATUS_DENIED: u8 = 2;
/// Reply status: a capture is already in progress, or none is to shoot or cancel.
pub const STATUS_BUSY: u8 = 3;
/// Reply status: the display could not be read (no display, a readback failure).
pub const STATUS_FAILED: u8 = 4;
/// Reply status: the file could not be written (the store refused or is full).
pub const STATUS_STORAGE: u8 = 5;
/// Reply status: the system runs without a display (nothing to capture).
pub const STATUS_NO_DISPLAY: u8 = 6;

/// Largest file-name stem (UTF-8 bytes).
pub const STEM_MAX_BYTES: usize = 64;
/// Largest saved file name: the stem, a ` (99)` collision suffix and `.png`.
pub const NAME_MAX_BYTES: usize = STEM_MAX_BYTES + 5 + 4;
/// Most windows a begin reply names (front to back).
pub const WINDOWS_MAX: usize = 8;
/// One packed window: `id:u32, x:i16, y:i16, w:u16, h:u16`.
pub const WINDOW_BYTES: usize = 12;
/// A buffer that holds any request of this protocol.
pub const REQUEST_MAX_BYTES: usize = 4 + 1 + 4 + 2 + 2 + 2 + 1 + STEM_MAX_BYTES;
/// A buffer that holds any reply of this protocol.
pub const REPLY_MAX_BYTES: usize = 4 + 1 + 2 + 2 + 1 + 2 + WINDOWS_MAX * WINDOW_BYTES;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Begin: `[S, C, ver, OP_BEGIN, 0]`.
    request fixed encode_begin / decode_begin (op = OP_BEGIN) {
        reserved: pad(1),
    }
    /// Shoot: `[S, C, ver, OP_SHOOT, mode, x:u32, y:u16, w:u16, h:u16, stem_len, stem]`.
    request encode_shoot / decode_shoot (op = OP_SHOOT) {
        mode: u8,
        x: u32le,
        y: u16le,
        w: u16le,
        h: u16le,
        stem: str8(min = 1, max = STEM_MAX_BYTES),
    }
    /// Cancel: `[S, C, ver, OP_CANCEL, 0]`.
    request fixed encode_cancel / decode_cancel (op = OP_CANCEL) {
        reserved: pad(1),
    }
    /// Shot: `[S, C, ver, OP_SHOT, mode, stem_len, stem]`.
    request encode_shot / decode_shot (op = OP_SHOT) {
        mode: u8,
        stem: str8(min = 1, max = STEM_MAX_BYTES),
    }
    /// Probe: `[S, C, ver, OP_PROBE, x:u16, y:u16, w:u16, h:u16]`.
    request fixed encode_probe / decode_probe (op = OP_PROBE) {
        x: u16le,
        y: u16le,
        w: u16le,
        h: u16le,
    }
    /// Status-only reply: `[S, C, ver, op|0x80, status]`.
    reply fixed encode_status / decode_status (op = caller) {
        status: u8,
    }
    /// Begin reply: `[S, C, ver, op|0x80, status, w:u16, h:u16, count, packed_len:u16, packed]`
    /// — windows per [`pack_window`], front to back.
    reply encode_begin_reply / decode_begin_reply (op = caller) {
        status: u8,
        w: u16le,
        h: u16le,
        count: u8,
        packed: bytes16(min = 0, max = WINDOWS_MAX * WINDOW_BYTES),
    }
    /// Saved reply (shoot / shot): `[S, C, ver, op|0x80, status, name_len, name]`.
    reply encode_saved_reply / decode_saved_reply (op = caller) {
        status: u8,
        name: str8(min = 0, max = NAME_MAX_BYTES),
    }
    /// Probe reply: `[S, C, ver, op|0x80, status, brightest:u32]` — one pixel's BGRA.
    reply fixed encode_probe_reply / decode_probe_reply (op = caller) {
        status: u8,
        brightest: u32le,
    }
}

/// The request op of a frame in this envelope (`None` = not ours / bad header).
#[must_use]
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)
}

/// One on-screen window as the begin reply carries it (display pixels; `x`/`y` may be
/// negative for a window partly off screen).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Window {
    /// Opaque within one capture: echoed back as `x` of a window `SHOOT`.
    pub id: u32,
    /// Left edge.
    pub x: i16,
    /// Top edge.
    pub y: i16,
    /// Width.
    pub w: u16,
    /// Height.
    pub h: u16,
}

/// Appends one window to `out` at `len`; `None` when it does not fit. Returns the new length.
#[must_use]
pub fn pack_window(out: &mut [u8], len: usize, w: &Window) -> Option<usize> {
    let end = len.checked_add(WINDOW_BYTES)?;
    let slot = out.get_mut(len..end)?;
    slot[..4].copy_from_slice(&w.id.to_le_bytes());
    slot[4..6].copy_from_slice(&w.x.to_le_bytes());
    slot[6..8].copy_from_slice(&w.y.to_le_bytes());
    slot[8..10].copy_from_slice(&w.w.to_le_bytes());
    slot[10..12].copy_from_slice(&w.h.to_le_bytes());
    Some(end)
}

/// Walks the packed windows (a trailing partial entry is ignored, never half-read).
pub fn unpack_windows(packed: &[u8]) -> impl Iterator<Item = Window> + '_ {
    packed.chunks_exact(WINDOW_BYTES).take(WINDOWS_MAX).map(|c| Window {
        id: u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
        x: i16::from_le_bytes([c[4], c[5]]),
        y: i16::from_le_bytes([c[6], c[7]]),
        w: u16::from_le_bytes([c[8], c[9]]),
        h: u16::from_le_bytes([c[10], c[11]]),
    })
}

/// Whether `stem` may name a file in the screenshot folder: non-empty, within
/// [`STEM_MAX_BYTES`], no path separator, no control character, not starting with a dot (no
/// hidden files, no `..`), no surrounding whitespace.
#[must_use]
pub fn stem_is_valid(stem: &str) -> bool {
    !stem.is_empty()
        && stem.len() <= STEM_MAX_BYTES
        && !stem.starts_with('.')
        && stem.trim() == stem
        && !stem.chars().any(|c| c == '/' || c == '\\' || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_round_trip() {
        let mut buf = [0u8; REQUEST_MAX_BYTES];
        assert!(decode_begin(&encode_begin()).is_some());
        assert!(decode_cancel(&encode_cancel()).is_some());
        let area = mode(KIND_AREA, true);
        let n = encode_shoot(area, 40, 30, 640, 480, "Bildschirmfoto", &mut buf).expect("encodes");
        assert_eq!(&buf[..4], &[b'S', b'C', 1, OP_SHOOT], "envelope");
        assert_eq!(decode_request_op(&buf[..n]), Some(OP_SHOOT));
        assert_eq!(decode_shoot(&buf[..n]), Some((area, 40, 30, 640, 480, "Bildschirmfoto")));
        assert_eq!(split_mode(area), (KIND_AREA, true));
        let window = mode(KIND_WINDOW, false);
        let n = encode_shot(window, "Fenster", &mut buf).expect("encodes");
        assert_eq!(decode_shot(&buf[..n]), Some((window, "Fenster")));
        assert_eq!(split_mode(window), (KIND_WINDOW, false));
        assert_eq!(decode_probe(&encode_probe(600, 380, 64, 4)), Some((600, 380, 64, 4)));
        let r = encode_probe_reply(OP_PROBE, STATUS_OK, 0xFF20_3040);
        assert_eq!(decode_probe_reply(OP_PROBE, &r), Some((STATUS_OK, 0xFF20_3040)));
        let mut out = [0u8; REPLY_MAX_BYTES];
        let n = encode_saved_reply(OP_SHOOT, STATUS_OK, "Bildschirmfoto (2).png", &mut out)
            .expect("encodes");
        assert_eq!(
            decode_saved_reply(OP_SHOOT, &out[..n]),
            Some((STATUS_OK, "Bildschirmfoto (2).png"))
        );
    }

    #[test]
    fn windows_pack_front_to_back_and_round_trip() {
        let mut packed = [0u8; WINDOWS_MAX * WINDOW_BYTES];
        let wins = [
            Window { id: 7, x: -20, y: 36, w: 640, h: 480 },
            Window { id: 9, x: 700, y: 36, w: 580, h: 764 },
        ];
        let mut len = 0;
        for w in &wins {
            len = pack_window(&mut packed, len, w).expect("fits");
        }
        let mut out = [0u8; REPLY_MAX_BYTES];
        let n = encode_begin_reply(OP_BEGIN, STATUS_OK, 1280, 800, 2, &packed[..len], &mut out)
            .expect("encodes");
        let (status, w, h, count, body) = decode_begin_reply(OP_BEGIN, &out[..n]).expect("ok");
        assert_eq!((status, w, h, count), (STATUS_OK, 1280, 800, 2));
        let back: [Window; 2] = {
            let mut it = unpack_windows(body);
            [it.next().expect("first"), it.next().expect("second")]
        };
        assert_eq!(back, wins);
        let full = [Window::default(); WINDOWS_MAX];
        let mut len = 0;
        for w in &full {
            len = pack_window(&mut packed, len, w).expect("eight fit");
        }
        assert_eq!(pack_window(&mut packed, len, &full[0]), None, "a ninth does not");
    }

    #[test]
    fn test_reject_stems_that_could_leave_the_folder_or_hide() {
        for bad in
            ["", ".", "..", ".hidden", "a/b", "a\\b", "line\nbreak", " pad", "pad ", "\u{7f}"]
        {
            assert!(!stem_is_valid(bad), "{bad:?}");
        }
        assert!(!stem_is_valid(&"x".repeat(STEM_MAX_BYTES + 1)), "over the bound");
        for good in ["Bildschirmfoto vom 2026-10-08 14-32-05", "Écran", "x"] {
            assert!(stem_is_valid(good), "{good:?}");
        }
    }

    #[test]
    fn test_reject_bounds_truncation_and_foreign_frames() {
        let mut buf = [0u8; REQUEST_MAX_BYTES + 8];
        assert_eq!(encode_shot(KIND_SCREEN, "", &mut buf), None, "an empty stem");
        let long = "s".repeat(STEM_MAX_BYTES + 1);
        assert_eq!(encode_shot(KIND_SCREEN, &long, &mut buf), None, "over the stem bound");
        let n = encode_shot(KIND_SCREEN, "ab", &mut buf).expect("encodes");
        assert_eq!(decode_shot(&buf[..n - 1]), None, "truncated");
        assert_eq!(decode_shoot(&buf[..n]), None, "op mismatch");
        let mut foreign = encode_begin();
        foreign[0] = b'C';
        assert_eq!(decode_request_op(&foreign), None, "another protocol's envelope");
        let mut out = [0u8; REPLY_MAX_BYTES + 16];
        let too_many = [0u8; (WINDOWS_MAX + 1) * WINDOW_BYTES];
        assert_eq!(
            encode_begin_reply(OP_BEGIN, STATUS_OK, 1, 1, 9, &too_many, &mut out),
            None,
            "more windows than the bound"
        );
    }
}
