// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: vfsd's packagefsd leg — the whole `pkg:/` namespace as vfsd sees it
//! (split from `os_lite.rs` under the structure ratchet, TASK-0033 P2).
//!
//! THE RULE HERE: vfsd relays, it does not carry. Metadata comes back in an
//! 11-byte reply; inline-tier bytes come back in one bounded frame; everything
//! larger moves as the CALLER'S VMO, forwarded on to packagefsd and from there
//! to bundlemgrd, which is the only hop that may write bytes or a success
//! header. vfsd used to hold whole entries — one `Vec` per resolve and another
//! per open handle, on a bump heap that never frees — and that is why 22 of the
//! system volume's 115 entries could not be read at all.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`SELFTEST: pkgimg …`, `SELFTEST: vfs …`)
//! RFC: docs/rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nexus_vfs_types::{
    decode_readdir_response, encode_readdir_error, encode_readdir_request, VfsError,
};

use crate::os_lite::{
    debug_print, map_namespace_error, Entry, Error, Result, KIND_FILE, PKGFS_OPCODE_ARM_VMO,
    PKGFS_OPCODE_LIST, PKGFS_OPCODE_READ, PKGFS_OPCODE_READ_VMO, PKGFS_OPCODE_RESOLVE,
};
use crate::NamespaceView;

pub(crate) struct Namespace {
    view: NamespaceView,
}

impl Namespace {
    pub(crate) fn new() -> Self {
        Self { view: NamespaceView::new(vec!["pkg:/".to_string()]) }
    }

    /// The packagefsd leg: request SEND plus vfsd's OWN reply inbox. Until
    /// TASK-0033 P2 the answers came back on packagefsd's shared response
    /// endpoint, which dsoftbusd and the harness read too — any of the three
    /// could take any answer. Now every request moves a reply capability and
    /// packagefsd answers exactly the sender that did.
    const PKGFS_SEND: u32 = nexus_service_topology::slots::vfsd::PACKAGEFSD.send;
    const PKGFS_REPLY: nexus_service_topology::SlotPair =
        nexus_service_topology::slots::vfsd::REPLY;

    /// Maps a `pkg:/` path to the bundle-relative path packagefsd speaks.
    fn packagefs_rel(&self, path: &str) -> Result<String> {
        let canonical = self.view.assert_allowed(path).map_err(map_namespace_error)?;
        let rel = canonical.strip_prefix("pkg:/").ok_or(Error::InvalidPath)?;
        if rel.is_empty() {
            return Err(Error::InvalidPath);
        }
        Ok(rel.to_string())
    }

    /// Asks packagefsd for one entry's METADATA. The reply is 11 bytes and
    /// carries no file content: entry bytes riding this frame is what made 22
    /// of the volume's 115 entries unreadable and fatal to packagefsd
    /// (TASK-0033, RFC-0097). `scratch` is the loop's reusable buffer — this
    /// service runs on a heap that never frees.
    fn packagefs_resolve(&self, path: &str, scratch: &mut [u8]) -> Result<Entry> {
        let rel = self.packagefs_rel(path)?;
        let mut frame = Vec::with_capacity(1 + rel.len());
        frame.push(PKGFS_OPCODE_RESOLVE);
        frame.extend_from_slice(rel.as_bytes());
        let n =
            nexus_ipc::exchange::call_into(Self::PKGFS_SEND, Self::PKGFS_REPLY, &frame, scratch)
                .map_err(|_| Error::Transport)?;
        let rsp = &scratch[..n];
        if rsp.len() < 11 || rsp[0] != 1 {
            return Err(Error::NotFound);
        }
        let size =
            u64::from_le_bytes([rsp[1], rsp[2], rsp[3], rsp[4], rsp[5], rsp[6], rsp[7], rsp[8]]);
        let kind = u16::from_le_bytes([rsp[9], rsp[10]]);
        Ok(Entry { kind, size })
    }

    /// Reads a whole entry of the INLINE tier into `scratch`, returning its
    /// length. An entry above `INLINE_IO_MAX` is `TooBig`: it has a VMO path
    /// (`OP_READ_VMO`) and must use it — never a silent truncation (RFC-0072).
    pub(crate) fn packagefs_read_inline(&self, path: &str, scratch: &mut [u8]) -> Result<usize> {
        let rel = self.packagefs_rel(path)?;
        let mut frame = Vec::with_capacity(1 + rel.len());
        frame.push(PKGFS_OPCODE_READ);
        frame.extend_from_slice(rel.as_bytes());
        let n =
            nexus_ipc::exchange::call_into(Self::PKGFS_SEND, Self::PKGFS_REPLY, &frame, scratch)
                .map_err(|_| Error::Transport)?;
        if n == 0 {
            return Err(Error::Transport);
        }
        match u16::from(scratch[0]) {
            code if code == nexus_vfs_types::CODE_OK => {
                scratch.copy_within(1..n, 0);
                Ok(n - 1)
            }
            code if code == VfsError::TooBig.code() => Err(Error::TooBig),
            _ => Err(Error::NotFound),
        }
    }

    /// FORWARDS the caller's VMO to packagefsd, which forwards it to bundlemgrd
    /// (RFC-0097 §2). vfsd touches no byte of the payload and writes no success
    /// header: only the authority that verified the digest may do that.
    pub(crate) fn packagefs_forward_vmo(
        &self,
        path: &str,
        vmo: u32,
    ) -> core::result::Result<(), VfsError> {
        let rel = self.packagefs_rel(path).map_err(|_| VfsError::NotFound)?;
        // A message moves ONE capability: ARM moves the VMO, the read that
        // follows moves the reply cap.
        let moved = nexus_abi::cap_clone(vmo).map_err(|_| VfsError::Io)?;
        let arm = [PKGFS_OPCODE_ARM_VMO];
        if nexus_ipc::exchange::send_with_cap(Self::PKGFS_SEND, &arm, moved).is_err() {
            let _ = nexus_abi::cap_close(moved);
            return Err(VfsError::Io);
        }
        let mut frame = Vec::with_capacity(1 + rel.len());
        frame.push(PKGFS_OPCODE_READ_VMO);
        frame.extend_from_slice(rel.as_bytes());
        let mut rsp = [0u8; 8];
        let n =
            nexus_ipc::exchange::call_into(Self::PKGFS_SEND, Self::PKGFS_REPLY, &frame, &mut rsp)
                .map_err(|_| VfsError::Io)?;
        if n < 6 {
            return Err(VfsError::Io);
        }
        let status = u16::from_le_bytes([rsp[0], rsp[1]]);
        match VfsError::from_code(status) {
            None => Ok(()),
            Some(err) => Err(err),
        }
    }

    pub(crate) fn stat(&self, path: &str, scratch: &mut [u8]) -> Result<Entry> {
        if path.starts_with("pkg:/") {
            return self.packagefs_resolve(path, scratch);
        }
        Err(Error::InvalidPath)
    }

    pub(crate) fn open(&self, path: &str, scratch: &mut [u8]) -> Result<Entry> {
        if !path.starts_with("pkg:/") {
            return Err(Error::InvalidPath);
        }
        let entry = self.packagefs_resolve(path, scratch)?;
        if entry.kind != KIND_FILE {
            return Err(Error::InvalidPath);
        }
        Ok(entry)
    }

    /// Relays a ReadDir request to packagefsd and returns the validated reply
    /// payload (shared `nexus-vfs-types` codec on both hops). The returned
    /// payload is sent to the caller verbatim; errors are already encoded.
    pub(crate) fn read_dir(
        &self,
        request_payload: &[u8],
        scratch: &mut [u8],
    ) -> (Vec<u8>, Option<usize>) {
        let request = match nexus_vfs_types::decode_readdir_request(request_payload) {
            Ok(request) => request,
            Err(err) => return (encode_readdir_error(err), None),
        };
        // Namespace: only pkg:/ paths exist in os-lite; "pkg:/" is the root.
        let rel = if request.path == "pkg:/" {
            ".".to_string()
        } else {
            let canonical = match self.view.assert_allowed(&request.path) {
                Ok(canonical) => canonical,
                Err(_) => {
                    debug_print("vfsd: access denied\n");
                    return (encode_readdir_error(VfsError::Access), None);
                }
            };
            match canonical.strip_prefix("pkg:/") {
                Some(rel) if !rel.is_empty() => rel.to_string(),
                _ => return (encode_readdir_error(VfsError::Invalid), None),
            }
        };
        let forwarded = match encode_readdir_request(&rel, request.cursor, request.limit) {
            Ok(payload) => payload,
            Err(err) => return (encode_readdir_error(err), None),
        };
        let mut frame = Vec::with_capacity(1 + forwarded.len());
        frame.push(PKGFS_OPCODE_LIST);
        frame.extend_from_slice(&forwarded);
        let n = match nexus_ipc::exchange::call_into(
            Self::PKGFS_SEND,
            Self::PKGFS_REPLY,
            &frame,
            scratch,
        ) {
            Ok(n) => n,
            Err(_) => return (encode_readdir_error(VfsError::Io), None),
        };
        // Validate before relaying: a malformed provider page must surface as
        // EIO here, never reach the app client half-broken.
        match decode_readdir_response(&scratch[..n]) {
            Ok(page) => {
                let count = page.entries.len();
                (scratch[..n].to_vec(), Some(count))
            }
            Err(err) => (encode_readdir_error(err), None),
        }
    }
}
