// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The eMMC side (JEDEC eMMC 5.1, the parts this driver speaks): command indices, the OCR
//! handshake, the R1 card status and its states, the CID and CSD fields it reads, the
//! EXT_CSD bytes it reads and switches, and their validation. Everything here decodes what
//! the card sent — untrusted-shape input, checked field by field, never unwrapped.
//! OWNERS: @runtime @drivers

/// Command indices.
pub mod index {
    /// Reset to idle.
    pub const GO_IDLE: u8 = 0;
    /// The power-up handshake (R3).
    pub const SEND_OP_COND: u8 = 1;
    /// Every card's CID (R2).
    pub const ALL_SEND_CID: u8 = 2;
    /// The host assigns the relative address.
    pub const SET_RELATIVE_ADDR: u8 = 3;
    /// Write an EXT_CSD byte (R1b).
    pub const SWITCH: u8 = 6;
    /// Select the card: standby to transfer.
    pub const SELECT_CARD: u8 = 7;
    /// Read the EXT_CSD (512 bytes).
    pub const SEND_EXT_CSD: u8 = 8;
    /// The CSD (R2).
    pub const SEND_CSD: u8 = 9;
    /// Stop a transfer (recovery).
    pub const STOP_TRANSMISSION: u8 = 12;
    /// The card status.
    pub const SEND_STATUS: u8 = 13;
    /// Read blocks (after `SET_BLOCK_COUNT`).
    pub const READ_MULTIPLE_BLOCK: u8 = 18;
    /// The block count of the next read or write.
    pub const SET_BLOCK_COUNT: u8 = 23;
    /// Write blocks (after `SET_BLOCK_COUNT`).
    pub const WRITE_MULTIPLE_BLOCK: u8 = 25;
}

/// OCR: the card finished power-up (the bit reads 0 while it is busy).
pub const OCR_READY: u32 = 1 << 31;
/// OCR: the access-mode field.
pub const OCR_ACCESS_MASK: u32 = 3 << 29;
/// OCR: sector (block) addressing.
pub const OCR_SECTOR: u32 = 2 << 29;
/// OCR: the voltage window the host offers (1.70–1.95 V and 2.7–3.6 V).
pub const OCR_VOLTAGES: u32 = 0x00FF_8080;
/// The CMD1 argument: sector mode and the voltage window.
pub const OCR_ARG: u32 = OCR_SECTOR | OCR_VOLTAGES;

/// The R1 card status.
pub mod status {
    /// The address or count is outside the card.
    pub const OUT_OF_RANGE: u32 = 1 << 31;
    /// A misaligned address.
    pub const ADDRESS_ERROR: u32 = 1 << 30;
    /// A block length the card does not allow.
    pub const BLOCK_LEN_ERROR: u32 = 1 << 29;
    /// An erase sequence error.
    pub const ERASE_SEQ_ERROR: u32 = 1 << 28;
    /// An invalid erase selection.
    pub const ERASE_PARAM: u32 = 1 << 27;
    /// A write to a protected block.
    pub const WP_VIOLATION: u32 = 1 << 26;
    /// A lock/unlock failure.
    pub const LOCK_UNLOCK_FAILED: u32 = 1 << 24;
    /// The previous command failed its CRC.
    pub const COM_CRC_ERROR: u32 = 1 << 23;
    /// The command is not legal in this state.
    pub const ILLEGAL_COMMAND: u32 = 1 << 22;
    /// The internal ECC could not correct the data.
    pub const CARD_ECC_FAILED: u32 = 1 << 21;
    /// An internal controller error.
    pub const CC_ERROR: u32 = 1 << 20;
    /// A general error.
    pub const ERROR: u32 = 1 << 19;
    /// The CID or CSD could not be overwritten.
    pub const CID_CSD_OVERWRITE: u32 = 1 << 16;
    /// An erase skipped protected blocks.
    pub const WP_ERASE_SKIP: u32 = 1 << 15;
    /// An erase was reset.
    pub const ERASE_RESET: u32 = 1 << 13;
    /// The card can take data.
    pub const READY_FOR_DATA: u32 = 1 << 8;
    /// A `SWITCH` the card refused.
    pub const SWITCH_ERROR: u32 = 1 << 7;
    /// Every error bit.
    pub const ERRORS: u32 = OUT_OF_RANGE
        | ADDRESS_ERROR
        | BLOCK_LEN_ERROR
        | ERASE_SEQ_ERROR
        | ERASE_PARAM
        | WP_VIOLATION
        | LOCK_UNLOCK_FAILED
        | COM_CRC_ERROR
        | ILLEGAL_COMMAND
        | CARD_ECC_FAILED
        | CC_ERROR
        | ERROR
        | CID_CSD_OVERWRITE
        | WP_ERASE_SKIP
        | ERASE_RESET
        | SWITCH_ERROR;

    /// The state the card was in when it took the command (`CURRENT_STATE`, [12:9]).
    pub fn state(status: u32) -> u8 {
        ((status >> 9) & 0xF) as u8
    }
}

/// Card states (`CURRENT_STATE`).
pub mod state {
    /// Identification.
    pub const IDENT: u8 = 2;
    /// Standby.
    pub const STANDBY: u8 = 3;
    /// Transfer: ready for the next data command.
    pub const TRANSFER: u8 = 4;
}

/// The CID fields a log names the card by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cid {
    /// Manufacturer id.
    pub manfid: u8,
    /// OEM id.
    pub oemid: u8,
    /// Product name (six ASCII bytes).
    pub name: [u8; 6],
    /// Product serial number.
    pub serial: u32,
}

impl Cid {
    /// From the 128-bit R2 value (`cmd::r2`).
    pub fn from_r2(v: u128) -> Self {
        let byte = |bit: u32| (v >> bit) as u8;
        Self {
            manfid: byte(120),
            oemid: byte(104),
            name: [byte(96), byte(88), byte(80), byte(72), byte(64), byte(56)],
            serial: (v >> 16) as u32,
        }
    }
}

/// The CSD's version fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Csd {
    /// `SPEC_VERS` [125:122]: 4 and above has an EXT_CSD.
    pub spec_vers: u8,
}

impl Csd {
    /// From the 128-bit R2 value.
    pub fn from_r2(v: u128) -> Self {
        Self { spec_vers: ((v >> 122) & 0xF) as u8 }
    }
}

/// EXT_CSD byte offsets.
pub mod ext {
    /// Flush the volatile cache (write 1).
    pub const FLUSH_CACHE: u8 = 32;
    /// The volatile cache is on (bit 0).
    pub const CACHE_CTRL: u8 = 33;
    /// The bus width and mode.
    pub const BUS_WIDTH: u8 = 183;
    /// Enhanced strobe is supported.
    pub const STROBE_SUPPORT: usize = 184;
    /// The timing interface.
    pub const HS_TIMING: u8 = 185;
    /// The EXT_CSD revision.
    pub const REV: usize = 192;
    /// The device types (speed modes) the card supports.
    pub const CARD_TYPE: usize = 196;
    /// The sector count, little endian, four bytes.
    pub const SEC_COUNT: usize = 212;
    /// The longest `SWITCH`, in 10 ms units (0: the card names none).
    pub const GENERIC_CMD6_TIME: usize = 248;

    /// `BUS_WIDTH`: 1-bit SDR.
    pub const WIDTH_1: u8 = 0;
    /// `BUS_WIDTH`: 4-bit SDR.
    pub const WIDTH_4: u8 = 1;
    /// `BUS_WIDTH`: 8-bit SDR.
    pub const WIDTH_8: u8 = 2;
    /// `BUS_WIDTH`: 8-bit DDR.
    pub const WIDTH_8_DDR: u8 = 6;
    /// `BUS_WIDTH`: the enhanced strobe (with DDR).
    pub const STROBE: u8 = 1 << 7;
    /// `HS_TIMING`: high speed.
    pub const TIMING_HS: u8 = 1;
    /// `HS_TIMING`: HS400.
    pub const TIMING_HS400: u8 = 3;

    /// `CARD_TYPE`: high speed at 26 MHz.
    pub const TYPE_HS26: u8 = 1 << 0;
    /// `CARD_TYPE`: high speed at 52 MHz.
    pub const TYPE_HS52: u8 = 1 << 1;
    /// `CARD_TYPE`: HS400 at 1.8 V.
    pub const TYPE_HS400_1V8: u8 = 1 << 6;
}

/// Why an EXT_CSD is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtCsdError {
    /// Not 512 bytes.
    Length,
    /// A sector count of zero.
    ZeroCapacity,
    /// A sector count a sector-addressed card cannot have (2 GiB or less).
    CapacityOutOfRange,
}

/// A `SWITCH` without a time from the card (EXT_CSD before revision 6).
pub const DEFAULT_SWITCH_US: u64 = 500_000;

/// The smallest sector count a sector-addressed card has (above 2 GiB).
pub const MIN_SECTORS: u32 = 0x40_0000;

/// What the driver reads from the EXT_CSD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtCsd {
    /// The revision (8 = eMMC 5.1).
    pub rev: u8,
    /// The speed modes the card supports.
    pub card_type: u8,
    /// Enhanced strobe is supported.
    pub strobe: bool,
    /// The capacity in 512-byte sectors.
    pub sectors: u32,
    /// The longest `SWITCH` busy the card may hold.
    pub switch_us: u64,
    /// The volatile cache is on.
    pub cache_on: bool,
}

impl ExtCsd {
    /// Decode and validate the 512 bytes a sector-addressed card sent.
    pub fn parse(raw: &[u8]) -> Result<Self, ExtCsdError> {
        let raw: &[u8; 512] = raw.try_into().map_err(|_| ExtCsdError::Length)?;
        let s = ext::SEC_COUNT;
        let sectors = u32::from_le_bytes([raw[s], raw[s + 1], raw[s + 2], raw[s + 3]]);
        if sectors == 0 {
            return Err(ExtCsdError::ZeroCapacity);
        }
        if sectors <= MIN_SECTORS {
            return Err(ExtCsdError::CapacityOutOfRange);
        }
        let cmd6 = u64::from(raw[ext::GENERIC_CMD6_TIME]);
        Ok(Self {
            rev: raw[ext::REV],
            card_type: raw[ext::CARD_TYPE],
            strobe: raw[ext::STROBE_SUPPORT] & 1 != 0,
            sectors,
            switch_us: if cmd6 == 0 { DEFAULT_SWITCH_US } else { cmd6 * 10_000 },
            cache_on: raw[usize::from(ext::CACHE_CTRL)] & 1 != 0,
        })
    }
}

/// The `SWITCH` argument that writes `value` into EXT_CSD byte `index`.
pub fn switch_arg(index: u8, value: u8) -> u32 {
    (3 << 24) | (u32::from(index) << 16) | (u32::from(value) << 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The board's eMMC as the stock kernel read it (2026-09-24 measurement).
    fn measured() -> [u8; 512] {
        let hex = include_str!(
            "../../../../../docs/board/measurements/2026-09-24-emmc-sdhci/ext_csd.hex"
        );
        let digits: Vec<u8> = hex.bytes().filter(u8::is_ascii_hexdigit).collect();
        let mut raw = [0u8; 512];
        for (i, pair) in digits.chunks_exact(2).take(512).enumerate() {
            let text = core::str::from_utf8(pair).unwrap();
            raw[i] = u8::from_str_radix(text, 16).unwrap();
        }
        raw
    }

    #[test]
    fn the_boards_card_decodes_to_its_measured_facts() {
        let ext = ExtCsd::parse(&measured()).unwrap();
        assert_eq!(ext.sectors, 30_535_680);
        assert_eq!(ext.rev, 8);
        assert_eq!(ext.card_type, 0x57);
        assert!(ext.strobe);
        assert_eq!(ext.switch_us, 100_000);
        assert!(ext.cache_on, "the stock kernel had switched the cache on");
        assert!(ext.card_type & ext::TYPE_HS400_1V8 != 0 && ext.card_type & ext::TYPE_HS52 != 0);
    }

    #[test]
    fn test_reject_ext_csd_with_zero_or_out_of_range_sector_count() {
        let mut raw = measured();
        raw[ext::SEC_COUNT..ext::SEC_COUNT + 4].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(ExtCsd::parse(&raw), Err(ExtCsdError::ZeroCapacity));
        raw[ext::SEC_COUNT..ext::SEC_COUNT + 4].copy_from_slice(&MIN_SECTORS.to_le_bytes());
        assert_eq!(ExtCsd::parse(&raw), Err(ExtCsdError::CapacityOutOfRange));
        raw[ext::SEC_COUNT..ext::SEC_COUNT + 4].copy_from_slice(&(MIN_SECTORS + 1).to_le_bytes());
        assert_eq!(ExtCsd::parse(&raw).map(|e| e.sectors), Ok(MIN_SECTORS + 1));
        assert_eq!(ExtCsd::parse(&raw[..511]), Err(ExtCsdError::Length));
    }

    #[test]
    fn a_card_that_names_no_switch_time_gets_the_default() {
        let mut raw = measured();
        raw[ext::GENERIC_CMD6_TIME] = 0;
        assert_eq!(ExtCsd::parse(&raw).unwrap().switch_us, DEFAULT_SWITCH_US);
    }

    #[test]
    fn cid_csd_status_and_switch_fields_sit_where_jedec_puts_them() {
        // MID 0x15, OID 0x0100 (its low byte 0x00), PNM "AJTD4R", PSN 0x1234_5678.
        let mut v: u128 = 0x15 << 120;
        for (i, b) in b"AJTD4R".iter().enumerate() {
            v |= u128::from(*b) << (96 - 8 * i as u32);
        }
        v |= 0x1234_5678u128 << 16;
        let cid = Cid::from_r2(v);
        assert_eq!(
            (cid.manfid, cid.oemid, &cid.name, cid.serial),
            (0x15, 0, b"AJTD4R", 0x1234_5678)
        );
        assert_eq!(Csd::from_r2((4u128 << 122) | (3u128 << 126)).spec_vers, 4);
        assert_eq!(
            status::state((u32::from(state::TRANSFER) << 9) | status::READY_FOR_DATA),
            state::TRANSFER
        );
        assert_eq!(switch_arg(ext::HS_TIMING, ext::TIMING_HS), 0x03B9_0100);
        assert_eq!(OCR_ARG, 0x40FF_8080);
    }
}
