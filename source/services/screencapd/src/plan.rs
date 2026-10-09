// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: screencapd's decisions (RFC-0095) — pure, host-tested. A frozen capture knows the
//! frame's size, the pointer and the windows that were on screen; a shoot names a kind and
//! (for a selection) a rectangle or (for a window) a window id from that list, and resolves to
//! the rectangle of the frame to save, clipped to the screen. A stem becomes `<stem>.png`, or
//! `<stem> (n).png` when the name is taken, in the Pictures folder's `Screenshots`.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs

use nexus_wire::screencapd as wire;

/// The Pictures folder (the home's standard top-level folder, RFC-0071 seed).
pub const PICTURES: &str = "/Bilder";
/// Where screenshots go.
pub const FOLDER: &str = "/Bilder/Screenshots";
/// The most names tried before a save gives up (`<stem> (99).png` is the last).
pub const ATTEMPTS_MAX: u32 = 99;
/// A buffer for any file name (`wire::NAME_MAX_BYTES`).
pub const NAME_BUF: usize = wire::NAME_MAX_BYTES;
/// A buffer for any file path: the folder, a slash, the name.
pub const PATH_BUF: usize = FOLDER.len() + 1 + NAME_BUF;

/// A rectangle in display pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The pointer at the frozen moment: position, hotspot, and the size of its sprite (stored
/// behind the frame in the frame VMO; `w == 0`: none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub w: u32,
    pub h: u32,
}

/// A frozen capture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frozen {
    /// The frame's size (the display mode at the frozen moment).
    pub w: u32,
    pub h: u32,
    pub pointer: Pointer,
    /// The on-screen windows, front to back.
    pub windows: [wire::Window; wire::WINDOWS_MAX],
    pub window_count: usize,
}

impl Frozen {
    /// The whole screen.
    #[must_use]
    pub const fn screen(&self) -> Rect {
        Rect { x: 0, y: 0, w: self.w, h: self.h }
    }

    /// The front-most window, clipped to the screen (the focused one: click-to-raise keeps
    /// the focused window on top).
    #[must_use]
    pub fn front_window(&self) -> Option<Rect> {
        self.windows[..self.window_count.min(wire::WINDOWS_MAX)]
            .iter()
            .find_map(|w| clip_window(w, self.screen()))
    }

    /// The window `id` from the begin reply, clipped to the screen.
    #[must_use]
    pub fn window(&self, id: u32) -> Option<Rect> {
        self.windows[..self.window_count.min(wire::WINDOWS_MAX)]
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| clip_window(w, self.screen()))
    }
}

fn clip_window(w: &wire::Window, screen: Rect) -> Option<Rect> {
    clip(i64::from(w.x), i64::from(w.y), i64::from(w.w), i64::from(w.h), screen)
}

/// The part of `x, y, w, h` inside `screen`; `None` when nothing is.
#[must_use]
pub fn clip(x: i64, y: i64, w: i64, h: i64, screen: Rect) -> Option<Rect> {
    let left = x.max(0);
    let top = y.max(0);
    let right = x.saturating_add(w).min(i64::from(screen.w));
    let bottom = y.saturating_add(h).min(i64::from(screen.h));
    if right <= left || bottom <= top {
        return None;
    }
    let to_u32 = |v: i64| u32::try_from(v).ok();
    Some(Rect {
        x: to_u32(left)?,
        y: to_u32(top)?,
        w: to_u32(right - left)?,
        h: to_u32(bottom - top)?,
    })
}

/// The rectangle a `SHOOT` saves: the selection clipped to the screen, the screen, or the
/// window `x` names. `Err` carries the reply status.
pub fn shoot_rect(kind: u8, x: u32, y: u32, w: u32, h: u32, frozen: &Frozen) -> Result<Rect, u8> {
    match kind {
        wire::KIND_SCREEN => Ok(frozen.screen()),
        wire::KIND_AREA => {
            let (x, y, w, h) = (i64::from(x), i64::from(y), i64::from(w), i64::from(h));
            clip(x, y, w, h, frozen.screen()).ok_or(wire::STATUS_MALFORMED)
        }
        wire::KIND_WINDOW => frozen.window(x).ok_or(wire::STATUS_MALFORMED),
        _ => Err(wire::STATUS_MALFORMED),
    }
}

/// The rectangle a `SHOT` (no UI) saves: the screen, or the front-most window.
pub fn shot_rect(kind: u8, frozen: &Frozen) -> Result<Rect, u8> {
    match kind {
        wire::KIND_SCREEN => Ok(frozen.screen()),
        wire::KIND_WINDOW => frozen.front_window().ok_or(wire::STATUS_MALFORMED),
        _ => Err(wire::STATUS_MALFORMED),
    }
}

/// Writes the file name of `stem` for `attempt` (1: `<stem>.png`, n: `<stem> (n).png`) into
/// `out`; returns its length. `None` for an invalid stem or an attempt outside
/// `1..=ATTEMPTS_MAX`.
#[must_use]
pub fn file_name(stem: &str, attempt: u32, out: &mut [u8; NAME_BUF]) -> Option<usize> {
    if !wire::stem_is_valid(stem) || !(1..=ATTEMPTS_MAX).contains(&attempt) {
        return None;
    }
    let mut len = 0;
    let mut push = |bytes: &[u8]| -> Option<()> {
        out.get_mut(len..len + bytes.len())?.copy_from_slice(bytes);
        len += bytes.len();
        Some(())
    };
    push(stem.as_bytes())?;
    if attempt > 1 {
        push(b" (")?;
        if attempt >= 10 {
            push(&[b'0' + (attempt / 10) as u8])?;
        }
        push(&[b'0' + (attempt % 10) as u8])?;
        push(b")")?;
    }
    push(b".png")?;
    Some(len)
}

/// Writes `FOLDER/name` into `out`; returns its length.
#[must_use]
pub fn file_path(name: &str, out: &mut [u8; PATH_BUF]) -> Option<usize> {
    let total = FOLDER.len() + 1 + name.len();
    let dst = out.get_mut(..total)?;
    dst[..FOLDER.len()].copy_from_slice(FOLDER.as_bytes());
    dst[FOLDER.len()] = b'/';
    dst[FOLDER.len() + 1..].copy_from_slice(name.as_bytes());
    Some(total)
}

/// The marker name of a kind.
#[must_use]
pub const fn kind_name(kind: u8) -> &'static str {
    match kind {
        wire::KIND_AREA => "area",
        wire::KIND_SCREEN => "screen",
        wire::KIND_WINDOW => "window",
        _ => "unknown",
    }
}
