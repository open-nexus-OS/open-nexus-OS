// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: screencapd's save (OS target, RFC-0095) — a rectangle of the frozen frame becomes
//! `/Bilder/Screenshots/<stem>.png` (or `<stem> (n).png` when taken). The folder is made if
//! missing; the name is claimed with an exclusive create; the PNG is encoded row by row —
//! each row read from the frame VMO, the pointer blended in when asked — into a VMO sized for
//! the encoder's worst case, which is then armed at vfsd and written in ONE transaction
//! (`OP_ARM_VMO` + `OP_WRITE_VMO`). Nothing is mapped and nothing file-sized touches the heap.
//! A failure after the create removes the claimed name, so no half-written file is left under
//! it; the encoder's scratch is scrubbed after every save (it held the user's pixels).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU — `screencapd: saved (kind=… w=… h=… bytes=…)` on the usb-visible lane

use nexus_service_topology::slots::screencapd as slots;
use nexus_vfs_types::fileops;
use nexus_vfs_types::{VfsError, CODE_OK};
use nexus_wire::screencapd as wire;

use crate::os_lite::Service;
use crate::plan::{self, Frozen, Rect, NAME_BUF, PATH_BUF};

/// A saved file: its name (for the shell's toast) and its size.
pub(crate) struct Saved {
    name: [u8; NAME_BUF],
    name_len: usize,
    pub(crate) bytes: u64,
}

impl Saved {
    pub(crate) fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// Why the encode stopped (the sink's and the row source's shared error).
enum SaveError {
    /// A frame row could not be read.
    Read,
    /// The output VMO refused bytes.
    Write,
    /// The PNG outgrew its bound (never, by `max_encoded_len`; checked anyway).
    Full,
}

/// The encoder's destination: the output VMO, written front to back.
struct VmoSink {
    vmo: u32,
    pos: usize,
    cap: usize,
}

impl png_encode::Sink for VmoSink {
    type Error = SaveError;

    fn write(&mut self, bytes: &[u8]) -> Result<(), SaveError> {
        let end = self.pos.checked_add(bytes.len()).ok_or(SaveError::Full)?;
        if end > self.cap {
            return Err(SaveError::Full);
        }
        nexus_abi::vmo_write(self.vmo, self.pos, bytes).map_err(|_| SaveError::Write)?;
        self.pos = end;
        Ok(())
    }
}

/// Saves `rect` of the frozen frame (with the pointer when `pointer`) under `stem`.
pub(crate) fn save(
    svc: &mut Service,
    frozen: &Frozen,
    rect: Rect,
    pointer: bool,
    stem: &str,
) -> Result<Saved, u8> {
    ensure_folder()?;
    let mut saved = Saved { name: [0u8; NAME_BUF], name_len: 0, bytes: 0 };
    let mut path = [0u8; PATH_BUF];
    let mut path_len = 0;
    for attempt in 1..=plan::ATTEMPTS_MAX {
        let n = plan::file_name(stem, attempt, &mut saved.name).ok_or(wire::STATUS_MALFORMED)?;
        let name = core::str::from_utf8(&saved.name[..n]).map_err(|_| wire::STATUS_MALFORMED)?;
        let p = plan::file_path(name, &mut path).ok_or(wire::STATUS_MALFORMED)?;
        let at = core::str::from_utf8(&path[..p]).map_err(|_| wire::STATUS_MALFORMED)?;
        match vfs(fileops::OP_CREATE, fileops::encode_path_request(at)) {
            Ok(()) => {
                saved.name_len = n;
                path_len = p;
                break;
            }
            Err(code) if code == VfsError::Exists.code() => continue,
            Err(_) => return Err(wire::STATUS_STORAGE),
        }
    }
    if saved.name_len == 0 {
        return Err(wire::STATUS_STORAGE);
    }
    let at = core::str::from_utf8(&path[..path_len]).map_err(|_| wire::STATUS_MALFORMED)?;
    match write_png(svc, frozen, rect, pointer, at) {
        Ok(bytes) => {
            saved.bytes = bytes;
            Ok(saved)
        }
        Err(status) => {
            let _ = vfs(fileops::OP_REMOVE, fileops::encode_path_request(at));
            Err(status)
        }
    }
}

/// The Pictures folder and its `Screenshots` (an existing one is fine).
fn ensure_folder() -> Result<(), u8> {
    for dir in [plan::PICTURES, plan::FOLDER] {
        match vfs(fileops::OP_MKDIR, fileops::encode_path_request(dir)) {
            Ok(()) => {}
            Err(code) if code == VfsError::Exists.code() => {}
            Err(_) => return Err(wire::STATUS_STORAGE),
        }
    }
    Ok(())
}

/// Encodes into a fresh VMO sized for the worst case and hands it to vfsd; the VMO is
/// destroyed afterwards (vfsd closed its clone before answering) and the scratch scrubbed.
fn write_png(
    svc: &mut Service,
    frozen: &Frozen,
    rect: Rect,
    pointer: bool,
    path: &str,
) -> Result<u64, u8> {
    let bound = png_encode::max_encoded_len(rect.w, rect.h);
    let vmo_len = bound.checked_next_multiple_of(4096).ok_or(wire::STATUS_STORAGE)?;
    let vmo = nexus_abi::vmo_create(vmo_len).map_err(|_| wire::STATUS_STORAGE)?;
    let result = encode_and_write(svc, frozen, rect, pointer, path, vmo, bound);
    let _ = nexus_abi::vmo_destroy(vmo);
    svc.encoder.reset();
    result
}

fn encode_and_write(
    svc: &mut Service,
    frozen: &Frozen,
    rect: Rect,
    pointer: bool,
    path: &str,
    vmo: u32,
    bound: usize,
) -> Result<u64, u8> {
    let mut sink = VmoSink { vmo, pos: 0, cap: bound };
    let (frame, frame_w) = (svc.frame, frozen.w as usize);
    let sprite = &svc.sprite;
    let rows = |y: u32, out: &mut [u8]| {
        let display_y = rect.y + y;
        let at = (display_y as usize * frame_w + rect.x as usize) * 4;
        nexus_abi::vmo_read(frame, at, out).map_err(|_| SaveError::Read)?;
        if pointer {
            crate::pointer::blend_row(out, display_y, rect.x, &frozen.pointer, sprite);
        }
        Ok(())
    };
    let written =
        svc.encoder.encode_rows(rect.w, rect.h, rows, &mut sink).map_err(|e| match e {
            png_encode::EncodeError::Sink(SaveError::Read) => wire::STATUS_FAILED,
            png_encode::EncodeError::Sink(SaveError::Write | SaveError::Full) => {
                wire::STATUS_STORAGE
            }
            _ => wire::STATUS_MALFORMED,
        })?;
    let len = u32::try_from(written).map_err(|_| wire::STATUS_STORAGE)?;
    // Arm the VMO (a plain clone, fire-and-forget), then the write that consumes it.
    let clone = nexus_abi::cap_clone(vmo).map_err(|_| wire::STATUS_STORAGE)?;
    if nexus_ipc::exchange::send_with_cap(slots::VFSD.send, &[fileops::OP_ARM_VMO], clone).is_err()
    {
        let _ = nexus_abi::cap_close(clone);
        return Err(wire::STATUS_STORAGE);
    }
    vfs(fileops::OP_WRITE_VMO, fileops::encode_write_vmo(path, len))
        .map_err(|_| wire::STATUS_STORAGE)?;
    Ok(u64::from(len))
}

/// One home op at vfsd (`op` + its payload) over the reply inbox; `Err` carries the RFC-0072
/// code (`Invalid` for a payload that would not encode, `Io` for a transport failure).
fn vfs(op: u8, payload: Option<alloc::vec::Vec<u8>>) -> Result<(), u16> {
    let payload = payload.ok_or(VfsError::Invalid.code())?;
    let mut frame = [0u8; 1 + 2 + PATH_BUF + 4];
    let body = frame.get_mut(1..1 + payload.len()).ok_or(VfsError::Invalid.code())?;
    body.copy_from_slice(&payload);
    frame[0] = op;
    let mut rsp = [0u8; 8];
    let n = nexus_ipc::exchange::call_into(
        slots::VFSD.send,
        slots::REPLY,
        &frame[..1 + payload.len()],
        &mut rsp,
    )
    .map_err(|_| VfsError::Io.code())?;
    match rsp.get(..n).and_then(fileops::decode_status_reply) {
        Some(CODE_OK) => Ok(()),
        Some(code) => Err(code),
        None => Err(VfsError::Io.code()),
    }
}
