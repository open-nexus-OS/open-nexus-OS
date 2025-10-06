// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! IPC message header definition.

use core::convert::TryInto;

/// IPC header exchanged between tasks.
///
/// The header is exactly 16 bytes and therefore cache-line friendly.
#[repr(C, align(4))]
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

    /// Serialises the header to a little-endian byte array suitable for golden vector comparisons.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn to_le_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&self.src.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.dst.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.ty.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.flags.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.len.to_le_bytes());
        bytes
    }

    /// Deserialises a little-endian byte array into a [`MessageHeader`].
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_le_bytes(bytes: [u8; 16]) -> Self {
        let src = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let dst = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let ty = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        let flags = u16::from_le_bytes(bytes[10..12].try_into().unwrap());
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        Self { src, dst, ty, flags, len }
    }
}

#[cfg(test)]
mod tests {
    use super::MessageHeader;
    use core::mem::{align_of, size_of};

    // Inline golden vector (LE):
    // src=0x01020304, dst=0x11223344, ty=0x5566, flags=0x7788, len=0x99aabbcc
    const VECTOR: [u8; 16] = [
        0x04, 0x03, 0x02, 0x01,
        0x44, 0x33, 0x22, 0x11,
        0x66, 0x55,
        0x88, 0x77,
        0xCC, 0xBB, 0xAA, 0x99,
    ];

    #[test]
    fn header_layout() {
        assert_eq!(size_of::<MessageHeader>(), 16);
        assert_eq!(align_of::<MessageHeader>(), 4);
    }

    #[test]
    fn golden_vector_roundtrip() {
        let header = MessageHeader::new(0x0102_0304, 0x1122_3344, 0x5566, 0x7788, 0x99aa_bbcc);
        assert_eq!(
            header.to_le_bytes(),
            VECTOR,
            "golden vector out of date; expected bytes: {:02x?}",
            header.to_le_bytes()
        );

        let mut raw = [0u8; 16];
        raw.copy_from_slice(&VECTOR);
        let decoded = MessageHeader::from_le_bytes(raw);
        assert_eq!(decoded, header);
    }
}
