// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The key channel inputd → windowd (RFC-0113 §2/§3, ADR-0090). The input source
//! decides a key's route: a LAYOUT source (de, us — dead keys included) is translated in inputd,
//! and its text or editing action rides this frame to windowd, which delivers it to the field it
//! has focused; a COMPOSITION source's key rides it too, marked for the input method, which
//! windowd feeds behind its focus. Every entry carries its source sequence and the time the
//! system first saw the key (`seq`, `t_ns`): the field that applies it measures, every hop
//! counts. inputd sends this frame BLOCKING — a full windowd queue holds inputd, it never drops a
//! key (RFC-0113 §2). Fixed buffers on both ends: nothing is allocated per key. The system's own
//! keys ride it too (TASK-0348 P17): a window chord of the shortcut registry and a capture key
//! are EVENTS — each entry applied once, in order — never a one-shot fact on the sampled visible
//! state, where a sample that changed nothing else could coalesce it away (the rule every input
//! stack keeps: samples coalesce, edges never).
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (round trip, `test_reject_*` for the bounds and the kinds)

use crate::{frame_has_op, HEADER_LEN, MAGIC0, MAGIC1, VERSION};

/// inputd → windowd: up to [`MAX_KEY_INPUTS`] key inputs in order. (The `'I','N'` envelope's
/// input family holds 1–7 on windowd's endpoint; the surface ops start at 8.)
pub const OP_PUSH_KEY_INPUT: u8 = 6;

/// Text a layout source produced: `value` is one Unicode scalar.
pub const KEY_INPUT_TEXT: u8 = 1;
/// An editing action: `value` is the input method wire's `ACTION_*` code (the shared action
/// space of RFC-0075 — Escape, Backspace, Enter, Tab and the TASK-0067B editing commands).
pub const KEY_INPUT_ACTION: u8 = 2;
/// A composition source's key for the input method: `sub` is its wire `KEY_KIND_*`, `value` the
/// character (text, dead) or the action code.
pub const KEY_INPUT_COMPOSE: u8 = 3;
/// A window chord of the shortcut registry (TASK-0066 / RFC-0113 §7): `value` is windowd's
/// `zones::CODE_*` (1..=255) — windowd applies it to the focused window; no app sees its keys.
pub const KEY_INPUT_CHORD: u8 = 4;
/// A capture key (RFC-0095): `value` is `CAPTURE_KIND_UI` (Print: the screenshot UI), `_SCREEN`
/// (Shift+Print) or `_WINDOW` (Alt+Print) — windowd hands it to the shell; no app sees the key.
pub const KEY_INPUT_CAPTURE: u8 = 5;
/// The capture kinds a `KEY_INPUT_CAPTURE` entry carries.
pub const CAPTURE_KIND_UI: u32 = 1;
pub const CAPTURE_KIND_SCREEN: u32 = 2;
pub const CAPTURE_KIND_WINDOW: u32 = 3;

/// The entry is an auto-repeat of a held key (inputd's repeat timer, not the device).
pub const KEY_FLAG_REPEAT: u8 = 1;
const KNOWN_FLAGS: u8 = KEY_FLAG_REPEAT;

const ENTRY_LEN: usize = 20;
/// Entries per frame: a burst beyond it goes in the next frame, in order.
pub const MAX_KEY_INPUTS: usize = 12;
/// The largest key-input frame.
pub const KEY_INPUT_FRAME_MAX: usize = HEADER_LEN + MAX_KEY_INPUTS * ENTRY_LEN;

/// One key input on the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyInput {
    /// `KEY_INPUT_*`.
    pub kind: u8,
    /// For [`KEY_INPUT_COMPOSE`]: the input method wire's `KEY_KIND_*`; 0 otherwise.
    pub sub: u8,
    /// `KEY_FLAG_*`.
    pub flags: u8,
    /// The modifiers held (the input method wire's modifier bits).
    pub modifiers: u8,
    /// The key event's sequence in its device stream (RFC-0113 §1).
    pub seq: u32,
    /// When the system first saw the key (monotonic ns).
    pub t_ns: u64,
    /// The character or the action code.
    pub value: u32,
}

impl KeyInput {
    /// Whether the entry is well-formed: a known kind, known flags, a text value that is a
    /// Unicode scalar.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let value_ok = match self.kind {
            KEY_INPUT_TEXT => char::from_u32(self.value).is_some(),
            KEY_INPUT_ACTION | KEY_INPUT_COMPOSE => true,
            KEY_INPUT_CHORD => (1..=u32::from(u8::MAX)).contains(&self.value),
            KEY_INPUT_CAPTURE => (CAPTURE_KIND_UI..=CAPTURE_KIND_WINDOW).contains(&self.value),
            _ => false,
        };
        value_ok && self.flags & !KNOWN_FLAGS == 0
    }
}

/// Encodes `inputs` (1..=[`MAX_KEY_INPUTS`], each valid) into `buf`; the frame's length, or
/// `None` when the count is out of range or an entry is malformed.
pub fn encode_push_key_input(
    inputs: &[KeyInput],
    buf: &mut [u8; KEY_INPUT_FRAME_MAX],
) -> Option<usize> {
    if inputs.is_empty() || inputs.len() > MAX_KEY_INPUTS || !inputs.iter().all(KeyInput::is_valid)
    {
        return None;
    }
    buf[..4].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_PUSH_KEY_INPUT]);
    buf[4..8].copy_from_slice(&(inputs.len() as u32).to_le_bytes());
    let mut pos = HEADER_LEN;
    for input in inputs {
        buf[pos] = input.kind;
        buf[pos + 1] = input.sub;
        buf[pos + 2] = input.flags;
        buf[pos + 3] = input.modifiers;
        buf[pos + 4..pos + 8].copy_from_slice(&input.seq.to_le_bytes());
        buf[pos + 8..pos + 16].copy_from_slice(&input.t_ns.to_le_bytes());
        buf[pos + 16..pos + 20].copy_from_slice(&input.value.to_le_bytes());
        pos += ENTRY_LEN;
    }
    Some(pos)
}

/// Decodes a key-input frame into `out`; the number of entries, or `None` for anything that is
/// not exactly a well-formed frame (fail-closed: one malformed entry refuses the frame).
pub fn decode_push_key_input(frame: &[u8], out: &mut [KeyInput; MAX_KEY_INPUTS]) -> Option<usize> {
    if !frame_has_op(frame, OP_PUSH_KEY_INPUT) {
        return None;
    }
    let count = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    if count == 0 || count > MAX_KEY_INPUTS || frame.len() != HEADER_LEN + count * ENTRY_LEN {
        return None;
    }
    for (i, slot) in out.iter_mut().enumerate().take(count) {
        let e = &frame[HEADER_LEN + i * ENTRY_LEN..HEADER_LEN + (i + 1) * ENTRY_LEN];
        let input = KeyInput {
            kind: e[0],
            sub: e[1],
            flags: e[2],
            modifiers: e[3],
            seq: u32::from_le_bytes([e[4], e[5], e[6], e[7]]),
            t_ns: u64::from_le_bytes([e[8], e[9], e[10], e[11], e[12], e[13], e[14], e[15]]),
            value: u32::from_le_bytes([e[16], e[17], e[18], e[19]]),
        };
        if !input.is_valid() {
            return None;
        }
        *slot = input;
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(seq: u32, ch: char) -> KeyInput {
        KeyInput {
            kind: KEY_INPUT_TEXT,
            seq,
            t_ns: 1_000 + u64::from(seq),
            value: ch as u32,
            ..KeyInput::default()
        }
    }

    #[test]
    fn key_inputs_round_trip_in_order() {
        let inputs = [
            text(7, 'k'),
            text(8, 'ü'),
            KeyInput { kind: KEY_INPUT_ACTION, seq: 9, t_ns: 5, value: 4, ..KeyInput::default() },
            KeyInput {
                kind: KEY_INPUT_COMPOSE,
                sub: 2,
                flags: KEY_FLAG_REPEAT,
                modifiers: 1,
                seq: 10,
                t_ns: 6,
                value: 'a' as u32,
            },
        ];
        let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
        let n = encode_push_key_input(&inputs, &mut buf).expect("encodes");
        let mut out = [KeyInput::default(); MAX_KEY_INPUTS];
        assert_eq!(decode_push_key_input(&buf[..n], &mut out), Some(4));
        assert_eq!(&out[..4], &inputs);
    }

    /// TASK-0348 P17: a window chord and a capture key are entries like any key — in order with
    /// the keys around them, each applied once.
    #[test]
    fn system_keys_ride_the_channel_in_order() {
        let inputs = [
            text(3, 'a'),
            KeyInput {
                kind: KEY_INPUT_CHORD,
                modifiers: 18,
                seq: 4,
                t_ns: 9,
                value: 14,
                ..KeyInput::default()
            },
            KeyInput {
                kind: KEY_INPUT_CAPTURE,
                seq: 5,
                t_ns: 10,
                value: CAPTURE_KIND_SCREEN,
                ..KeyInput::default()
            },
        ];
        let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
        let n = encode_push_key_input(&inputs, &mut buf).expect("encodes");
        let mut out = [KeyInput::default(); MAX_KEY_INPUTS];
        assert_eq!(decode_push_key_input(&buf[..n], &mut out), Some(3));
        assert_eq!(&out[..3], &inputs);
    }

    #[test]
    fn test_reject_a_chord_without_a_code_or_a_capture_kind_past_the_window() {
        let no_code = KeyInput { kind: KEY_INPUT_CHORD, value: 0, ..KeyInput::default() };
        let wide = KeyInput { kind: KEY_INPUT_CHORD, value: 256, ..KeyInput::default() };
        let no_kind = KeyInput { kind: KEY_INPUT_CAPTURE, value: 0, ..KeyInput::default() };
        let past = KeyInput {
            kind: KEY_INPUT_CAPTURE,
            value: CAPTURE_KIND_WINDOW + 1,
            ..KeyInput::default()
        };
        for bad in [no_code, wide, no_kind, past] {
            assert!(!bad.is_valid(), "{bad:?}");
            let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
            assert_eq!(encode_push_key_input(&[bad], &mut buf), None);
        }
    }

    #[test]
    fn test_reject_an_empty_or_oversized_burst() {
        let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
        assert_eq!(encode_push_key_input(&[], &mut buf), None);
        let burst = [text(1, 'a'); MAX_KEY_INPUTS + 1];
        assert_eq!(
            encode_push_key_input(&burst, &mut buf),
            None,
            "a burst is chunked by the sender"
        );
    }

    #[test]
    fn test_reject_a_frame_of_the_wrong_length_or_count() {
        let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
        let n = encode_push_key_input(&[text(1, 'a'), text(2, 'b')], &mut buf).expect("encodes");
        let mut out = [KeyInput::default(); MAX_KEY_INPUTS];
        assert_eq!(decode_push_key_input(&buf[..n - 1], &mut out), None, "truncated");
        let mut lying = buf;
        lying[4..8].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(decode_push_key_input(&lying[..n], &mut out), None, "count disagrees");
        let mut v2 = buf;
        v2[2] = 2;
        assert_eq!(decode_push_key_input(&v2[..n], &mut out), None, "a v2 frame is not this");
    }

    #[test]
    fn test_reject_an_unknown_kind_flag_or_a_non_scalar() {
        let mut buf = [0u8; KEY_INPUT_FRAME_MAX];
        let bad_kind = KeyInput { kind: 9, ..text(1, 'a') };
        assert_eq!(encode_push_key_input(&[bad_kind], &mut buf), None);
        let bad_flag = KeyInput { flags: 0x80, ..text(1, 'a') };
        assert_eq!(encode_push_key_input(&[bad_flag], &mut buf), None);
        // A surrogate is no character: refused on the wire too, not only by the encoder.
        let n = encode_push_key_input(&[text(1, 'a')], &mut buf).expect("encodes");
        buf[HEADER_LEN + 16..HEADER_LEN + 20].copy_from_slice(&0xD800u32.to_le_bytes());
        let mut out = [KeyInput::default(); MAX_KEY_INPUTS];
        assert_eq!(decode_push_key_input(&buf[..n], &mut out), None);
    }
}
