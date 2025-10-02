// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! IPC message header definition.

/// IPC header exchanged between tasks.
///
/// The header is exactly 16 bytes and therefore cache-line friendly.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    pub src: u32,
    pub dst: u32,
    pub ty: u16,
    pub flags: u16,
    pub len: u32,
}

impl MessageHeader {
    /// Creates a new header with all fields initialised.
    pub const fn new(src: u32, dst: u32, ty: u16, flags: u16, len: u32) -> Self {
        Self { src, dst, ty, flags, len }
    }
}
