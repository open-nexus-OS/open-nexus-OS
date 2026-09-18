// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SSOT for the VFS client/server surface types shared across vfsd,
//! packagefsd, and the `nexus-vfs` client crate: the stable storage error
//! codes (RFC-0072, re-exported from the wire SSOT), directory-entry types +
//! bounds, and the bounded raw wire codec for the ReadDir op (os-lite frames).
//! One codec, two ends — servers and clients cannot drift apart on the byte
//! layout.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0291)
//! API_STABILITY: Unstable (error codes themselves are append-only stable)
//! TEST_COVERAGE: module unit tests (roundtrip, bounds, negative decode)

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod entry;
pub mod fileops;
pub mod splice;
mod wire;

pub use entry::{DirEntry, FileKind, MAX_ENTRIES_PER_PAGE, MAX_NAME_LEN, MAX_PATH_LEN};
// The RFC-0072 status table lives in the wire SSOT (`nexus_wire::status`): it is
// carried by the payload-VMO header too, which bundlemgrd, execd, init and
// app-host write and read without being VFS clients (TASK-0033 P1). Re-exported
// here so the VFS surface is unchanged.
pub use nexus_wire::status::{VfsError, CODE_OK};
pub use splice::{
    decode_read_vmo_request, decode_splice_header, encode_read_vmo_request, encode_splice_header,
    splice_fits, INLINE_IO_MAX, OP_READ_VMO, SPLICE_DATA_OFFSET, SPLICE_HEADER_LEN, SPLICE_MAGIC,
};
pub use wire::{
    decode_readdir_request, decode_readdir_response, encode_readdir_error, encode_readdir_page,
    encode_readdir_request, encode_readdir_response, ReadDirPage, ReadDirRequest,
    MAX_READDIR_RESPONSE_BYTES,
};
