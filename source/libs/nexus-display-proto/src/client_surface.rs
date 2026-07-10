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
//! The op numbers here live in that shared space and MUST NOT collide with
//! the input ops (1–4) — pinned by a unit test on the windowd side.

/// Shared envelope (input-live-protocol family on windowd's server endpoint).
pub const ENVELOPE_MAGIC0: u8 = b'I';
pub const ENVELOPE_MAGIC1: u8 = b'N';
pub const ENVELOPE_VERSION: u8 = 1;
const HEADER_LEN: usize = 4;

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

/// app → windowd: the surface's **material-tagged layer regions** (the R1 layer
/// seam, RFC-0067 Revival). The app renders its whole scene into ONE surface and
/// submits the sub-rects that are glass panels (from the DSL nodes carrying
/// `.material()`); windowd composites each region via the `nexus-gfx`
/// `composite_layer_full` SSOT — real cached backdrop blur + SDF rounded +
/// shadow — over the retained wallpaper, so the shell's panels are true glass
/// layers, not a flat bitmap. Regions not covered by a glass layer blit opaque.
/// Payload: `count:u8, [LayerDesc; count]`. Empty/absent = the whole surface
/// composites with the default (window) treatment (the pre-R1 behavior).
pub const OP_SURFACE_LAYERS: u8 = 15;

/// Material kinds for a submitted layer region.
pub const MATERIAL_OPAQUE: u8 = 0;
pub const MATERIAL_GLASS: u8 = 1;
/// Glass levels — the design-system glass tokens (panel/card/subtle/window),
/// mapped to a backdrop blur radius by the compositor.
pub const GLASS_PANEL: u8 = 0;
pub const GLASS_CARD: u8 = 1;
pub const GLASS_SUBTLE: u8 = 2;
pub const GLASS_WINDOW: u8 = 3;

/// Max glass layers per surface (bounds the frame; a real shell uses a handful:
/// topbar, dock, launcher panel, a few cards).
pub const MAX_SURFACE_LAYERS: usize = 16;

/// One material-tagged region of a client surface (surface-local pixels).
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

const LAYER_DESC_BYTES: usize = 12;
pub const SURFACE_LAYERS_MAX_LEN: usize = HEADER_LEN + 1 + MAX_SURFACE_LAYERS * LAYER_DESC_BYTES;

/// Encodes the layer list; clamps to [`MAX_SURFACE_LAYERS`]. Returns the length.
#[must_use]
pub fn encode_surface_layers(layers: &[LayerDesc], out: &mut [u8; SURFACE_LAYERS_MAX_LEN]) -> usize {
    let count = layers.len().min(MAX_SURFACE_LAYERS);
    out[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_LAYERS));
    out[4] = count as u8;
    let mut pos = 5;
    for l in &layers[..count] {
        out[pos..pos + 2].copy_from_slice(&l.x.to_le_bytes());
        out[pos + 2..pos + 4].copy_from_slice(&l.y.to_le_bytes());
        out[pos + 4..pos + 6].copy_from_slice(&l.w.to_le_bytes());
        out[pos + 6..pos + 8].copy_from_slice(&l.h.to_le_bytes());
        out[pos + 8] = l.material;
        out[pos + 9] = l.glass_level;
        out[pos + 10] = l.radius;
        out[pos + 11] = l.shadow_alpha;
        pos += LAYER_DESC_BYTES;
    }
    pos
}

/// Decodes the layer list into `out`; returns the count (strictly validated).
#[must_use]
pub fn decode_surface_layers(
    frame: &[u8],
    out: &mut [LayerDesc; MAX_SURFACE_LAYERS],
) -> Option<usize> {
    if !has_op(frame, OP_SURFACE_LAYERS) || frame.len() < HEADER_LEN + 1 {
        return None;
    }
    let count = frame[4] as usize;
    if count > MAX_SURFACE_LAYERS || frame.len() != HEADER_LEN + 1 + count * LAYER_DESC_BYTES {
        return None;
    }
    let mut pos = 5;
    for slot in out.iter_mut().take(count) {
        *slot = LayerDesc {
            x: u16::from_le_bytes([frame[pos], frame[pos + 1]]),
            y: u16::from_le_bytes([frame[pos + 2], frame[pos + 3]]),
            w: u16::from_le_bytes([frame[pos + 4], frame[pos + 5]]),
            h: u16::from_le_bytes([frame[pos + 6], frame[pos + 7]]),
            material: frame[pos + 8],
            glass_level: frame[pos + 9],
            radius: frame[pos + 10],
            shadow_alpha: frame[pos + 11],
        };
        pos += LAYER_DESC_BYTES;
    }
    Some(count)
}

/// windowd → app: the active theme mode, so an app renders with the SAME
/// tokens as the compositor (dark desktop ⇒ dark app) and re-themes on a live
/// light/dark toggle. Sent when the app event channel attaches (before the app
/// mounts) and again on every theme change. Payload: `mode:u8` (`THEME_*`).
pub const OP_SURFACE_THEME: u8 = 16;
/// Theme modes (align with windowd `ThemeMode` + the DSL `LightTokens`/`DarkTokens`).
pub const THEME_LIGHT: u8 = 0;
pub const THEME_DARK: u8 = 1;

pub const SURFACE_THEME_FRAME_LEN: usize = HEADER_LEN + 1;

/// Encodes the theme mode.
#[must_use]
pub fn encode_surface_theme(mode: u8) -> [u8; SURFACE_THEME_FRAME_LEN] {
    let mut f = [0u8; SURFACE_THEME_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_THEME));
    f[4] = mode;
    f
}

/// `mode`.
#[must_use]
pub fn decode_surface_theme(frame: &[u8]) -> Option<u8> {
    if !has_op(frame, OP_SURFACE_THEME) || frame.len() != SURFACE_THEME_FRAME_LEN {
        return None;
    }
    Some(frame[4])
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

/// Input kinds (taps + hover motion; keys land with the focus model).
pub const INPUT_KIND_TAP: u8 = 0;
/// Frame-aligned pointer motion inside the surface (hover). windowd stages
/// raw input per frame, so MOVE volume is bounded by frame rate, not by the
/// device event rate.
pub const INPUT_KIND_MOVE: u8 = 1;
/// The pointer left the surface (or moved onto another surface/chrome):
/// the client clears any hover presentation. x/y carry the last position.
pub const INPUT_KIND_LEAVE: u8 = 2;

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

/// One damage rect in surface-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

fn header(op: u8) -> [u8; HEADER_LEN] {
    [ENVELOPE_MAGIC0, ENVELOPE_MAGIC1, ENVELOPE_VERSION, op]
}

fn has_op(frame: &[u8], op: u8) -> bool {
    frame.len() >= HEADER_LEN
        && frame[0] == ENVELOPE_MAGIC0
        && frame[1] == ENVELOPE_MAGIC1
        && frame[2] == ENVELOPE_VERSION
        && frame[3] == op
}

// ------------------------------------------------------------------ create

// +4 intent bytes (style, level, mode, resizable): the declared window intent
// rides ATOMICALLY with the create — a separate pre-create OP_SURFACE_INTENT
// raced when two app-hosts connected concurrently (shell + counter: the
// pending intent crossed and BOTH surfaces landed in the desktop role).
pub const SURFACE_CREATE_FRAME_LEN: usize = HEADER_LEN + 17;

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn encode_surface_create(
    width: u16,
    height: u16,
    format: u8,
    style: u8,
    level: u8,
    mode: u8,
    resizable: bool,
    nonce: u64,
) -> [u8; SURFACE_CREATE_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CREATE_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CREATE));
    f[4..6].copy_from_slice(&width.to_le_bytes());
    f[6..8].copy_from_slice(&height.to_le_bytes());
    f[8] = format;
    f[9] = style;
    f[10] = level;
    f[11] = mode;
    f[12] = resizable as u8;
    f[13..21].copy_from_slice(&nonce.to_le_bytes());
    f
}

/// `(width, height, format, style, level, mode, resizable, nonce)`.
#[must_use]
#[allow(clippy::type_complexity)]
pub fn decode_surface_create(frame: &[u8]) -> Option<(u16, u16, u8, u8, u8, u8, bool, u64)> {
    if !has_op(frame, OP_SURFACE_CREATE) || frame.len() != SURFACE_CREATE_FRAME_LEN {
        return None;
    }
    Some((
        u16::from_le_bytes([frame[4], frame[5]]),
        u16::from_le_bytes([frame[6], frame[7]]),
        frame[8],
        frame[9],
        frame[10],
        frame[11],
        frame[12] != 0,
        u64::from_le_bytes([
            frame[13], frame[14], frame[15], frame[16], frame[17], frame[18], frame[19], frame[20],
        ]),
    ))
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
        let f = encode_surface_create(
            320,
            240,
            FORMAT_BGRA8888,
            WIN_STYLE_PLAIN,
            WIN_LEVEL_DESKTOP,
            WIN_MODE_FULLSCREEN,
            false,
            0xA1B2_C3D4_E5F6_0718,
        );
        assert_eq!(
            decode_surface_create(&f),
            Some((320, 240, FORMAT_BGRA8888, WIN_STYLE_PLAIN, WIN_LEVEL_DESKTOP, WIN_MODE_FULLSCREEN, false, 0xA1B2_C3D4_E5F6_0718))
        );
        assert_eq!(decode_surface_create(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_PRESENT;
        assert_eq!(decode_surface_create(&wrong), None);
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
    fn theme_round_trip_and_guards() {
        let f = encode_surface_theme(THEME_DARK);
        assert_eq!(decode_surface_theme(&f), Some(THEME_DARK));
        assert_eq!(decode_surface_theme(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_RECT;
        assert_eq!(decode_surface_theme(&wrong), None);
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
    fn layers_round_trip_clamps_and_validates() {
        let layers = [
            LayerDesc { x: 0, y: 0, w: 1280, h: 48, material: MATERIAL_GLASS, glass_level: GLASS_PANEL, radius: 0, shadow_alpha: 40 },
            LayerDesc { x: 20, y: 60, w: 200, h: 120, material: MATERIAL_GLASS, glass_level: GLASS_CARD, radius: 12, shadow_alpha: 80 },
        ];
        let mut buf = [0u8; SURFACE_LAYERS_MAX_LEN];
        let len = encode_surface_layers(&layers, &mut buf);
        let mut out = [LayerDesc::default(); MAX_SURFACE_LAYERS];
        let n = decode_surface_layers(&buf[..len], &mut out).expect("decodes");
        assert_eq!(n, 2);
        assert_eq!(out[0], layers[0]);
        assert_eq!(out[1], layers[1]);
        // Empty list is valid (whole-surface default treatment).
        let empty = encode_surface_layers(&[], &mut buf);
        assert_eq!(decode_surface_layers(&buf[..empty], &mut out), Some(0));
        // Truncated + wrong-op frames rejected.
        assert_eq!(decode_surface_layers(&buf[..len - 1], &mut out), None);
        let mut wrong = buf;
        wrong[3] = OP_SURFACE_PRESENT;
        assert_eq!(decode_surface_layers(&wrong[..len], &mut out), None);
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
