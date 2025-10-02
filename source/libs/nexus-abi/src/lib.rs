#![cfg_attr(not(test), no_std)]

/// IPC message header shared between kernel and userland.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsgHeader {
    pub dst: u32,
    pub len: u16,
    pub flags: u16,
}

impl MsgHeader {
    pub const fn new(dst: u32, len: u16, flags: u16) -> Self {
        Self { dst, len, flags }
    }

    pub fn serialize(&self) -> [u8; 8] {
        let mut buf = [0_u8; 8];
        buf[0..4].copy_from_slice(&self.dst.to_le_bytes());
        buf[4..6].copy_from_slice(&self.len.to_le_bytes());
        buf[6..8].copy_from_slice(&self.flags.to_le_bytes());
        buf
    }

    pub fn deserialize(bytes: [u8; 8]) -> Self {
        let dst = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let len = u16::from_le_bytes([bytes[4], bytes[5]]);
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        Self { dst, len, flags }
    }
}

#[cfg(test)]
mod tests {
    use super::MsgHeader;

    #[test]
    fn round_trip() {
        let header = MsgHeader::new(42, 16, 3);
        let bytes = header.serialize();
        assert_eq!(header, MsgHeader::deserialize(bytes));
    }
}
