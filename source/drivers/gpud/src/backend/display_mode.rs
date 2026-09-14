// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The `gpud: display info WxH` marker. The resolution POLICY itself moved to
//! `nexus_display_proto::resolve_display_mode` (TASK-0324 P6-a): it lived here alone, which
//! is why windowd and inputd each grew their own protocol to ask someone else for the mode.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable

/// The VISIBLE mode gpud commands onto the scanout (RFC-0074 / ADR-0050, RFC-0093 §5).
///
/// The clamp POLICY lives once, in `nexus_display_proto`; this wrapper adds the one thing only
/// gpud can know — whether the device's advertised capability DISAGREES with the configured
/// mode. The configured mode still wins (kernel-derived and race-free, which is the point of
/// RFC-0074), but the disagreement is named instead of silently overruled: the GTK-window race
/// that made a device report its un-realized default used to be invisible.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(super) fn resolve(configured: Option<(u32, u32)>, device: Option<(u32, u32)>) -> (u32, u32) {
    if let (Some((cw, ch)), Some((dw, dh))) = (configured, device) {
        if (cw, ch) != (dw, dh) {
            emit_mode_mismatch(cw, ch, dw, dh);
        }
    }
    nexus_display_proto::resolve_display_mode(configured, device, nexus_display_proto::LAYOUT_MAX)
}

/// `gpud: display info WxH` — the resolved visible mode (alloc-free: gpud's
/// stack-buffer marker pattern, the heap never sees boot markers).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(super) fn emit_display_info_marker(w: u32, h: u32) {
    fn put(buf: &mut [u8; 40], p: &mut usize, s: &[u8]) {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    }
    fn put_dec(buf: &mut [u8; 40], p: &mut usize, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut n = 0;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            put(buf, p, &tmp[n..=n]);
        }
    }
    let mut buf = [0u8; 40];
    let mut p = 0usize;
    put(&mut buf, &mut p, b"gpud: display info ");
    put_dec(&mut buf, &mut p, w);
    put(&mut buf, &mut p, b"x");
    put_dec(&mut buf, &mut p, h);
    let _ = nexus_abi::trace_line(core::str::from_utf8(&buf[..p]).unwrap_or("gpud: display info"));
}

/// `gpud: FAIL display mode <cfg> vs device <cap>` (RFC-0093 §5) — the device advertises a
/// mode other than the configured one. The configured mode still wins (it is kernel-derived
/// and race-free, which is the whole point of RFC-0074), but the disagreement is EVIDENCE:
/// the GTK-window race that made a device report the un-realized default used to be invisible.
/// Alloc-free, same stack-buffer pattern as the info marker.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn emit_mode_mismatch(cfg_w: u32, cfg_h: u32, dev_w: u32, dev_h: u32) {
    fn put(buf: &mut [u8; 72], p: &mut usize, s: &[u8]) {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    }
    fn put_dec(buf: &mut [u8; 72], p: &mut usize, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut n = 0;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            put(buf, p, &tmp[n..=n]);
        }
    }
    let mut buf = [0u8; 72];
    let mut p = 0usize;
    put(&mut buf, &mut p, b"gpud: FAIL display mode ");
    put_dec(&mut buf, &mut p, cfg_w);
    put(&mut buf, &mut p, b"x");
    put_dec(&mut buf, &mut p, cfg_h);
    put(&mut buf, &mut p, b" vs device ");
    put_dec(&mut buf, &mut p, dev_w);
    put(&mut buf, &mut p, b"x");
    put_dec(&mut buf, &mut p, dev_h);
    let _ = nexus_abi::trace_line(
        core::str::from_utf8(&buf[..p]).unwrap_or("gpud: FAIL display mode mismatch"),
    );
}
