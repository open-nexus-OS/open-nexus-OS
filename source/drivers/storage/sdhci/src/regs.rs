// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The standard SDHCI register map (specification 3.00; the parts this driver uses),
//! accessed only as aligned 32-bit words: some controllers accept nothing narrower, and
//! every narrower register is a field of one word here. A word that holds a
//! write-1-to-clear field (`INT_STATUS`) or a self-clearing one (the reset byte of `CLOCK`)
//! is written with exactly the bits meant.
//! OWNERS: @runtime @drivers

/// Block size [11:0], SDMA boundary [14:12], block count [31:16].
pub const BLOCK: usize = 0x04;
/// The command argument.
pub const ARGUMENT: usize = 0x08;
/// Transfer mode [15:0] and command [31:16]: writing the word issues the command.
pub const XFER_CMD: usize = 0x0C;
/// The response: bits [39:8] in the first word, up to [127:104] in the fourth (R2).
pub const RESPONSE: usize = 0x10;
/// The PIO data port.
pub const DATA_PORT: usize = 0x20;
/// Present state.
pub const PRESENT: usize = 0x24;
/// Host control 1 [7:0], power control [15:8], block gap control [23:16], wakeup [31:24].
pub const HOST_CTRL: usize = 0x28;
/// Clock control [15:0], timeout control [23:16], software reset [31:24].
pub const CLOCK: usize = 0x2C;
/// Normal [15:0] and error [31:16] interrupt status; write 1 to clear.
pub const INT_STATUS: usize = 0x30;
/// Which status bits latch.
pub const INT_ENABLE: usize = 0x34;
/// Which latched bits drive the interrupt line.
pub const INT_SIGNAL: usize = 0x38;
/// Auto-command error status [15:0] (read-only), host control 2 [31:16].
pub const HOST_CTRL2: usize = 0x3C;
/// Capabilities, low word.
pub const CAPS: usize = 0x40;
/// ADMA error status [7:0].
pub const ADMA_ERROR: usize = 0x54;
/// The ADMA descriptor table's address, low word.
pub const ADMA_ADDR: usize = 0x58;
/// The ADMA descriptor table's address, high word (0: 32-bit descriptors).
pub const ADMA_ADDR_HI: usize = 0x5C;
/// Slot interrupt status [15:0], host controller version [31:16] (specification [23:16]).
pub const VERSION: usize = 0xFC;

/// Transfer mode: DMA.
pub const TM_DMA: u32 = 1 << 0;
/// Transfer mode: the block count counts down.
pub const TM_BLOCK_COUNT: u32 = 1 << 1;
/// Transfer mode: card to host.
pub const TM_READ: u32 = 1 << 4;
/// Transfer mode: more than one block.
pub const TM_MULTI: u32 = 1 << 5;

/// Command: no response.
pub const CMD_RESP_NONE: u32 = 0;
/// Command: a 136-bit response.
pub const CMD_RESP_136: u32 = 1;
/// Command: a 48-bit response.
pub const CMD_RESP_48: u32 = 2;
/// Command: a 48-bit response, then busy on DAT0.
pub const CMD_RESP_48_BUSY: u32 = 3;
/// Command: check the response CRC.
pub const CMD_CRC: u32 = 1 << 3;
/// Command: check the response index.
pub const CMD_INDEX: u32 = 1 << 4;
/// Command: data follows on the DAT lines.
pub const CMD_DATA: u32 = 1 << 5;

/// Present state: the command line is busy.
pub const PS_CMD_INHIBIT: u32 = 1 << 0;
/// Present state: the data lines are busy.
pub const PS_DAT_INHIBIT: u32 = 1 << 1;

/// Host control 1: 4-bit bus.
pub const HC1_WIDTH_4: u32 = 1 << 1;
/// Host control 1: high-speed timing.
pub const HC1_HIGH_SPEED: u32 = 1 << 2;
/// Host control 1: the DMA select field.
pub const HC1_DMA_MASK: u32 = 3 << 3;
/// Host control 1: ADMA2 with 32-bit addresses.
pub const HC1_ADMA2_32: u32 = 2 << 3;
/// Host control 1: 8-bit bus.
pub const HC1_WIDTH_8: u32 = 1 << 5;
/// Power control: bus power on.
pub const PWR_ON: u32 = 1 << 8;
/// Power control: the voltage field.
pub const PWR_VDD_MASK: u32 = 7 << 9;
/// Power control: 1.8 V.
pub const PWR_180: u32 = 5 << 9;
/// Power control: 3.0 V.
pub const PWR_300: u32 = 6 << 9;
/// Power control: 3.3 V.
pub const PWR_330: u32 = 7 << 9;

/// Clock control: internal clock enable.
pub const CLK_INT_EN: u32 = 1 << 0;
/// Clock control: internal clock stable.
pub const CLK_INT_STABLE: u32 = 1 << 1;
/// Clock control: the card clock runs.
pub const CLK_CARD_EN: u32 = 1 << 2;
/// Clock control: the PLL runs (specification 4.10 on).
pub const CLK_PLL_EN: u32 = 1 << 3;
/// Timeout control: the field.
pub const TIMEOUT_MASK: u32 = 0xF << 16;
/// Timeout control: the longest data timeout (the driver keeps its own deadlines).
pub const TIMEOUT_MAX: u32 = 0xE << 16;
/// Software reset: everything.
pub const RESET_ALL: u32 = 1 << 24;
/// Software reset: the command line.
pub const RESET_CMD: u32 = 1 << 25;
/// Software reset: the data lines.
pub const RESET_DATA: u32 = 1 << 26;
/// Software reset: the byte.
pub const RESET_MASK: u32 = 0xFF << 24;

/// Interrupt: the command completed.
pub const INT_CMD_COMPLETE: u32 = 1 << 0;
/// Interrupt: the transfer (or the busy after R1b) completed.
pub const INT_XFER_COMPLETE: u32 = 1 << 1;
/// Interrupt: the buffer can take a block (PIO write).
pub const INT_BUF_WRITE: u32 = 1 << 4;
/// Interrupt: the buffer holds a block (PIO read).
pub const INT_BUF_READ: u32 = 1 << 5;
/// Error: the command got no response.
pub const ERR_CMD_TIMEOUT: u32 = 1 << 16;
/// Error: the response failed its CRC.
pub const ERR_CMD_CRC: u32 = 1 << 17;
/// Error: the data did not arrive in time.
pub const ERR_DATA_TIMEOUT: u32 = 1 << 20;
/// Error: the data failed its CRC.
pub const ERR_DATA_CRC: u32 = 1 << 21;
/// Error: the ADMA engine stopped.
pub const ERR_ADMA: u32 = 1 << 25;
/// Every error bit.
pub const ERR_ALL: u32 = 0xFFFF_0000;
/// The status bits the driver latches (and signals on the line when it sleeps on it).
pub const INT_USED: u32 =
    INT_CMD_COMPLETE | INT_XFER_COMPLETE | INT_BUF_WRITE | INT_BUF_READ | ERR_ALL;

/// Host control 2: the UHS mode field.
pub const HC2_UHS_MASK: u32 = 7 << 16;
/// Host control 2: HS400.
pub const HC2_UHS_HS400: u32 = 5 << 16;
/// Host control 2: 1.8 V signalling.
pub const HC2_1V8: u32 = 1 << 19;

/// Capabilities: the base clock field starts here (MHz; 8 bits from 3.00, 6 before).
pub const CAP_BASE_CLOCK_SHIFT: u32 = 8;
/// Capabilities: an 8-bit bus.
pub const CAP_8_BIT: u32 = 1 << 18;
/// Capabilities: ADMA2.
pub const CAP_ADMA2: u32 = 1 << 19;
/// Capabilities: 3.3 V.
pub const CAP_330: u32 = 1 << 24;
/// Capabilities: 3.0 V.
pub const CAP_300: u32 = 1 << 25;
/// Capabilities: 1.8 V.
pub const CAP_180: u32 = 1 << 26;

/// Specification 3.00 (`VERSION[23:16]` = 2): the 10-bit divider, host control 2.
pub const SPEC_300: u8 = 2;
/// Specification 4.10: the clock's PLL enable.
pub const SPEC_410: u8 = 4;
