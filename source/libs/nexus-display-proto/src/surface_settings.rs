// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0083 presentation snapshot (`OP_SURFACE_SETTINGS`) — the ONE
//! versioned windowd→app settings frame replacing the separate THEME (16) /
//! PROFILE (17) / REGION (23) pushes. Same `'I','N'` envelope + op space as
//! `client_surface` (collision-pinned there).
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0307)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below (roundtrip + reject matrix)
//! RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md

use crate::client_surface::{has_op, header, HEADER_LEN};
use crate::surface_text::{REGION_KEYMAP_MAX, REGION_LOCALE_MAX, REGION_TZ_MAX};

/// windowd → app: the versioned PRESENTATION SNAPSHOT (RFC-0083) — theme,
/// accent, shell profile, hour format, locale, timezone and keymap in ONE
/// idempotent frame, replacing the separate THEME (16) / PROFILE (17) /
/// REGION (23) pushes. `gen` is windowd-local and monotonic (wrapping):
/// receivers skip `gen == last` and otherwise field-compare-and-apply, so a
/// lost push heals at the next one and a duplicate is free. Apps must accept
/// it only on windowd's established push channel.
pub const OP_SURFACE_SETTINGS: u8 = 26;

pub const SURFACE_SETTINGS_FRAME_MAX: usize =
    HEADER_LEN + 7 + REGION_LOCALE_MAX + REGION_TZ_MAX + REGION_KEYMAP_MAX + 3;

/// One decoded presentation snapshot (borrowed strings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceSettings<'a> {
    /// windowd-local wrapping generation (dedupe/short-circuit token only —
    /// ordering is structural: kernel IPC is FIFO per channel).
    pub gen: u32,
    /// The packed theme byte (`client_surface::pack_theme`: low nibble mode,
    /// high nibble accent-palette index).
    pub theme: u8,
    /// `client_surface::PROFILE_*`.
    pub profile: u8,
    /// `REGION_HOUR_24` / `REGION_HOUR_12`.
    pub hour_fmt: u8,
    pub locale: &'a str,
    pub tz: &'a str,
    pub keymap: &'a str,
}

/// Encodes a presentation snapshot; `None` when a field exceeds its bound.
/// Layout: `[hdr:4][gen:u32 LE][theme][profile][hour_fmt]
/// [locale_len][locale…][tz_len][tz…][keymap_len][keymap…]`.
#[must_use]
pub fn encode_surface_settings(
    s: &SurfaceSettings<'_>,
) -> Option<([u8; SURFACE_SETTINGS_FRAME_MAX], usize)> {
    let (l, t, k) = (s.locale.as_bytes(), s.tz.as_bytes(), s.keymap.as_bytes());
    if l.len() > REGION_LOCALE_MAX || t.len() > REGION_TZ_MAX || k.len() > REGION_KEYMAP_MAX {
        return None;
    }
    let mut f = [0u8; SURFACE_SETTINGS_FRAME_MAX];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_SETTINGS));
    f[4..8].copy_from_slice(&s.gen.to_le_bytes());
    f[8] = s.theme;
    f[9] = s.profile;
    f[10] = s.hour_fmt;
    let mut n = 11;
    for field in [l, t, k] {
        f[n] = field.len() as u8;
        n += 1;
        f[n..n + field.len()].copy_from_slice(field);
        n += field.len();
    }
    Some((f, n))
}

/// Fail-closed decode (bounds, exact length, UTF-8). Future fields append as
/// optional tails (the region-keymap precedent); today the frame is exact.
#[must_use]
pub fn decode_surface_settings(frame: &[u8]) -> Option<SurfaceSettings<'_>> {
    if !has_op(frame, OP_SURFACE_SETTINGS) || frame.len() < HEADER_LEN + 7 + 3 {
        return None;
    }
    let gen = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let (theme, profile, hour_fmt) = (frame[8], frame[9], frame[10]);
    let mut n = 11;
    let mut fields = ["", "", ""];
    for (i, max) in [REGION_LOCALE_MAX, REGION_TZ_MAX, REGION_KEYMAP_MAX].iter().enumerate() {
        let len = usize::from(*frame.get(n)?);
        n += 1;
        if len > *max || frame.len() < n + len {
            return None;
        }
        fields[i] = core::str::from_utf8(&frame[n..n + len]).ok()?;
        n += len;
    }
    if frame.len() != n {
        return None; // trailing garbage fails closed (no tails defined yet)
    }
    Some(SurfaceSettings {
        gen,
        theme,
        profile,
        hour_fmt,
        locale: fields[0],
        tz: fields[1],
        keymap: fields[2],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::OP_SURFACE_INPUT;
    use crate::surface_text::REGION_HOUR_24;

    #[test]
    fn surface_settings_round_trip() {
        let snap = SurfaceSettings {
            gen: 0xDEAD_BEEF,
            theme: 0x31, // light + accent 3
            profile: 1,
            hour_fmt: REGION_HOUR_24,
            locale: "de-DE",
            tz: "Europe/Berlin",
            keymap: "de",
        };
        let (f, n) = encode_surface_settings(&snap).unwrap();
        assert_eq!(decode_surface_settings(&f[..n]), Some(snap));
        // Empty strings are valid (unset fields).
        let empty = SurfaceSettings { locale: "", tz: "", keymap: "", ..snap };
        let (f, n) = encode_surface_settings(&empty).unwrap();
        assert_eq!(decode_surface_settings(&f[..n]), Some(empty));
    }

    #[test]
    fn test_reject_surface_settings_malformed() {
        let snap = SurfaceSettings {
            gen: 7,
            theme: 1,
            profile: 0,
            hour_fmt: 0,
            locale: "de-DE",
            tz: "UTC",
            keymap: "de",
        };
        // Oversize fields reject on encode.
        assert!(encode_surface_settings(&SurfaceSettings {
            locale: "x-way-too-long-locale",
            ..snap
        })
        .is_none());
        assert!(
            encode_surface_settings(&SurfaceSettings { keymap: "way-too-long", ..snap }).is_none()
        );
        let (f, n) = encode_surface_settings(&snap).unwrap();
        // Truncation anywhere fails closed.
        for cut in [n - 1, n - 4, HEADER_LEN + 8, HEADER_LEN + 2] {
            assert_eq!(decode_surface_settings(&f[..cut]), None, "cut at {cut}");
        }
        // Lying length field fails closed.
        let mut lying = f;
        lying[11] = 200;
        assert_eq!(decode_surface_settings(&lying[..n]), None);
        // Trailing garbage fails closed (no optional tails defined).
        let mut long = [0u8; SURFACE_SETTINGS_FRAME_MAX + 1];
        long[..n].copy_from_slice(&f[..n]);
        assert_eq!(decode_surface_settings(&long[..n + 1]), None);
        // Wrong op fails closed.
        let mut wrong_op = f;
        wrong_op[3] = OP_SURFACE_INPUT;
        assert_eq!(decode_surface_settings(&wrong_op[..n]), None);
    }
}
