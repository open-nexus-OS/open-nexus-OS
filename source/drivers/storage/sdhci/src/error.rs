// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What can go wrong, named: every bounded wait has a [`Stage`], every refusal of what the
//! card or the controller reported carries the evidence (the status bits, the state).
//! OWNERS: @runtime @drivers

use crate::adma::AdmaError;
use crate::proto::ExtCsdError;

/// The bounded wait that ran out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// A software reset bit that did not clear.
    Reset,
    /// The internal clock (or its PLL) that did not report stable.
    ClockStable,
    /// The command or data line that stayed busy before a command.
    Inhibit,
    /// A command that did not complete.
    Command,
    /// A data transfer that did not complete.
    Data,
    /// A card that held DAT0 busy past the switch or program time.
    Busy,
    /// A card that did not finish power-up (CMD1) within the eMMC's second.
    OpCond,
    /// The K1 PHY DLL that did not lock.
    DllLock,
    /// A card that did not return to the transfer state after an error.
    Recovery,
}

/// Why a driver call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A bounded wait ran out.
    Timeout(Stage),
    /// The controller raised error interrupts during command `cmd` (`status` = the error
    /// status bits, `INT_STATUS[31:16]`).
    Controller {
        /// The command index.
        cmd: u8,
        /// The error status bits.
        status: u32,
    },
    /// The ADMA engine stopped during command `cmd` (`status` = `ADMA_ERROR`).
    Adma {
        /// The command index.
        cmd: u8,
        /// The ADMA error status.
        status: u32,
    },
    /// The card's status after command `cmd` carries error bits.
    CardStatus {
        /// The command index.
        cmd: u8,
        /// The R1 card status.
        status: u32,
    },
    /// The card was not in the state the protocol needs when it took command `cmd`.
    CardState {
        /// The command index.
        cmd: u8,
        /// The state it reported.
        state: u8,
        /// The state the protocol needs.
        want: u8,
    },
    /// A transfer ended with blocks left (the controller's residual block count).
    ShortTransfer {
        /// Blocks not moved.
        remaining: u16,
    },
    /// The card is byte-addressed (2 GiB or less): only sector-addressed cards are driven.
    ByteAddressed,
    /// The card's OCR shares no voltage with the window the host offered.
    Voltage,
    /// The card's CSD names a specification without EXT_CSD (before eMMC 4.0).
    CardTooOld,
    /// The EXT_CSD is not one this driver can trust.
    ExtCsd(ExtCsdError),
    /// The EXT_CSD read again at the final bus mode differs from the one read at 1 bit: the
    /// wider or faster bus does not carry the data intact.
    BusVerify,
    /// The host's 1.8 V signalling did not stay enabled.
    Signal1v8,
    /// The capability register names no base clock and none was configured.
    NoBaseClock,
    /// The controller cannot do what was asked (no ADMA2, no voltage, no enhanced strobe on
    /// this layer).
    Unsupported,
    /// The divider cannot bring the base clock down to the target.
    Clock,
    /// A configuration value outside its domain (a bus width that is not 1, 4 or 8).
    Config,
    /// A request outside the card, not whole sectors, empty, or larger than one transfer.
    Range,
    /// DMA memory the controller cannot be programmed with.
    DmaMemory(AdmaError),
    /// The DMA buffers do not fit the device (a non-coherent device on harts without Zicbom).
    DmaBuffer,
}
