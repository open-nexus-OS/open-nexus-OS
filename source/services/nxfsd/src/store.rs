// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The `DataStore` — owns a virtio-blk device, mounts/formats an nxfs
//! container, and answers the `/data` provider protocol frames. Paths arrive
//! mount-relative (vfsd strips `/data`), so the store operates on absolute
//! nxfs paths. Never fakes success: an unmounted container answers EIO.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0293)
//! API_STABILITY: Unstable

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use nexus_vfs_types::fileops::{
    self, OP_CREATE, OP_MKDIR, OP_READ, OP_READDIR, OP_REMOVE, OP_RENAME, OP_STAT, OP_WRITE_TEXT,
};
use nexus_vfs_types::{FileKind, VfsError};
use nxfs::{MkfsOptions, Nxfs};
use storage::virtio_blk::VirtioBlkDevice;

/// The device-2 MMIO cap slot init grants for the user-data device (distinct
/// from statefsd's slot 48 on device 1). ADR-0044: one owner per virtio queue.
pub const DATA_MMIO_SLOT: u32 = 49;
/// Fixed container UUID for dev images (the engine takes no RNG).
const CONTAINER_UUID: [u8; 16] = *b"nexus-data-vol01";
/// The vfsd mount prefix routed to this store. v1: the store knows its mount
/// point so vfsd can forward `/data` frames verbatim (no per-op rewrite);
/// extracting to a mount-agnostic nxfsd process moves this to vfsd's router.
const MOUNT_PREFIX: &str = "/data";

/// Strips the `/data` mount prefix, yielding an absolute nxfs path (root = "/").
fn to_nxfs_path(path: &str) -> String {
    match path.strip_prefix(MOUNT_PREFIX) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            if rest.is_empty() {
                String::from("/")
            } else {
                String::from(rest)
            }
        }
        _ => String::from(path),
    }
}

fn mark(line: &str) {
    let _ = nexus_abi::debug_write(line.as_bytes());
    let _ = nexus_abi::debug_write(b"\n");
}

/// The mounted user-data store.
pub struct DataStore {
    fs: Nxfs<VirtioBlkDevice>,
}

impl DataStore {
    /// Acquires the device and mounts (or formats) the nxfs container. Returns
    /// `None` if the MMIO cap is not yet granted or the device is unusable —
    /// the caller retries (the grant may land after the server endpoint).
    pub fn acquire() -> Option<Self> {
        let mut query = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        if nexus_abi::cap_query(DATA_MMIO_SLOT, &mut query).is_err() {
            mark("nxfsd: no mmio cap at slot 49");
            return None;
        }
        if query.kind_tag != 2 {
            mark("nxfsd: slot 49 wrong kind (not DeviceMmio)");
            return None;
        }
        mark("nxfsd: mmio cap present, opening device");
        // The virtio-blk driver maps a FIXED MMIO window, so the device can be
        // created only ONCE — `open_or_format` peeks the superblock and either
        // mounts an existing container or formats a blank one, consuming the
        // device a single time (no silent reformat of a valid container).
        let device = match VirtioBlkDevice::new(DATA_MMIO_SLOT) {
            Ok(device) => device,
            Err(_) => {
                mark("nxfsd: virtio-blk open FAIL");
                return None;
            }
        };
        mark("nxfsd: device opened, mount/format");
        let opts = MkfsOptions { uuid: CONTAINER_UUID, journal_blocks: 64 };
        match Nxfs::open_or_format(device, opts) {
            Ok(fs) => {
                let fresh = fs.formatted_fresh;
                if fs.replay_discarded_tail {
                    mark("nxfsd: mounted /data (rw, recovered)");
                } else {
                    mark("nxfsd: mounted /data (rw, clean)");
                }
                let mut store = Self { fs };
                if fresh {
                    store.seed_first_run();
                }
                Some(store)
            }
            Err(_) => {
                mark("nxfsd: open/format FAIL");
                None
            }
        }
    }

    /// Seeds a blank container with a small set of first-run files spanning
    /// several types — an OS shipping example content — so the file-type icon
    /// pipeline (TASK-0294) has varied entries to render. Best-effort: a failed
    /// entry is skipped, never fatal. Only ever runs on a fresh `mkfs`.
    fn seed_first_run(&mut self) {
        let dirs = ["/Documents", "/Pictures"];
        let files: &[(&str, &[u8])] = &[
            ("/Welcome.txt", b"Welcome to Nexus.\n"),
            ("/Read Me.md", b"# Nexus\n\nYour files live here.\n"),
            ("/Report.pdf", b"%PDF-1.4 nexus demo\n"),
            ("/Photo.png", b"\x89PNG\r\n\x1a\n nexus demo"),
            ("/Song.mp3", b"ID3 nexus demo audio"),
            ("/Archive.zip", b"PK\x03\x04 nexus demo"),
            ("/config.json", b"{ \"nexus\": true }\n"),
        ];
        let mut n = 0u32;
        for dir in dirs {
            if self.fs.mkdir(dir).is_ok() {
                n += 1;
            }
        }
        for (path, content) in files {
            if self.fs.create(path).is_ok() {
                let _ = self.fs.write(path, 0, content);
                n += 1;
            }
        }
        let mut line = String::from("nxfsd: seeded first-run content (n=");
        push_u32(&mut line, n);
        line.push(')');
        mark(&line);
    }

    /// Answers one `/data` provider frame (opcode + mount-relative payload).
    pub fn handle(&mut self, frame: &[u8]) -> Vec<u8> {
        let Some((&opcode, payload)) = frame.split_first() else {
            return status_reply(VfsError::Invalid);
        };
        match opcode {
            OP_STAT => self.handle_stat(payload),
            OP_READ => self.handle_read(payload),
            OP_READDIR => self.handle_readdir(payload),
            OP_MKDIR => run_write(self.fs.mkdir(&decode_path(payload))),
            OP_CREATE => run_write(self.fs.create(&decode_path(payload))),
            OP_REMOVE => run_write(self.fs.remove(&decode_path(payload))),
            OP_WRITE_TEXT => match fileops::decode_write_text(payload) {
                Some((path, text)) => {
                    run_write(self.fs.write(&to_nxfs_path(&path), 0, text.as_bytes()))
                }
                None => status_reply(VfsError::Invalid),
            },
            OP_RENAME => match fileops::decode_rename(payload) {
                Some((from, to)) => {
                    run_write(self.fs.rename(&to_nxfs_path(&from), &to_nxfs_path(&to)))
                }
                None => status_reply(VfsError::Invalid),
            },
            _ => status_reply(VfsError::Unsupported),
        }
    }

    fn handle_stat(&self, payload: &[u8]) -> Vec<u8> {
        let path = decode_path(payload);
        match self.fs.stat(&path) {
            Ok((kind, size)) => {
                let kind_u16: u16 = if kind == FileKind::Dir { 1 } else { 0 };
                let mut out = Vec::with_capacity(11);
                out.push(1);
                out.extend_from_slice(&size.to_le_bytes());
                out.extend_from_slice(&kind_u16.to_le_bytes());
                out
            }
            Err(_) => alloc::vec![0u8],
        }
    }

    /// Reads a whole file's bytes (bounded by `max`) for the VMO-splice data
    /// plane (TASK-0295). Mount-relative path; errors map to `VfsError`.
    pub fn read_bytes(&self, path: &str, max: usize) -> core::result::Result<Vec<u8>, VfsError> {
        self.fs
            .read(&to_nxfs_path(path), 0, max)
            .map_err(|err| err.to_vfs())
    }

    fn handle_read(&self, payload: &[u8]) -> Vec<u8> {
        let path = decode_path(payload);
        match self.fs.read(&path, 0, fileops::MAX_INLINE_TEXT) {
            Ok(bytes) => {
                let mut out = Vec::with_capacity(2 + bytes.len());
                out.extend_from_slice(&nexus_vfs_types::CODE_OK.to_le_bytes());
                out.extend_from_slice(&bytes);
                out
            }
            Err(err) => status_reply(err.to_vfs()),
        }
    }

    fn handle_readdir(&self, payload: &[u8]) -> Vec<u8> {
        let request = match nexus_vfs_types::decode_readdir_request(payload) {
            Ok(request) => request,
            Err(err) => return nexus_vfs_types::encode_readdir_error(err),
        };
        let path = to_nxfs_path(&request.path);
        match self.fs.read_dir(&path, request.cursor, request.limit) {
            Ok(page) => nexus_vfs_types::encode_readdir_page(&page)
                .unwrap_or_else(nexus_vfs_types::encode_readdir_error),
            Err(err) => nexus_vfs_types::encode_readdir_error(err.to_vfs()),
        }
    }
}

/// An error readdir page for the unmounted-container path.
pub fn readdir_unavailable() -> Vec<u8> {
    nexus_vfs_types::encode_readdir_error(VfsError::Io)
}

/// A stat "not found" reply for the unmounted-container path.
pub fn stat_unavailable() -> Vec<u8> {
    alloc::vec![0u8]
}

/// A status reply for a write op when the container is unmounted.
pub fn write_unavailable() -> Vec<u8> {
    status_reply(VfsError::Io)
}

fn run_write(result: nxfs::Result<()>) -> Vec<u8> {
    match result {
        Ok(()) => fileops::encode_status_reply(nexus_vfs_types::CODE_OK),
        Err(err) => status_reply(err.to_vfs()),
    }
}

fn status_reply(err: VfsError) -> Vec<u8> {
    fileops::encode_status_reply(err.code())
}

fn decode_path(payload: &[u8]) -> String {
    to_nxfs_path(core::str::from_utf8(payload).unwrap_or(""))
}

/// Appends a decimal `u32` to a string (no `format!` in the mount hot path).
fn push_u32(out: &mut String, mut value: u32) {
    if value == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut i = digits.len();
    while value > 0 {
        i -= 1;
        digits[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.push_str(core::str::from_utf8(&digits[i..]).unwrap_or("?"));
}
