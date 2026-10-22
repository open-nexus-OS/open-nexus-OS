// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADR-0042 client-surface transport frames — the wire contract an
//! app process speaks to windowd: `SURFACE_CREATE` (moves the app's surface
//! VMO capability with the message), `SURFACE_PRESENT` (seq + bounded damage
//! rects), `SURFACE_DESTROY`. Acks return over the app's dedicated reply
//! channel. Fixed-layout, length-guarded codecs — no allocation.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below + windowd `client_surface` host tests
//!
//! ENVELOPE: these frames arrive on windowd's SERVER endpoint, which speaks
//! the `[b'I', b'N', version, op]`-header family (input-live-protocol).
//! Ops share that space; MUST NOT collide with input ops 1–4 (pinned by a
//! windowd unit test). Text ops 21/22: `crate::surface_text` (23 reserved).

// The envelope itself lives in `crate::envelope` (one home for "is this frame
// mine?"); re-exported here so `client_surface::ENVELOPE_*` stays the path.
pub(crate) use crate::envelope::{has_op, header, HEADER_LEN};
pub use crate::envelope::{is_client_envelope, ENVELOPE_MAGIC0, ENVELOPE_MAGIC1, ENVELOPE_VERSION};

/// Creates an app surface. Payload: `w:u16, h:u16, format:u8`. The message
/// MOVES the app's surface VMO capability (gpud-attach pattern).
pub const OP_SURFACE_CREATE: u8 = 8;
/// Presents damage. Payload: `surface_id:u32, seq:u32, count:u8,
/// (x:u16,y:u16,w:u16,h:u16) * count`.
pub const OP_SURFACE_PRESENT: u8 = 9;
/// Destroys a surface. Payload: `surface_id:u32`.
pub const OP_SURFACE_DESTROY: u8 = 10;
/// windowd → app: an input event routed to the surface (R3). Payload:
/// `surface_id:u32, kind:u8, x:u16, y:u16` — surface-LOCAL body pixels.
pub const OP_SURFACE_INPUT: u8 = 11;
/// execd → windowd: attaches the app's DEDICATED event channel. The message
/// MOVES the channel's SEND capability; windowd retains it and delivers ALL
/// app-bound frames (input events + surface acks) on it. Replaces the shared
/// `window_rsp` delivery, which raced with inputd's ack drain — a tap sent on
/// the shared channel could be consumed by ANY receiver (the R3 "buttons do
/// nothing" bug). Header-only frame.
pub const OP_SURFACE_EVENTS: u8 = 12;
/// app → windowd: the app's WINDOW INTENT, sent BEFORE `SURFACE_CREATE` so the
/// WM can compose the frame + geometry and hand back the content rect the app
/// sizes its surface VMO to. Payload: `style:u8, level:u8, mode:u8,
/// resizable:u8` (the `WIN_*` tags). windowd stores it for the pending surface
/// and answers `OP_SURFACE_RECT` on the event channel. See
/// docs/dev/ui/patterns/windowing/window-intent.md (`chrome = intent ⟂ policy`).
pub const OP_SURFACE_INTENT: u8 = 13;
/// windowd → app: the content rect the WM composed for the app's intent under
/// the active windowing policy (the app owns no geometry). Payload:
/// `x:u16, y:u16, w:u16, h:u16`. Sent once before create (initial size) and
/// again on every mode/resize change (the general geometry channel — no
/// separate "query display mode" op).
pub const OP_SURFACE_RECT: u8 = 14;

/// RETIRED (RFC-0109, number reserved forever — never reuse): the standalone
/// material-region list. A surface's regions ride its structure now
/// (`crate::surface_layers::OP_SURFACE_TREE`), with its scroll layers and patches.
pub const OP_SURFACE_LAYERS: u8 = 15;

/// Material kinds of a region (`LayerDesc`, carried by `OP_SURFACE_TREE`).
pub const MATERIAL_OPAQUE: u8 = 0;
pub const MATERIAL_GLASS: u8 = 1;
/// Glass levels — the design-system glass tokens (panel/card/subtle/window),
/// mapped to a backdrop blur radius by the compositor.
pub const GLASS_PANEL: u8 = 0;
pub const GLASS_CARD: u8 = 1;
pub const GLASS_SUBTLE: u8 = 2;
pub const GLASS_WINDOW: u8 = 3;
/// Modal/sheet overlay material (append-only wire id).
pub const GLASS_OVERLAY: u8 = 4;

/// Max material regions per surface (bounds the structure; a real shell uses a
/// handful: topbar, dock, launcher panel, a few cards).
pub const MAX_SURFACE_LAYERS: usize = 16;

/// One material-tagged region of a client surface (base-layer pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerDesc {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// `MATERIAL_*`.
    pub material: u8,
    /// `GLASS_*` (only meaningful when `material == MATERIAL_GLASS`).
    pub glass_level: u8,
    /// Corner radius in px (0 = square).
    pub radius: u8,
    /// Drop-shadow alpha (0 = no shadow).
    pub shadow_alpha: u8,
}

/// RETIRED (RFC-0083 P5, number reserved forever — never reuse): the
/// standalone theme push. Theme mode + accent now travel in the versioned
/// presentation snapshot (`surface_settings::OP_SURFACE_SETTINGS = 26`).
pub const OP_SURFACE_THEME: u8 = 16;
/// The theme byte (mode, accent, solid materials) — its own module (`theme_byte.rs`).
pub use crate::theme_byte::{
    pack_theme, theme_is_solid, unpack_theme, THEME_ACCENT_SHIFT, THEME_DARK, THEME_LIGHT,
    THEME_MODE_MASK, THEME_SOLID,
};

/// RETIRED (RFC-0083 P5, number reserved forever — never reuse): the
/// standalone shell-profile push, folded into the presentation snapshot.
pub const OP_SURFACE_PROFILE: u8 = 17;
/// Shell profiles (align with the DSL `device.profile` vocabulary; the
/// snapshot's `profile` field).
pub const PROFILE_TABLET: u8 = 0;
pub const PROFILE_DESKTOP: u8 = 1;
pub const PROFILE_PHONE: u8 = 2;
pub const PROFILE_TV: u8 = 3;

/// app → windowd: a WINDOW control request (`CONTROL_WIN_*` + the launch
/// wait-ring hint). Settings are NOT controls any more (RFC-0083): theme,
/// accent and shell mode route to settingsd like every other key and come
/// back as watch events. Payload: `control:u8, value:u8`.
pub const OP_SURFACE_CONTROL: u8 = 18;

/// Frame pulse (the Choreographer contract): a client that is ANIMATING
/// (scroll ease, fling) sends `OP_SURFACE_FRAME_REQ` — a ONE-SHOT request —
/// and the compositor answers with ONE `OP_SURFACE_FRAME` pulse on the
/// client's event channel after its next composited frame. The client ticks
/// its physics on the pulse and re-requests while motion continues. One-shot
/// semantics bound the traffic to the animation's lifetime — no idle pulses,
/// no client-side timer guessing (recv-timeout self-pacing measured ~3-12Hz;
/// the pulse rides the REAL frame cadence).
pub const OP_SURFACE_FRAME_REQ: u8 = 19;
/// The compositor's pulse answering `OP_SURFACE_FRAME_REQ`.
pub const OP_SURFACE_FRAME: u8 = 20;

pub const SURFACE_FRAME_FRAME_LEN: usize = HEADER_LEN + 4;

#[must_use]
pub fn encode_surface_frame_req(surface_id: u32) -> [u8; SURFACE_FRAME_FRAME_LEN] {
    let mut f = [0u8; SURFACE_FRAME_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_FRAME_REQ));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f
}

#[must_use]
pub fn decode_surface_frame_req(frame: &[u8]) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_FRAME_REQ) || frame.len() != SURFACE_FRAME_FRAME_LEN {
        return None;
    }
    Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]))
}

#[must_use]
pub fn encode_surface_frame(surface_id: u32) -> [u8; SURFACE_FRAME_FRAME_LEN] {
    let mut f = [0u8; SURFACE_FRAME_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_FRAME));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f
}

#[must_use]
pub fn decode_surface_frame(frame: &[u8]) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_FRAME) || frame.len() != SURFACE_FRAME_FRAME_LEN {
        return None;
    }
    Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]))
}
// Control kinds (`CONTROL_*`) live in `crate::control` (split out when the
// window-control vocabulary outgrew this file); re-exported below so
// `client_surface::CONTROL_*` stays the path.
pub use crate::control::*;

pub const SURFACE_CONTROL_FRAME_LEN: usize = HEADER_LEN + 2;

/// Encodes a control request.
#[must_use]
pub fn encode_surface_control(control: u8, value: u8) -> [u8; SURFACE_CONTROL_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CONTROL_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CONTROL));
    f[4] = control;
    f[5] = value;
    f
}

/// `(control, value)`.
#[must_use]
pub fn decode_surface_control(frame: &[u8]) -> Option<(u8, u8)> {
    if !has_op(frame, OP_SURFACE_CONTROL) || frame.len() != SURFACE_CONTROL_FRAME_LEN {
        return None;
    }
    Some((frame[4], frame[5]))
}

/// Window-intent wire tags (mirror the IR `WindowStyle`/`WindowLevel`/
/// `WindowMode` enum ordinals; both ends agree on these, not the capnp type).
pub const WIN_STYLE_TITLEBAR: u8 = 0;
pub const WIN_STYLE_HIDDEN_TITLEBAR: u8 = 1;
pub const WIN_STYLE_PLAIN: u8 = 2;
pub const WIN_LEVEL_NORMAL: u8 = 0;
pub const WIN_LEVEL_DESKTOP: u8 = 1;
pub const WIN_LEVEL_OVERLAY: u8 = 2;
pub const WIN_MODE_AUTO: u8 = 0;
pub const WIN_MODE_FREEFORM: u8 = 1;
pub const WIN_MODE_FULLSCREEN: u8 = 2;
/// `CONTROL_WIN_MODE`-only presentation value (NOT an intent): snap the
/// window to the left work-area half. `WIN_MODE_AUTO` in that control =
/// toggle fullscreen ⇄ freeform (the title-bar zoom semantics).
pub const WIN_MODE_SPLIT: u8 = 3;

/// The input kinds `OP_SURFACE_INPUT` carries (taps, hover, wheel, drag).
pub use crate::surface_input::{
    wheel_delta_from_wire, wheel_delta_to_wire, INPUT_KIND_DRAG, INPUT_KIND_LEAVE, INPUT_KIND_MOVE,
    INPUT_KIND_RELEASE, INPUT_KIND_TAP, INPUT_KIND_WHEEL,
};

/// Pixel format tags. v1: BGRA8888 only.
pub const FORMAT_BGRA8888: u8 = 0;

/// Bounded damage list per present (ADR-0042).
pub const MAX_DAMAGE_RECTS: usize = 4;

/// Ack status codes (reply frames).
pub const SURFACE_STATUS_OK: u8 = 0;
pub const SURFACE_STATUS_MALFORMED: u8 = 1;
pub const SURFACE_STATUS_DENIED: u8 = 2;
pub const SURFACE_STATUS_QUOTA: u8 = 3;
pub const SURFACE_STATUS_BAD_SURFACE: u8 = 4;
pub const SURFACE_STATUS_BAD_SEQ: u8 = 5;
/// A create that placed the surface's BASE but not its store (no atlas room for it, RFC-0109):
/// the surface exists — the ack's value is its id — and the client paints its scroll layers flat
/// and declares no band and no patch.
pub const SURFACE_STATUS_NO_STORE: u8 = 6;

/// One damage rect in surface-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

// ------------------------------------------------------------------ create

// +4 intent bytes (style, level, mode, resizable): the declared window intent
// rides ATOMICALLY with the create — a separate pre-create OP_SURFACE_INTENT
// raced when two app-hosts connected concurrently (shell + counter: the
// pending intent crossed and BOTH surfaces landed in the desktop role).
//
// +4 rows and store bytes (RFC-0109): the surface is `width × rows` — rows
// `[0, height)` the base layer, rows `[height, rows)` the client's store (its
// scroll layers' bands and its wash patches), of which columns `[0, store_w)`
// are used. windowd places the base and the store as two atlas regions at
// create, so both sizes ride atomically.
pub const SURFACE_CREATE_FRAME_LEN: usize = HEADER_LEN + 21;

/// A surface create: its size, its declared window intent, the client's event-channel nonce and
/// its store (RFC-0109).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCreate {
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub style: u8,
    pub level: u8,
    pub mode: u8,
    pub resizable: bool,
    pub nonce: u64,
    /// The surface's rows: `height ≤ rows ≤ MAX_SURFACE_ROWS`.
    pub rows: u16,
    /// The store's used width (`≤ width`): `0` exactly when there is no store (`rows ==
    /// height`).
    pub store_w: u16,
}

#[must_use]
pub fn encode_surface_create(c: &SurfaceCreate) -> [u8; SURFACE_CREATE_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CREATE_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CREATE));
    f[4..6].copy_from_slice(&c.width.to_le_bytes());
    f[6..8].copy_from_slice(&c.height.to_le_bytes());
    f[8] = c.format;
    f[9] = c.style;
    f[10] = c.level;
    f[11] = c.mode;
    f[12] = c.resizable as u8;
    f[13..21].copy_from_slice(&c.nonce.to_le_bytes());
    f[21..23].copy_from_slice(&c.rows.to_le_bytes());
    f[23..25].copy_from_slice(&c.store_w.to_le_bytes());
    f
}

/// Length-exact; a create whose `rows` is below its `height` or past
/// [`crate::surface_layers::MAX_SURFACE_ROWS`], whose store is wider than the surface, or which
/// names a store without rows for it (or rows without a store) is malformed.
#[must_use]
pub fn decode_surface_create(frame: &[u8]) -> Option<SurfaceCreate> {
    if !has_op(frame, OP_SURFACE_CREATE) || frame.len() != SURFACE_CREATE_FRAME_LEN {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let (width, height, rows, store_w) = (u16_at(4), u16_at(6), u16_at(21), u16_at(23));
    if rows < height
        || rows > crate::surface_layers::MAX_SURFACE_ROWS
        || store_w > width
        || (rows > height) != (store_w > 0)
    {
        return None;
    }
    let mut nonce = [0u8; 8];
    nonce.copy_from_slice(&frame[13..21]);
    Some(SurfaceCreate {
        width,
        height,
        format: frame[8],
        style: frame[9],
        level: frame[10],
        mode: frame[11],
        resizable: frame[12] != 0,
        nonce: u64::from_le_bytes(nonce),
        rows,
        store_w,
    })
}

// ------------------------------------------------------------ intent + rect

/// Intent carries the client's event-channel NONCE (same correlation contract
/// as `OP_SURFACE_EVENTS`/`OP_SURFACE_CREATE`): the composed content-rect
/// REPLY must reach the asking client's own channel — without it, concurrent
/// mounts stole each other's answer and every app fell back to the probe size
/// (boot-proven `apphost: no content rect (fallback)` ×3, 2026-07-10).
pub const SURFACE_INTENT_FRAME_LEN: usize = HEADER_LEN + 12;

/// Encodes the app's window intent (`style, level, mode, resizable, nonce`).
#[must_use]
pub fn encode_surface_intent(
    style: u8,
    level: u8,
    mode: u8,
    resizable: bool,
    nonce: u64,
) -> [u8; SURFACE_INTENT_FRAME_LEN] {
    let mut f = [0u8; SURFACE_INTENT_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_INTENT));
    f[4] = style;
    f[5] = level;
    f[6] = mode;
    f[7] = u8::from(resizable);
    f[8..16].copy_from_slice(&nonce.to_le_bytes());
    f
}

/// `(style, level, mode, resizable, nonce)`.
#[must_use]
pub fn decode_surface_intent(frame: &[u8]) -> Option<(u8, u8, u8, bool, u64)> {
    if !has_op(frame, OP_SURFACE_INTENT) || frame.len() != SURFACE_INTENT_FRAME_LEN {
        return None;
    }
    let nonce = u64::from_le_bytes(frame[8..16].try_into().ok()?);
    Some((frame[4], frame[5], frame[6], frame[7] != 0, nonce))
}

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

// ----------------------------------------------------------------- present

pub const SURFACE_PRESENT_MAX_LEN: usize = HEADER_LEN + 9 + MAX_DAMAGE_RECTS * 8;

/// Encodes a present frame; `damage` is clamped to [`MAX_DAMAGE_RECTS`].
/// Returns the frame length.
#[must_use]
pub fn encode_surface_present(
    surface_id: u32,
    seq: u32,
    damage: &[DamageRect],
    out: &mut [u8; SURFACE_PRESENT_MAX_LEN],
) -> usize {
    let count = damage.len().min(MAX_DAMAGE_RECTS);
    out[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_PRESENT));
    out[4..8].copy_from_slice(&surface_id.to_le_bytes());
    out[8..12].copy_from_slice(&seq.to_le_bytes());
    out[12] = count as u8;
    let mut pos = 13;
    for rect in &damage[..count] {
        out[pos..pos + 2].copy_from_slice(&rect.x.to_le_bytes());
        out[pos + 2..pos + 4].copy_from_slice(&rect.y.to_le_bytes());
        out[pos + 4..pos + 6].copy_from_slice(&rect.width.to_le_bytes());
        out[pos + 6..pos + 8].copy_from_slice(&rect.height.to_le_bytes());
        pos += 8;
    }
    pos
}

/// `(surface_id, seq, damage)` — count and length strictly validated.
#[must_use]
pub fn decode_surface_present(
    frame: &[u8],
) -> Option<(u32, u32, [DamageRect; MAX_DAMAGE_RECTS], usize)> {
    if !has_op(frame, OP_SURFACE_PRESENT) || frame.len() < HEADER_LEN + 9 {
        return None;
    }
    let surface_id = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let seq = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]);
    let count = frame[12] as usize;
    if count > MAX_DAMAGE_RECTS || frame.len() != HEADER_LEN + 9 + count * 8 {
        return None;
    }
    let mut rects = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; MAX_DAMAGE_RECTS];
    for (i, rect) in rects.iter_mut().enumerate().take(count) {
        let pos = 13 + i * 8;
        *rect = DamageRect {
            x: u16::from_le_bytes([frame[pos], frame[pos + 1]]),
            y: u16::from_le_bytes([frame[pos + 2], frame[pos + 3]]),
            width: u16::from_le_bytes([frame[pos + 4], frame[pos + 5]]),
            height: u16::from_le_bytes([frame[pos + 6], frame[pos + 7]]),
        };
    }
    Some((surface_id, seq, rects, count))
}

// ----------------------------------------------------------------- destroy

pub const SURFACE_DESTROY_FRAME_LEN: usize = HEADER_LEN + 4;

#[must_use]
pub fn encode_surface_destroy(surface_id: u32) -> [u8; SURFACE_DESTROY_FRAME_LEN] {
    let mut f = [0u8; SURFACE_DESTROY_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_DESTROY));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f
}

#[must_use]
pub fn decode_surface_destroy(frame: &[u8]) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_DESTROY) || frame.len() != SURFACE_DESTROY_FRAME_LEN {
        return None;
    }
    Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]))
}

// ------------------------------------------------------------------- input

pub const SURFACE_INPUT_FRAME_LEN: usize = HEADER_LEN + 9;

#[must_use]
pub fn encode_surface_input(
    surface_id: u32,
    kind: u8,
    x: u16,
    y: u16,
) -> [u8; SURFACE_INPUT_FRAME_LEN] {
    let mut f = [0u8; SURFACE_INPUT_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_INPUT));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f[8] = kind;
    f[9..11].copy_from_slice(&x.to_le_bytes());
    f[11..13].copy_from_slice(&y.to_le_bytes());
    f
}

/// `(surface_id, kind, x, y)`.
#[must_use]
pub fn decode_surface_input(frame: &[u8]) -> Option<(u32, u8, u16, u16)> {
    if !has_op(frame, OP_SURFACE_INPUT) || frame.len() != SURFACE_INPUT_FRAME_LEN {
        return None;
    }
    Some((
        u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
        frame[8],
        u16::from_le_bytes([frame[9], frame[10]]),
        u16::from_le_bytes([frame[11], frame[12]]),
    ))
}

// ------------------------------------------------------------------ events

// +8 nonce bytes: the app-host mints a nonce, attaches its event channel with
// it, and repeats it on SURFACE_CREATE — windowd binds channel↔surface by
// nonce (deterministic under N concurrently connecting app-hosts; arrival
// ORDER carries no identity).
pub const SURFACE_EVENTS_FRAME_LEN: usize = HEADER_LEN + 8;

/// Header-only attach frame (the moved SEND capability rides the message).
#[must_use]
pub fn encode_surface_events(nonce: u64) -> [u8; SURFACE_EVENTS_FRAME_LEN] {
    let mut f = [0u8; SURFACE_EVENTS_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_EVENTS));
    f[4..12].copy_from_slice(&nonce.to_le_bytes());
    f
}

/// The attach nonce, when the frame is a well-formed events attach.
#[must_use]
pub fn decode_surface_events(frame: &[u8]) -> Option<u64> {
    if !has_op(frame, OP_SURFACE_EVENTS) || frame.len() != SURFACE_EVENTS_FRAME_LEN {
        return None;
    }
    Some(u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]))
}

#[must_use]
pub fn is_surface_events(frame: &[u8]) -> bool {
    has_op(frame, OP_SURFACE_EVENTS) && frame.len() == SURFACE_EVENTS_FRAME_LEN
}

// -------------------------------------------------------------------- acks

/// Ack layout (all ops): `[hdr(op | 0x80), status, value:u32]` — `value` is
/// the surface id (create) or the acked seq (present/destroy: surface id).
pub const SURFACE_ACK_FRAME_LEN: usize = HEADER_LEN + 5;

#[must_use]
pub fn encode_surface_ack(op: u8, status: u8, value: u32) -> [u8; SURFACE_ACK_FRAME_LEN] {
    let mut f = [0u8; SURFACE_ACK_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(op | 0x80));
    f[4] = status;
    f[5..9].copy_from_slice(&value.to_le_bytes());
    f
}

/// `(status, value)` for the given op's ack.
#[must_use]
pub fn decode_surface_ack(frame: &[u8], op: u8) -> Option<(u8, u32)> {
    if !has_op(frame, op | 0x80) || frame.len() != SURFACE_ACK_FRAME_LEN {
        return None;
    }
    Some((frame[4], u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_round_trip_and_guards() {
        // The declared intent rides ATOMICALLY on the create frame (a separate
        // pre-create intent op raced across concurrently connecting app-hosts).
        let c = SurfaceCreate {
            width: 320,
            height: 240,
            format: FORMAT_BGRA8888,
            style: WIN_STYLE_PLAIN,
            level: WIN_LEVEL_DESKTOP,
            mode: WIN_MODE_FULLSCREEN,
            resizable: false,
            nonce: 0xA1B2_C3D4_E5F6_0718,
            rows: 640,
            store_w: 200,
        };
        let f = encode_surface_create(&c);
        assert_eq!(decode_surface_create(&f), Some(c));
        // A truncated frame is malformed — there is no shorter legacy form.
        assert_eq!(decode_surface_create(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_PRESENT;
        assert_eq!(decode_surface_create(&wrong), None);
    }

    fn create(rows: u16, store_w: u16) -> [u8; SURFACE_CREATE_FRAME_LEN] {
        encode_surface_create(&SurfaceCreate {
            width: 320,
            height: 240,
            format: FORMAT_BGRA8888,
            style: 0,
            level: 0,
            mode: 0,
            resizable: true,
            nonce: 7,
            rows,
            store_w,
        })
    }

    #[test]
    fn test_reject_a_create_with_rows_below_its_height() {
        assert_eq!(decode_surface_create(&create(239, 0)), None, "rows below the height");
        assert_eq!(decode_surface_create(&create(240, 0)).map(|c| c.rows), Some(240), "no store");
        let cap = crate::surface_layers::MAX_SURFACE_ROWS;
        assert_eq!(decode_surface_create(&create(cap, 16)).map(|c| c.rows), Some(cap));
        assert_eq!(decode_surface_create(&create(cap + 1, 16)), None, "past the surface cap");
    }

    /// RFC-0109: the store lies inside the surface's columns, and exists exactly when the
    /// surface has rows for it.
    #[test]
    fn test_reject_a_store_wider_than_the_surface_or_without_rows() {
        assert_eq!(decode_surface_create(&create(400, 321)), None, "wider than the surface");
        assert_eq!(decode_surface_create(&create(400, 0)), None, "rows without a store");
        assert_eq!(decode_surface_create(&create(240, 16)), None, "a store without rows");
        assert_eq!(decode_surface_create(&create(400, 320)).map(|c| c.store_w), Some(320));
    }

    #[test]
    fn intent_round_trip_and_guards() {
        let f = encode_surface_intent(
            WIN_STYLE_PLAIN,
            WIN_LEVEL_DESKTOP,
            WIN_MODE_FULLSCREEN,
            false,
            0xDEAD_BEEF_1234_5678,
        );
        assert_eq!(
            decode_surface_intent(&f),
            Some((
                WIN_STYLE_PLAIN,
                WIN_LEVEL_DESKTOP,
                WIN_MODE_FULLSCREEN,
                false,
                0xDEAD_BEEF_1234_5678
            ))
        );
        // Defaults (ordinary window) round-trip; resizable bool preserved.
        let d = encode_surface_intent(WIN_STYLE_TITLEBAR, WIN_LEVEL_NORMAL, WIN_MODE_AUTO, true, 7);
        assert_eq!(decode_surface_intent(&d), Some((0, 0, 0, true, 7)));
        assert_eq!(decode_surface_intent(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_CREATE;
        assert_eq!(decode_surface_intent(&wrong), None);
    }

    #[test]
    fn control_round_trip_and_guards() {
        let f = encode_surface_control(CONTROL_WIN_MODE, 0x24);
        assert_eq!(decode_surface_control(&f), Some((CONTROL_WIN_MODE, 0x24)));
        assert_eq!(decode_surface_control(&f[..f.len() - 1]), None);
        // Retired control VALUES stay reserved (never reused) — pin them.
        assert_eq!(
            (CONTROL_THEME, CONTROL_SHELL_PROFILE, CONTROL_THEME_ACCENT),
            (0, 1, 3),
            "retired control ids are reserved forever"
        );
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

    #[test]
    fn present_round_trip_clamps_and_validates() {
        let damage = [
            DamageRect { x: 0, y: 0, width: 320, height: 240 },
            DamageRect { x: 10, y: 20, width: 30, height: 40 },
        ];
        let mut buf = [0u8; SURFACE_PRESENT_MAX_LEN];
        let len = encode_surface_present(7, 3, &damage, &mut buf);
        let (id, seq, rects, count) = decode_surface_present(&buf[..len]).expect("decodes");
        assert_eq!((id, seq, count), (7, 3, 2));
        assert_eq!(rects[1], damage[1]);
        // Truncated + over-count frames are rejected.
        assert_eq!(decode_surface_present(&buf[..len - 1]), None);
        let mut bad = buf;
        bad[12] = (MAX_DAMAGE_RECTS + 1) as u8;
        assert_eq!(decode_surface_present(&bad[..len]), None);
    }

    #[test]
    fn destroy_and_ack_round_trip() {
        let f = encode_surface_destroy(9);
        assert_eq!(decode_surface_destroy(&f), Some(9));
        let ack = encode_surface_ack(OP_SURFACE_PRESENT, SURFACE_STATUS_OK, 3);
        assert_eq!(decode_surface_ack(&ack, OP_SURFACE_PRESENT), Some((SURFACE_STATUS_OK, 3)));
        assert_eq!(decode_surface_ack(&ack, OP_SURFACE_CREATE), None);
    }

    #[test]
    fn ops_do_not_collide_with_the_input_family() {
        // input-live-protocol occupies 1–4 on the same endpoint envelope.
        for op in [
            OP_SURFACE_CREATE,
            OP_SURFACE_PRESENT,
            OP_SURFACE_DESTROY,
            OP_SURFACE_INPUT,
            OP_SURFACE_EVENTS,
        ] {
            assert!(op > 4, "op {op} collides with the input family");
        }
    }

    #[test]
    fn events_attach_frame_round_trip() {
        let f = encode_surface_events(7);
        assert!(is_surface_events(&f));
        assert!(!is_surface_events(&f[..f.len() - 1]));
        let mut wrong = f;
        wrong[3] = OP_SURFACE_INPUT;
        assert!(!is_surface_events(&wrong));
    }

    #[test]
    fn input_round_trip() {
        let f = encode_surface_input(3, INPUT_KIND_TAP, 120, 88);
        assert_eq!(decode_surface_input(&f), Some((3, INPUT_KIND_TAP, 120, 88)));
        assert_eq!(decode_surface_input(&f[..f.len() - 1]), None);
    }
}
