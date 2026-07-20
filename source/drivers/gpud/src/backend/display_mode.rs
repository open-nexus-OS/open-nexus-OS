// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Display-mode resolution policy (RFC-0074 / ADR-0050). The compositor
//! OWNS the visible mode; this pure function decides which candidate gpud commands
//! onto the scanout. Split out of `backend/mod.rs` so the policy + its invariant
//! test stand alone (structure-gate: keep the backend god-file from growing).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable

/// Resolve the VISIBLE display mode the compositor commands (RFC-0074 / ADR-0050).
///
/// Authority order: the fw_cfg-**configured** mode (kernel-derived, race-free) wins;
/// else the device's advertised **capability**; else the fixed `layout_max`. Every
/// candidate is validated (non-zero) and clamped to `layout_max` — so a racy or
/// malicious device report can never shrink or enlarge the scanout to a degenerate
/// size. Pure + bounded; the negative test below proves the invariant.
///
/// Compiled for the OS build (its only caller) and for host `test` (no dead-code on
/// a plain host `cargo check`, where neither cfg is active).
#[cfg(any(all(feature = "os-lite", target_os = "none"), test))]
pub(crate) fn resolve_display_mode(
    configured: Option<(u32, u32)>,
    device: Option<(u32, u32)>,
    layout_max: (u32, u32),
) -> (u32, u32) {
    let sane = |wh: Option<(u32, u32)>| -> Option<(u32, u32)> {
        wh.and_then(|(w, h)| {
            if w == 0 || h == 0 {
                None
            } else {
                Some((w.min(layout_max.0), h.min(layout_max.1)))
            }
        })
    };
    sane(configured).or_else(|| sane(device)).unwrap_or(layout_max)
}

#[cfg(test)]
mod tests {
    use super::resolve_display_mode;

    const MAX: (u32, u32) = (1280, 800);

    #[test]
    fn configured_wins_over_device() {
        // The GTK race makes the device report the tiny window default; the
        // fw_cfg-configured mode is authoritative and must win.
        assert_eq!(resolve_display_mode(Some((1280, 800)), Some((640, 507)), MAX), (1280, 800));
    }

    #[test]
    fn follows_configured_smaller_mode() {
        assert_eq!(resolve_display_mode(Some((1024, 768)), Some((640, 507)), MAX), (1024, 768));
    }

    #[test]
    fn device_capability_used_when_unconfigured() {
        assert_eq!(resolve_display_mode(None, Some((1024, 768)), MAX), (1024, 768));
    }

    #[test]
    fn falls_back_to_layout_max() {
        assert_eq!(resolve_display_mode(None, None, MAX), MAX);
    }

    #[test]
    fn test_reject_degenerate_display_mode() {
        // Zero / degenerate reports are rejected, never sizing the scanout.
        assert_eq!(resolve_display_mode(Some((0, 0)), None, MAX), MAX);
        assert_eq!(resolve_display_mode(Some((1280, 0)), Some((0, 800)), MAX), MAX);
        // Oversized is clamped to the layout maximum, never enlarged.
        assert_eq!(resolve_display_mode(Some((5000, 5000)), None, MAX), MAX);
        assert_eq!(resolve_display_mode(None, Some((99999, 1)), MAX), (1280, 1));
    }
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
