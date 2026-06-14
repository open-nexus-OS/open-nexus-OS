// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Headless debug instrument. Renders an RGBA8 framebuffer region as a
//! small ASCII-art thumbnail over the serial console, so our OWN graphical
//! output can be inspected without any host display server — open-nexus-OS uses
//! no X11, no Wayland, and the QEMU window is irrelevant (a host-side bug). Used
//! to bisect the compositor pipeline: the windowd-composited VMO (the source)
//! versus the GPU scanout readback (what is actually presented).
//! OWNERS: @gpud
//! STATUS: Experimental (debug-only)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host unit tests (luminance ramp + cell mapping).

#[cfg(all(feature = "os-lite", target_os = "none"))]
use nexus_abi::debug_println;

/// Thumbnail grid. 80x40 fits a terminal and resolves panel/sidebar/chat blocks.
pub const THUMB_COLS: usize = 80;
pub const THUMB_ROWS: usize = 40;

/// Dark->light luminance ramp. ASCII-only so `from_utf8` never fails.
const RAMP: &[u8] = b" .:-=+*#%@";

/// Map an 8-bit-per-channel RGB sample to a ramp glyph (Rec.601 luma).
fn glyph_for(r: u32, g: u32, b: u32) -> u8 {
    let lum = (r * 77 + g * 150 + b * 29) >> 8; // 0..255
    RAMP[(lum as usize * (RAMP.len() - 1)) / 255]
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn emit_tagged(prefix: &[u8], tag: &str) {
    let mut buf = [0u8; 96];
    let mut p = 0usize;
    for &b in prefix.iter().chain(tag.as_bytes()) {
        if p < buf.len() {
            buf[p] = b;
            p += 1;
        }
    }
    if let Ok(s) = core::str::from_utf8(&buf[..p]) {
        let _ = debug_println(s);
    }
}

/// Render an RGBA8 region as an ASCII thumbnail to the serial console.
///
/// `base` points at RGBA8 pixels; `len` bounds the buffer in bytes; `stride_px`
/// is the row stride in pixels; the sampled region is `(x0,y0)..(x0+w, y0+h)`.
/// Each cell samples its center pixel (cheap). Pure / no-alloc (one stack line
/// buffer), safe to call from gpud's bump-allocator context (no heap, no
/// per-frame `format!`/`Vec`). Output is bracketed by `gpud: THUMB BEGIN <tag>`
/// / `END`, every picture row prefixed `T ` so it survives interleaved UART:
/// reassemble with `grep '^T '`.
///
/// # Safety
/// `base` must be valid for `len` bytes for the duration of the call.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub unsafe fn emit_ascii_thumbnail(
    tag: &str,
    base: *const u8,
    len: usize,
    stride_px: usize,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
) {
    if base.is_null() || w == 0 || h == 0 || stride_px == 0 {
        return;
    }
    emit_tagged(b"gpud: THUMB BEGIN ", tag);
    let mut line = [0u8; THUMB_COLS + 2];
    line[0] = b'T';
    line[1] = b' ';
    for cy in 0..THUMB_ROWS {
        let sy = y0 + (cy * h + h / 2) / THUMB_ROWS;
        let mut p = 2usize;
        for cx in 0..THUMB_COLS {
            let sx = x0 + (cx * w + w / 2) / THUMB_COLS;
            let off = (sy * stride_px + sx) * 4;
            line[p] = if off + 3 < len {
                let r = base.add(off).read_volatile() as u32;
                let g = base.add(off + 1).read_volatile() as u32;
                let b = base.add(off + 2).read_volatile() as u32;
                glyph_for(r, g, b)
            } else {
                b'?'
            };
            p += 1;
        }
        if let Ok(s) = core::str::from_utf8(&line[..p]) {
            let _ = debug_println(s);
        }
    }
    emit_tagged(b"gpud: THUMB END ", tag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_endpoints_map_black_to_space_and_white_to_at() {
        assert_eq!(glyph_for(0, 0, 0), b' ');
        assert_eq!(glyph_for(255, 255, 255), b'@');
    }

    #[test]
    fn ramp_is_monotonic_in_luminance() {
        let mut last = 0usize;
        for v in (0..=255).step_by(15) {
            let g = glyph_for(v, v, v);
            let idx = RAMP.iter().position(|&c| c == g).unwrap();
            assert!(idx >= last, "ramp regressed at v={v}");
            last = idx;
        }
    }
}
