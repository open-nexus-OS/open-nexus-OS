// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the ONE pixel readback (RFC-0095, ADR-0071) — the request and its CPU half. `serve`
//! answers `OP_READBACK` for whichever display gpud drives (GL copies its front render target
//! in `gl_probe`). On the virtio 2D path
//! and the board's display controller the shown frame is the display plane of the framebuffer
//! the CPU executor writes; a readback copies a rectangle of it into the caller's VMO, rows
//! tight (`w × 4`, BGRA), and a freeze copies the whole display plane into the retained plane
//! — the compositor's base layer — so the frame stays on screen under the shell's overlay. The
//! copies are pure functions on byte slices (host-tested with a checkerboard); the OS part
//! maps the moved VMO for the one copy and unmaps it again (the destination is the caller's
//! memory, never gpud's).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

use nexus_display_proto::readback::Readback;

/// Why a copy was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyError {
    /// The rectangle leaves the source plane (or is empty).
    OutOfBounds,
    /// The destination holds fewer than `w × h × 4` bytes.
    DestinationTooSmall,
}

/// Copies `rect` of `plane` (rows of `stride` bytes, BGRA) into `dst`, rows tight.
pub(crate) fn copy_rect_out(
    plane: &[u8],
    stride: usize,
    plane_w: u32,
    plane_h: u32,
    rect: &Readback,
    dst: &mut [u8],
) -> Result<(), CopyError> {
    if !rect.fits(plane_w, plane_h) || (plane_w as usize) * 4 > stride {
        return Err(CopyError::OutOfBounds);
    }
    let row_bytes = usize::from(rect.w) * 4;
    if dst.len() < rect.bytes() {
        return Err(CopyError::DestinationTooSmall);
    }
    for row in 0..usize::from(rect.h) {
        let src_at = (usize::from(rect.y) + row) * stride + usize::from(rect.x) * 4;
        let src = plane.get(src_at..src_at + row_bytes).ok_or(CopyError::OutOfBounds)?;
        let out = dst.get_mut(row * row_bytes..(row + 1) * row_bytes);
        out.ok_or(CopyError::DestinationTooSmall)?.copy_from_slice(src);
    }
    Ok(())
}

/// The freeze: copies `rows` rows of `row_bytes` from byte offset `from` to byte offset `to`
/// inside one buffer (the display plane into the retained plane). The ranges must not overlap.
pub(crate) fn copy_plane(
    buf: &mut [u8],
    from: usize,
    to: usize,
    stride: usize,
    row_bytes: usize,
    rows: usize,
) -> Result<(), CopyError> {
    let span = (rows.saturating_sub(1)) * stride + row_bytes;
    let (from_end, to_end) = (from + span, to + span);
    if rows == 0 || row_bytes > stride || from_end > buf.len() || to_end > buf.len() {
        return Err(CopyError::OutOfBounds);
    }
    if from < to_end && to < from_end {
        return Err(CopyError::OutOfBounds);
    }
    for row in 0..rows {
        let (s, d) = (from + row * stride, to + row * stride);
        buf.copy_within(s..s + row_bytes, d);
    }
    Ok(())
}

/// `OP_READBACK` in the request loop: the destination VMO rides the request and is the caller's
/// memory — used for this one copy, then its slot is closed (a moved capability has no Drop).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn serve(
    display: &mut dyn crate::backend::display::Display,
    frame: &[u8],
    moved: Option<nexus_ipc::ReplyCap>,
) -> u8 {
    match (nexus_display_proto::readback::decode_readback(frame), moved) {
        (Some(req), Some(cap)) => {
            let status = display.readback(cap.slot(), &req);
            cap.close();
            status
        }
        (_, cap) => {
            if let Some(cap) = cap {
                cap.close();
            }
            nexus_display_proto::STATUS_MALFORMED
        }
    }
}

/// Maps the moved destination VMO for one copy, runs `f` on its bytes, unmaps it. The map is
/// read-write and exactly page-rounded to `len`; a VMO shorter than `len` fails to map.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn with_destination<R>(
    vmo: u32,
    len: usize,
    f: impl FnOnce(&mut [u8]) -> R,
) -> Option<R> {
    let map_len = (len + 4095) & !4095;
    if map_len == 0 {
        return None;
    }
    let flags = nexus_abi::page_flags::VALID
        | nexus_abi::page_flags::USER
        | nexus_abi::page_flags::READ
        | nexus_abi::page_flags::WRITE;
    let va = nexus_abi::vm_map(vmo, 0, map_len, flags).ok()?;
    // SAFETY: `va..va + map_len` is the caller's VMO, mapped read-write for this call alone;
    // nothing else in gpud aliases it, and it is unmapped before returning.
    let bytes = unsafe { core::slice::from_raw_parts_mut(va as *mut u8, len) };
    let out = f(bytes);
    if let Err(err) = nexus_abi::vm_unmap(va, map_len) {
        crate::diag::err_line(b"readback unmap", err);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_display_proto::readback::READBACK_FREEZE;

    /// A 16×8 plane in a 20-pixel stride; pixel (x, y) = [x, y, x ^ y, 0xFF].
    fn checkerboard() -> (alloc::vec::Vec<u8>, usize) {
        let stride = 20 * 4;
        let mut plane = alloc::vec![0u8; stride * 8];
        for y in 0..8 {
            for x in 0..16 {
                let at = y * stride + x * 4;
                plane[at..at + 4].copy_from_slice(&[x as u8, y as u8, (x ^ y) as u8, 0xFF]);
            }
        }
        (plane, stride)
    }

    #[test]
    fn a_rectangle_lands_tight_and_exact() {
        let (plane, stride) = checkerboard();
        let rect = Readback { x: 3, y: 2, w: 5, h: 4, flags: 0 };
        let mut dst = alloc::vec![0u8; rect.bytes()];
        copy_rect_out(&plane, stride, 16, 8, &rect, &mut dst).expect("copies");
        for row in 0..4 {
            for col in 0..5 {
                let at = (row * 5 + col) * 4;
                let (x, y) = (3 + col, 2 + row);
                assert_eq!(&dst[at..at + 4], &[x as u8, y as u8, (x ^ y) as u8, 0xFF]);
            }
        }
    }

    #[test]
    fn the_freeze_copies_the_display_plane_into_the_retained_plane() {
        let stride = 8 * 4;
        let mut buf = alloc::vec![0u8; stride * 8];
        for (i, b) in buf[stride * 4..].iter_mut().enumerate() {
            *b = (i % 251) as u8; // the "display plane": rows 4..8
        }
        let want = buf[stride * 4..].to_vec();
        copy_plane(&mut buf, stride * 4, 0, stride, stride, 4).expect("freezes");
        assert_eq!(&buf[..stride * 4], want.as_slice());
        let _ = READBACK_FREEZE;
    }

    #[test]
    fn test_reject_rectangles_outside_the_plane() {
        let (plane, stride) = checkerboard();
        let mut dst = alloc::vec![0u8; 4096];
        for rect in [
            Readback { x: 12, y: 0, w: 5, h: 1, flags: 0 },
            Readback { x: 0, y: 7, w: 1, h: 2, flags: 0 },
            Readback { x: 0, y: 0, w: 0, h: 1, flags: 0 },
            Readback { x: u16::MAX, y: 0, w: 2, h: 1, flags: 0 },
        ] {
            assert_eq!(
                copy_rect_out(&plane, stride, 16, 8, &rect, &mut dst),
                Err(CopyError::OutOfBounds),
                "{rect:?}"
            );
        }
    }

    #[test]
    fn test_reject_a_destination_smaller_than_the_rectangle() {
        let (plane, stride) = checkerboard();
        let rect = Readback { x: 0, y: 0, w: 4, h: 4, flags: 0 };
        let mut dst = alloc::vec![0u8; rect.bytes() - 1];
        assert_eq!(
            copy_rect_out(&plane, stride, 16, 8, &rect, &mut dst),
            Err(CopyError::DestinationTooSmall)
        );
    }

    #[test]
    fn test_reject_overlapping_or_overlong_plane_copies() {
        let stride = 8 * 4;
        let mut buf = alloc::vec![0u8; stride * 8];
        assert_eq!(copy_plane(&mut buf, 0, stride, stride, stride, 4), Err(CopyError::OutOfBounds));
        assert_eq!(
            copy_plane(&mut buf, stride * 4, 0, stride, stride, 5),
            Err(CopyError::OutOfBounds),
            "past the buffer"
        );
        assert_eq!(
            copy_plane(&mut buf, 0, stride * 4, stride, stride + 4, 1),
            Err(CopyError::OutOfBounds)
        );
    }
}
