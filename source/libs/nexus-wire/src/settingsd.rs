// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! settingsd v1 wire protocol (TASK-0072 Phase 8): a TYPED settings registry.
//! Keys are dotted namespaces (`ui.theme.mode`), values carry a type tag, and
//! every mutation flows through settingsd (validation + apply hook +
//! persistence via statefsd) — no client writes prefs directly.

/// First magic byte (`'S'`).
pub const MAGIC0: u8 = b'S';
/// Second magic byte (`'T'`).
pub const MAGIC1: u8 = b'T';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Read one key's typed value.
pub const OP_GET: u8 = 1;
/// Write one key's typed value (validated, applied, persisted).
pub const OP_SET: u8 = 2;
/// Subscribe to APPLIED changes under a key prefix (RFC-0078). The request
/// MUST cap-move the subscriber's push-channel SEND half; settingsd keeps it
/// and pushes `OP_EVENT` frames on every matching change. A second watch on
/// the same channel replaces the prefix. No ack frame (the moved cap is the
/// subscription; a malformed request without a moved cap is answered
/// MALFORMED on the shared endpoint).
pub const OP_WATCH: u8 = 3;
/// Change push (settingsd → subscriber): `flags` bit0 = resync (events were
/// dropped — re-read via OP_GET).
pub const OP_EVENT: u8 = 4;

/// `OP_EVENT` flags bit0: deliveries were dropped since the last event.
pub const EVENT_FLAG_RESYNC: u8 = 0x01;
/// Maximum watch-prefix bytes (RFC-0078 bound).
pub const WATCH_PREFIX_MAX: usize = 64;

/// Operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Request frame was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// The key is not in the registry.
pub const STATUS_UNKNOWN_KEY: u8 = 2;
/// The value's type/content failed the key's validation.
pub const STATUS_INVALID_VALUE: u8 = 3;
/// Persisting to statefsd failed (the in-memory value did NOT change —
/// set is atomic: validate → persist → apply).
pub const STATUS_PERSIST_FAIL: u8 = 4;

/// Value type tags. v1 carries every value as UTF-8 TEXT with a tag that
/// names the key's semantic type — enums (`dark`/`light`) and identifiers
/// stay human-readable on the wire and in the persisted journal.
pub const TYPE_TEXT: u8 = 0;

/// Seed keys (TASK-0225 vocabulary). The registry defines defaults +
/// validation server-side; clients only name keys.
pub const KEY_UI_THEME_MODE: &str = "ui.theme.mode";
/// Accent-palette pick (`"default"` or a `nexus-theme-tokens`
/// `ACCENT_PALETTE` name — violet/pink/red/orange/green); windowd
/// applies (packed into the theme push) + persists it.
pub const KEY_UI_THEME_ACCENT: &str = "ui.theme.accent";
/// Shell windowing mode (`tablet`/`desktop`) — the Control-Center
/// Desktop/Tablet toggle; windowd applies + persists it.
pub const KEY_UI_SHELL_MODE: &str = "ui.shell.mode";
/// UI font family (read-only default today; live switching is a follow-up).
pub const KEY_UI_FONT_FAMILY: &str = "ui.font.family";
/// Prepared (registered, no consumer yet): locale + MIME defaults.
pub const KEY_UI_LOCALE: &str = "ui.locale";

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// GET request: `[S, T, ver, OP_GET, key_len:u8, key...]`.
    request encode_get_req / decode_get_req (op = OP_GET) {
        key: str8(min = 1, max = 255),
    }
    /// SET request:
    /// `[S, T, ver, OP_SET, key_len:u8, key..., type:u8, val_len:u8, val...]`.
    request encode_set_req / decode_set_req (op = OP_SET) {
        key: str8(min = 1, max = 255),
        _type: lit(TYPE_TEXT),
        value: str8(min = 0, max = 255),
    }
    /// WATCH request: `[S, T, ver, OP_WATCH, prefix_len:u8, prefix...]`
    /// (cap-moves the subscriber's push SEND half alongside).
    request encode_watch_req / decode_watch_req (op = OP_WATCH) {
        prefix: str8(min = 1, max = WATCH_PREFIX_MAX),
    }
    /// Change push: `[S, T, ver, OP_EVENT, flags:u8, key_len:u8, key...,
    /// val_len:u8, val...]`.
    request encode_event / decode_event (op = OP_EVENT) {
        flags: u8,
        key: str8(min = 1, max = 255),
        value: str8(min = 0, max = 255),
    }
    /// Response: `[S, T, ver, op|0x80, status:u8, type:u8, val_len:u8, val...]`
    /// (`val` is the key's current value for OK GET/SET; empty otherwise).
    reply encode_response / decode_response (op = caller) {
        status: u8,
        _type: lit(TYPE_TEXT),
        value: str8(min = 0, max = 255),
    }
}

/// Decodes a request → `(op, key, value)`; `value` is empty for GET.
/// Returns `None` on any malformed frame (bad magic/version/lengths).
pub fn decode_request(frame: &[u8]) -> Option<(u8, &str, &str)> {
    match crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)? {
        OP_GET => decode_get_req(frame).map(|key| (OP_GET, key, "")),
        OP_SET => decode_set_req(frame).map(|(key, value)| (OP_SET, key, value)),
        OP_WATCH => decode_watch_req(frame).map(|prefix| (OP_WATCH, prefix, "")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden byte layouts: the wire format is a cross-service contract —
    // a layout drift breaks windowd/statefsd interop silently.
    #[test]
    fn get_request_golden_bytes() {
        let mut buf = [0u8; 32];
        let n = encode_get_req("ui.theme.mode", &mut buf).unwrap();
        assert_eq!(&buf[..5], &[b'S', b'T', 1, OP_GET, 13]);
        assert_eq!(&buf[5..n], b"ui.theme.mode");
        let (op, key, value) = decode_request(&buf[..n]).unwrap();
        assert_eq!((op, key, value), (OP_GET, "ui.theme.mode", ""));
    }

    #[test]
    fn set_request_golden_bytes_and_roundtrip() {
        let mut buf = [0u8; 64];
        let n = encode_set_req(KEY_UI_THEME_MODE, "light", &mut buf).unwrap();
        assert_eq!(&buf[..5], &[b'S', b'T', 1, OP_SET, 13]);
        assert_eq!(buf[5 + 13], TYPE_TEXT);
        assert_eq!(buf[6 + 13], 5);
        let (op, key, value) = decode_request(&buf[..n]).unwrap();
        assert_eq!((op, key, value), (OP_SET, "ui.theme.mode", "light"));
    }

    #[test]
    fn response_roundtrip_and_rejects() {
        let mut buf = [0u8; 32];
        let n = encode_response(OP_GET, STATUS_OK, "dark", &mut buf).unwrap();
        assert_eq!(decode_response(OP_GET, &buf[..n]), Some((STATUS_OK, "dark")));
        // Wrong op bit / truncation / bad magic all reject.
        assert_eq!(decode_response(OP_SET, &buf[..n]), None);
        assert_eq!(decode_response(OP_GET, &buf[..n - 1]), None);
        let mut bad = buf;
        bad[0] = b'X';
        assert_eq!(decode_response(OP_GET, &bad[..n]), None);
    }

    #[test]
    fn malformed_requests_reject() {
        // Truncated key, empty key, unknown op, trailing garbage on GET.
        assert_eq!(decode_request(&[b'S', b'T', 1, OP_GET, 5, b'a']), None);
        assert_eq!(decode_request(&[b'S', b'T', 1, OP_GET, 0]), None);
        assert_eq!(decode_request(&[b'S', b'T', 1, 99, 1, b'a']), None);
        let mut buf = [0u8; 32];
        let n = encode_get_req("a", &mut buf).unwrap();
        assert_eq!(decode_request(&buf[..n + 1]), None);
    }

    #[test]
    fn watch_and_event_golden_bytes_and_roundtrip() {
        let mut buf = [0u8; 96];
        let n = encode_watch_req("input.", &mut buf).unwrap();
        assert_eq!(&buf[..5], &[b'S', b'T', 1, OP_WATCH, 6]);
        assert_eq!(decode_request(&buf[..n]), Some((OP_WATCH, "input.", "")));
        // Prefix bounds: empty and oversized reject on encode.
        assert_eq!(encode_watch_req("", &mut buf), None);
        let long = core::str::from_utf8(&[b'a'; WATCH_PREFIX_MAX + 1]).unwrap();
        assert_eq!(encode_watch_req(long, &mut buf), None);

        let mut ev = [0u8; 600];
        let n = encode_event(EVENT_FLAG_RESYNC, "input.keymap", "de", &mut ev).unwrap();
        assert_eq!(&ev[..4], &[b'S', b'T', 1, OP_EVENT]);
        assert_eq!(decode_event(&ev[..n]), Some((EVENT_FLAG_RESYNC, "input.keymap", "de")));
    }

    #[test]
    fn test_reject_watch_event_matrix() {
        let mut buf = [0u8; 96];
        let n = encode_watch_req("time.", &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_watch_req(f).is_some()
        });
        let mut ev = [0u8; 600];
        let m = encode_event(0, "time.zone", "UTC", &mut ev).unwrap();
        crate::codec::testing::assert_reject_matrix(&ev[..m], 4, &|f| decode_event(f).is_some());
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let mut buf = [0u8; 64];
        let n = encode_set_req(KEY_UI_THEME_MODE, "light", &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| decode_set_req(f).is_some());
        let mut rsp = [0u8; 32];
        let m = encode_response(OP_GET, STATUS_OK, "dark", &mut rsp).unwrap();
        crate::codec::testing::assert_reject_matrix(&rsp[..m], 4, &|f| {
            decode_response(OP_GET, f).is_some()
        });
    }
}
