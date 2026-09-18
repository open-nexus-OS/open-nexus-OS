// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the OS-side VFS client — request frames over kernel IPC, and the
//! zero-copy read (`OP_READ_VMO`, RFC-0072 Phase 3 / RFC-0097).
//!
//! The splice is the reason this is its own file: a bulk read is not "a call
//! that returns bytes". The caller creates the VMO, moves it to vfsd, and waits
//! for the header that the authority who verified the bytes writes LAST. The
//! payload never passes through a reply frame, and — with `read_vmo_sample` —
//! need never pass through the caller's heap either.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU (`SELFTEST: vfs splice roundtrip ok`, `SELFTEST: pkgimg vmo ok`)

use alloc::{format, vec::Vec};

use super::{Error, Result};
use nexus_ipc::{Client as _, KernelClient, Wait};

/// OS backend forwarding requests to the kernel IPC channel.
pub struct Client {
    ipc: KernelClient,
}

impl Client {
    pub fn new() -> Result<Self> {
        // Route to the vfsd service (init-lite responder).
        let ipc = KernelClient::new_for("vfsd").map_err(map_ipc_error)?;
        Ok(Self { ipc })
    }

    pub fn call(&self, frame: Vec<u8>) -> Result<Vec<u8>> {
        if let Err(err) = self.ipc.send(&frame, Wait::Blocking) {
            return Err(map_ipc_error(err));
        }
        self.ipc.recv(Wait::Blocking).map_err(map_ipc_error)
    }

    /// Zero-copy read: create a `cap`-byte VMO, CAP_MOVE a clone to vfsd
    /// with the `OP_READ_VMO` request, then wait for the header the writing
    /// authority releases into it (payload-first, header-last) and copy the
    /// bytes back. The copy is the convenience, not the mechanism — see
    /// [`read_vmo_sample`](Self::read_vmo_sample) for an entry too big to
    /// land in the caller's heap.
    pub fn read_vmo(&self, path: &str, cap: usize) -> Result<Vec<u8>> {
        let (vmo, len) = self.splice_into_vmo(path, cap)?;
        let mut out = alloc::vec![0u8; len];
        let ok = out.is_empty()
            || nexus_abi::vmo_read(vmo, nexus_vfs_types::SPLICE_DATA_OFFSET, &mut out).is_ok();
        let _ = nexus_abi::cap_close(vmo);
        if ok {
            Ok(out)
        } else {
            Err(Error::Ipc("splice payload read".into()))
        }
    }

    /// Splices and reports the length, copying out only `sample`.
    pub fn read_vmo_sample(&self, path: &str, cap: usize, sample: &mut [u8]) -> Result<usize> {
        let (vmo, len) = self.splice_into_vmo(path, cap)?;
        let take = core::cmp::min(sample.len(), len);
        let ok = take == 0
            || nexus_abi::vmo_read(vmo, nexus_vfs_types::SPLICE_DATA_OFFSET, &mut sample[..take])
                .is_ok();
        let _ = nexus_abi::cap_close(vmo);
        if ok {
            Ok(len)
        } else {
            Err(Error::Ipc("splice payload read".into()))
        }
    }

    /// The splice itself: move a fresh VMO to vfsd and wait for the header
    /// the writing authority releases. Returns the VMO (still open) and the
    /// payload length.
    fn splice_into_vmo(&self, path: &str, cap: usize) -> Result<(u32, usize)> {
        use nexus_vfs_types::{
            decode_splice_header, encode_read_vmo_request, VfsError, OP_READ_VMO, SPLICE_HEADER_LEN,
        };
        /// Bounded header-poll attempts (with a yield between each).
        const SPLICE_POLL_MAX: u32 = 200_000;
        let payload = encode_read_vmo_request(path).ok_or(Error::InvalidPath)?;
        let mut frame = Vec::with_capacity(1 + payload.len());
        frame.push(OP_READ_VMO);
        frame.extend_from_slice(&payload);
        // Create the VMO (kept for read-back) + a clone to hand to vfsd.
        let vmo = nexus_abi::vmo_create(cap).map_err(|_| Error::Unsupported)?;
        let Ok(clone) = nexus_abi::cap_clone(vmo) else {
            let _ = nexus_abi::cap_close(vmo);
            return Err(Error::Unsupported);
        };
        // The moved cap is the splice VMO, not a reply inbox: CAP_MOVE consumes `clone` on
        // success, so it is only closed on failure.
        let send = nexus_ipc::exchange::send_with_cap(self.ipc.slots().0, &frame, clone);
        if let Err(err) = send {
            let _ = nexus_abi::cap_close(clone);
            let _ = nexus_abi::cap_close(vmo);
            return Err(map_ipc_error(err));
        }
        // Poll the splice header (magic absent = provider still writing).
        let mut hdr = [0u8; SPLICE_HEADER_LEN];
        let mut attempts = 0u32;
        let (status, len) = loop {
            if nexus_abi::vmo_read(vmo, 0, &mut hdr).is_ok() {
                if let Some(decoded) = decode_splice_header(&hdr) {
                    break decoded;
                }
            }
            attempts += 1;
            if attempts > SPLICE_POLL_MAX {
                let _ = nexus_abi::cap_close(vmo);
                return Err(Error::Ipc("splice header timeout".into()));
            }
            let _ = nexus_abi::yield_();
        };
        if status != nexus_vfs_types::CODE_OK {
            let _ = nexus_abi::cap_close(vmo);
            return Err(Error::Vfs(VfsError::from_code(status).unwrap_or(VfsError::Io)));
        }
        Ok((vmo, len as usize))
    }
}

fn map_ipc_error(err: nexus_ipc::IpcError) -> Error {
    match err {
        nexus_ipc::IpcError::Unsupported => Error::Unsupported,
        other => Error::Ipc(format!("{other:?}")),
    }
}
