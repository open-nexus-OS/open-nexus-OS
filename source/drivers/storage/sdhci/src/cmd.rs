// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A command as the controller issues it — index, argument, the response the card gives —
//! and the response words read back.
//! OWNERS: @runtime @drivers

use crate::regs::{
    CMD_CRC, CMD_DATA, CMD_INDEX, CMD_RESP_136, CMD_RESP_48, CMD_RESP_48_BUSY, CMD_RESP_NONE,
};

/// The response a command expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resp {
    /// None (CMD0).
    None,
    /// 48 bits with index and CRC: the card status.
    R1,
    /// R1, then the card holds DAT0 busy until it is done.
    R1b,
    /// 136 bits with CRC: the CID or the CSD.
    R2,
    /// 48 bits without index or CRC: the OCR.
    R3,
}

impl Resp {
    /// The command register's response-type and check bits.
    pub fn flags(self) -> u32 {
        match self {
            Resp::None => CMD_RESP_NONE,
            Resp::R1 => CMD_RESP_48 | CMD_CRC | CMD_INDEX,
            Resp::R1b => CMD_RESP_48_BUSY | CMD_CRC | CMD_INDEX,
            Resp::R2 => CMD_RESP_136 | CMD_CRC,
            Resp::R3 => CMD_RESP_48,
        }
    }
}

/// A command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    /// The index (0..=63).
    pub index: u8,
    /// The argument.
    pub arg: u32,
    /// The response.
    pub resp: Resp,
}

impl Command {
    /// A command.
    pub const fn new(index: u8, arg: u32, resp: Resp) -> Self {
        Self { index, arg, resp }
    }

    /// The `XFER_CMD` word that issues it: the transfer `mode` in [15:0], the command in
    /// [31:16].
    pub fn word(&self, mode: u32, data: bool) -> u32 {
        let data = if data { CMD_DATA } else { 0 };
        let cmd = (u32::from(self.index & 0x3F) << 8) | self.resp.flags() | data;
        (cmd << 16) | (mode & 0xFFFF)
    }
}

/// The 128-bit CID or CSD from an R2 response's four words (the controller keeps bits
/// [127:8]; the CRC byte reads as zero).
pub fn r2(words: [u32; 4]) -> u128 {
    (u128::from(words[3] & 0x00FF_FFFF) << 104)
        | (u128::from(words[2]) << 72)
        | (u128::from(words[1]) << 40)
        | (u128::from(words[0]) << 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_word_carries_index_response_checks_and_data() {
        // CMD18 (R1, data), multi-block read with DMA and a block count.
        assert_eq!(Command::new(18, 7, Resp::R1).word(0x33, true), 0x123A_0033);
        // CMD6 (R1b, no data): busy response type 3 with CRC and index checks.
        assert_eq!(Command::new(6, 0, Resp::R1b).word(0, false), 0x061B_0000);
        // CMD2 (R2): 136 bits, CRC checked, no index check.
        assert_eq!(Command::new(2, 0, Resp::R2).word(0, false), 0x0209_0000);
        // CMD1 (R3): 48 bits, no checks. CMD0: no response.
        assert_eq!(Command::new(1, 0, Resp::R3).word(0, false), 0x0102_0000);
        assert_eq!(Command::new(0, 0, Resp::None).word(0, false), 0);
    }

    #[test]
    fn an_r2_is_reassembled_with_the_crc_byte_zero() {
        let v = r2([0x4433_2211, 0x8877_6655, 0xCCBB_AA99, 0xFFEE_DDCC]);
        assert_eq!(v >> 104, 0xEE_DDCC, "the top byte of word 3 is not part of the response");
        assert_eq!(v & 0xFF, 0);
        assert_eq!((v >> 8) as u32, 0x4433_2211);
    }
}
