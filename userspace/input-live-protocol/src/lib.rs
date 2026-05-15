// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Narrow live-input wire protocol shared by `hidrawd`, `inputd`, and
//! `selftest-client` for RFC-0054.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests in this crate.
//! ADR: docs/adr/0017-service-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

pub const MAGIC0: u8 = b'I';
pub const MAGIC1: u8 = b'N';
pub const VERSION: u8 = 1;

pub const OP_PUSH_HID_BATCH: u8 = 1;
pub const OP_GET_VISIBLE_STATE: u8 = 2;
pub const OP_SEND_COMPOSED_FRAME_VMO: u8 = 3;
pub const OP_UPDATE_VISIBLE_STATE: u8 = 4;

pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_UNSUPPORTED: u8 = 2;
pub const STATUS_OVERFLOW: u8 = 3;

pub const HID_KIND_KEYBOARD: u8 = 1;
pub const HID_KIND_MOUSE: u8 = 2;

pub const POINTER_SOURCE_NONE: u8 = 0;
pub const POINTER_SOURCE_MOUSE_RELATIVE: u8 = 1;
pub const POINTER_SOURCE_TABLET_ABSOLUTE: u8 = 2;
pub const POINTER_SOURCE_TOUCH_ABSOLUTE: u8 = 3;

pub const EVENT_KIND_KEY: u8 = 1;
pub const EVENT_KIND_REL: u8 = 2;
pub const EVENT_KIND_ABS: u8 = 3;
pub const EVENT_KIND_BTN: u8 = 4;

const HEADER_LEN: usize = 8;
const EVENT_LEN: usize = 15;
pub const MAX_HID_BATCH_FRAME_LEN: usize = 256;
const STATE_LEN: usize = 32;
pub const VISIBLE_STATE_FRAME_LEN: usize = HEADER_LEN + STATE_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireHidEvent {
    pub kind: u8,
    pub code: u16,
    pub value: i32,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireHidBatch {
    pub device_kind: u8,
    pub device_id: u16,
    pub pointer_source: u8,
    pub abs_max_x: i32,
    pub abs_max_y: i32,
    pub raw_event_count: u16,
    pub normalized_event_count: u16,
    pub events: Vec<WireHidEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VisibleState {
    pub virtio_raw_seen: bool,
    pub hid_normalized_seen: bool,
    pub backend_visible: bool,
    pub display_scanout_ready: bool,
    pub systemui_first_frame_visible: bool,
    pub scene_ready: bool,
    pub full_window_visible: bool,
    pub click_target_visible: bool,
    pub keyboard_target_visible: bool,
    pub input_visible_on: bool,
    pub cursor_move_visible: bool,
    pub hover_visible: bool,
    pub focus_visible: bool,
    pub launcher_click_visible: bool,
    pub keyboard_visible: bool,
    pub wheel_up_visible: bool,
    pub wheel_down_visible: bool,
    pub pointer_route_live: bool,
    pub keyboard_route_live: bool,
    pub cursor_svg_visible: bool,
    pub text_target_visible: bool,
    pub icon_target_visible: bool,
    pub wallpaper_visible: bool,
    pub cursor_overlay_visible: bool,
    pub cursor_x: i32,
    pub cursor_y: i32,
}

pub fn encode_push_hid_batch(batch: &WireHidBatch) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + 16 + batch.events.len() * EVENT_LEN);
    out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_PUSH_HID_BATCH]);
    out.extend_from_slice(&(batch.events.len() as u32).to_le_bytes());
    out.push(batch.device_kind);
    out.extend_from_slice(&batch.device_id.to_le_bytes());
    out.push(batch.pointer_source);
    out.extend_from_slice(&batch.abs_max_x.to_le_bytes());
    out.extend_from_slice(&batch.abs_max_y.to_le_bytes());
    out.extend_from_slice(&batch.raw_event_count.to_le_bytes());
    out.extend_from_slice(&batch.normalized_event_count.to_le_bytes());
    for event in &batch.events {
        out.push(event.kind);
        out.extend_from_slice(&event.code.to_le_bytes());
        out.extend_from_slice(&event.value.to_le_bytes());
        out.extend_from_slice(&event.timestamp_ns.to_le_bytes());
    }
    out
}

/// Encode a HID batch into a pre-allocated stack buffer. Returns bytes written.
pub fn encode_push_hid_batch_into(
    batch: &WireHidBatch,
    buf: &mut [u8; MAX_HID_BATCH_FRAME_LEN],
) -> Option<usize> {
    let needed = HEADER_LEN + 16 + batch.events.len() * EVENT_LEN;
    if needed > MAX_HID_BATCH_FRAME_LEN {
        return None;
    }
    let mut pos = 0;
    buf[pos..pos + 4].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_PUSH_HID_BATCH]);
    pos += 4;
    buf[pos..pos + 4].copy_from_slice(&(batch.events.len() as u32).to_le_bytes());
    pos += 4;
    buf[pos] = batch.device_kind;
    pos += 1;
    buf[pos..pos + 2].copy_from_slice(&batch.device_id.to_le_bytes());
    pos += 2;
    buf[pos] = batch.pointer_source;
    pos += 1;
    buf[pos..pos + 4].copy_from_slice(&batch.abs_max_x.to_le_bytes());
    pos += 4;
    buf[pos..pos + 4].copy_from_slice(&batch.abs_max_y.to_le_bytes());
    pos += 4;
    buf[pos..pos + 2].copy_from_slice(&batch.raw_event_count.to_le_bytes());
    pos += 2;
    buf[pos..pos + 2].copy_from_slice(&batch.normalized_event_count.to_le_bytes());
    pos += 2;
    for event in &batch.events {
        buf[pos] = event.kind;
        pos += 1;
        buf[pos..pos + 2].copy_from_slice(&event.code.to_le_bytes());
        pos += 2;
        buf[pos..pos + 4].copy_from_slice(&event.value.to_le_bytes());
        pos += 4;
        buf[pos..pos + 8].copy_from_slice(&event.timestamp_ns.to_le_bytes());
        pos += 8;
    }
    Some(pos)
}

pub fn decode_push_hid_batch(frame: &[u8]) -> Option<WireHidBatch> {
    if frame.len() < HEADER_LEN + 4 || !frame_has_op(frame, OP_PUSH_HID_BATCH) {
        return None;
    }
    let event_count = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    if frame.len() != HEADER_LEN + 16 + event_count * EVENT_LEN {
        return None;
    }
    let device_kind = frame[8];
    let device_id = u16::from_le_bytes([frame[9], frame[10]]);
    let pointer_source = frame[11];
    let abs_max_x = i32::from_le_bytes([frame[12], frame[13], frame[14], frame[15]]);
    let abs_max_y = i32::from_le_bytes([frame[16], frame[17], frame[18], frame[19]]);
    let raw_event_count = u16::from_le_bytes([frame[20], frame[21]]);
    let normalized_event_count = u16::from_le_bytes([frame[22], frame[23]]);
    let mut events = Vec::with_capacity(event_count);
    let mut offset = 24;
    for _ in 0..event_count {
        let kind = frame[offset];
        let code = u16::from_le_bytes([frame[offset + 1], frame[offset + 2]]);
        let value = i32::from_le_bytes([
            frame[offset + 3],
            frame[offset + 4],
            frame[offset + 5],
            frame[offset + 6],
        ]);
        let timestamp_ns = u64::from_le_bytes([
            frame[offset + 7],
            frame[offset + 8],
            frame[offset + 9],
            frame[offset + 10],
            frame[offset + 11],
            frame[offset + 12],
            frame[offset + 13],
            frame[offset + 14],
        ]);
        events.push(WireHidEvent { kind, code, value, timestamp_ns });
        offset += EVENT_LEN;
    }
    Some(WireHidBatch {
        device_kind,
        device_id,
        pointer_source,
        abs_max_x,
        abs_max_y,
        raw_event_count,
        normalized_event_count,
        events,
    })
}

#[must_use]
pub fn encode_get_visible_state() -> [u8; HEADER_LEN] {
    [MAGIC0, MAGIC1, VERSION, OP_GET_VISIBLE_STATE, 0, 0, 0, 0]
}

#[must_use]
pub fn encode_send_composed_frame_vmo() -> [u8; HEADER_LEN] {
    [MAGIC0, MAGIC1, VERSION, OP_SEND_COMPOSED_FRAME_VMO, 0, 0, 0, 0]
}

#[must_use]
pub fn encode_update_visible_state(state: VisibleState) -> [u8; VISIBLE_STATE_FRAME_LEN] {
    encode_state_frame(OP_UPDATE_VISIBLE_STATE, state)
}

pub fn encode_visible_state(state: VisibleState) -> Vec<u8> {
    encode_visible_state_frame(state).to_vec()
}

#[must_use]
pub fn encode_visible_state_frame(state: VisibleState) -> [u8; VISIBLE_STATE_FRAME_LEN] {
    encode_state_frame(OP_GET_VISIBLE_STATE | 0x80, state)
}

#[must_use]
fn encode_state_frame(op: u8, state: VisibleState) -> [u8; VISIBLE_STATE_FRAME_LEN] {
    let mut out = [0u8; VISIBLE_STATE_FRAME_LEN];
    out[0] = MAGIC0;
    out[1] = MAGIC1;
    out[2] = VERSION;
    out[3] = op;
    out[4..8].copy_from_slice(&(STATE_LEN as u32).to_le_bytes());
    out[8..25].copy_from_slice(&[
        u8::from(state.virtio_raw_seen),
        u8::from(state.hid_normalized_seen),
        u8::from(state.backend_visible),
        u8::from(state.display_scanout_ready),
        u8::from(state.systemui_first_frame_visible),
        u8::from(state.scene_ready),
        u8::from(state.full_window_visible),
        u8::from(state.click_target_visible),
        u8::from(state.keyboard_target_visible),
        u8::from(state.input_visible_on),
        u8::from(state.cursor_move_visible),
        u8::from(state.hover_visible),
        u8::from(state.focus_visible),
        u8::from(state.launcher_click_visible),
        u8::from(state.keyboard_visible),
        u8::from(state.pointer_route_live),
        u8::from(state.keyboard_route_live),
    ]);
    out[25..29].copy_from_slice(&state.cursor_x.to_le_bytes());
    out[29..33].copy_from_slice(&state.cursor_y.to_le_bytes());
    out[33] = u8::from(state.wheel_up_visible);
    out[34] = u8::from(state.wheel_down_visible);
    out[35] = u8::from(state.cursor_svg_visible);
    out[36] = u8::from(state.text_target_visible);
    out[37] = u8::from(state.icon_target_visible);
    out[38] = u8::from(state.wallpaper_visible);
    out[39] = u8::from(state.cursor_overlay_visible);
    out
}

#[must_use]
pub fn encode_status(op: u8, status: u8) -> [u8; HEADER_LEN] {
    [MAGIC0, MAGIC1, VERSION, op | 0x80, status, 0, 0, 0]
}

#[must_use]
pub fn decode_status(frame: &[u8], op: u8) -> Option<u8> {
    (frame.len() == HEADER_LEN && frame_has_op(frame, op | 0x80)).then_some(frame[4])
}

pub fn decode_visible_state(frame: &[u8]) -> Option<VisibleState> {
    if frame.len() != HEADER_LEN + STATE_LEN || !frame_has_op(frame, OP_GET_VISIBLE_STATE | 0x80) {
        return None;
    }
    decode_state_payload(frame)
}

pub fn decode_update_visible_state(frame: &[u8]) -> Option<VisibleState> {
    if frame.len() != HEADER_LEN + STATE_LEN || !frame_has_op(frame, OP_UPDATE_VISIBLE_STATE) {
        return None;
    }
    decode_state_payload(frame)
}

fn decode_state_payload(frame: &[u8]) -> Option<VisibleState> {
    Some(VisibleState {
        virtio_raw_seen: frame[8] != 0,
        hid_normalized_seen: frame[9] != 0,
        backend_visible: frame[10] != 0,
        display_scanout_ready: frame[11] != 0,
        systemui_first_frame_visible: frame[12] != 0,
        scene_ready: frame[13] != 0,
        full_window_visible: frame[14] != 0,
        click_target_visible: frame[15] != 0,
        keyboard_target_visible: frame[16] != 0,
        input_visible_on: frame[17] != 0,
        cursor_move_visible: frame[18] != 0,
        hover_visible: frame[19] != 0,
        focus_visible: frame[20] != 0,
        launcher_click_visible: frame[21] != 0,
        keyboard_visible: frame[22] != 0,
        pointer_route_live: frame[23] != 0,
        keyboard_route_live: frame[24] != 0,
        cursor_x: i32::from_le_bytes([frame[25], frame[26], frame[27], frame[28]]),
        cursor_y: i32::from_le_bytes([frame[29], frame[30], frame[31], frame[32]]),
        wheel_up_visible: frame[33] != 0,
        wheel_down_visible: frame[34] != 0,
        cursor_svg_visible: frame[35] != 0,
        text_target_visible: frame[36] != 0,
        icon_target_visible: frame[37] != 0,
        wallpaper_visible: frame[38] != 0,
        cursor_overlay_visible: frame[39] != 0,
    })
}

#[must_use]
pub fn frame_has_op(frame: &[u8], op: u8) -> bool {
    frame.len() >= HEADER_LEN
        && frame[0] == MAGIC0
        && frame[1] == MAGIC1
        && frame[2] == VERSION
        && frame[3] == op
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_hid_batch_round_trips() {
        let encoded = encode_push_hid_batch(&WireHidBatch {
            device_kind: HID_KIND_MOUSE,
            device_id: 7,
            pointer_source: POINTER_SOURCE_MOUSE_RELATIVE,
            abs_max_x: 0,
            abs_max_y: 0,
            raw_event_count: 3,
            normalized_event_count: 2,
            events: vec![WireHidEvent {
                kind: EVENT_KIND_REL,
                code: 1,
                value: -4,
                timestamp_ns: 33,
            }],
        });
        let decoded = decode_push_hid_batch(&encoded);
        assert_eq!(
            decoded,
            Some(WireHidBatch {
                device_kind: HID_KIND_MOUSE,
                device_id: 7,
                pointer_source: POINTER_SOURCE_MOUSE_RELATIVE,
                abs_max_x: 0,
                abs_max_y: 0,
                raw_event_count: 3,
                normalized_event_count: 2,
                events: vec![WireHidEvent {
                    kind: EVENT_KIND_REL,
                    code: 1,
                    value: -4,
                    timestamp_ns: 33,
                }],
            })
        );
    }

    #[test]
    fn visible_state_round_trips() {
        let state = VisibleState {
            virtio_raw_seen: true,
            hid_normalized_seen: true,
            backend_visible: true,
            display_scanout_ready: true,
            systemui_first_frame_visible: true,
            scene_ready: true,
            full_window_visible: true,
            click_target_visible: true,
            keyboard_target_visible: false,
            input_visible_on: true,
            cursor_move_visible: true,
            hover_visible: true,
            focus_visible: true,
            launcher_click_visible: true,
            keyboard_visible: false,
            pointer_route_live: true,
            keyboard_route_live: false,
            cursor_svg_visible: true,
            text_target_visible: true,
            icon_target_visible: true,
            wallpaper_visible: true,
            cursor_overlay_visible: true,
            cursor_x: 320,
            cursor_y: 200,
            wheel_up_visible: true,
            wheel_down_visible: false,
        };
        assert_eq!(decode_visible_state(&encode_visible_state(state)), Some(state));
        assert_eq!(decode_update_visible_state(&encode_update_visible_state(state)), Some(state));
    }

    #[test]
    fn update_visible_state_rejects_response_and_truncated_frames() {
        let state = VisibleState { cursor_svg_visible: true, ..VisibleState::default() };
        let update = encode_update_visible_state(state);
        assert_eq!(decode_visible_state(&update), None);
        assert_eq!(decode_update_visible_state(&update[..update.len() - 1]), None);
    }
}
