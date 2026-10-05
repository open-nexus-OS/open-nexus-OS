// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The xHCI register map (xHCI 1.2 §5): the capability registers, the operational registers
//! with the port register sets, interrupter 0 of the runtime registers, the doorbells and the
//! Supported Protocol capabilities — 32-bit words through `nexus_hal::Bus`; a 64-bit register
//! is written low word first.

use nexus_hal::Bus;

/// Capability registers.
const CAPLENGTH: usize = 0x00;
const HCSPARAMS1: usize = 0x04;
const HCSPARAMS2: usize = 0x08;
const HCCPARAMS1: usize = 0x10;
const DBOFF: usize = 0x14;
const RTSOFF: usize = 0x18;

/// Operational registers (from CAPLENGTH).
pub const USBCMD: usize = 0x00;
pub const USBSTS: usize = 0x04;
pub const CRCR: usize = 0x18;
pub const DCBAAP: usize = 0x30;
pub const CONFIG: usize = 0x38;
const PORTSC: usize = 0x400;

pub const CMD_RUN: u32 = 1 << 0;
pub const CMD_HCRST: u32 = 1 << 1;
pub const CMD_INTE: u32 = 1 << 2;

pub const STS_HCH: u32 = 1 << 0;
pub const STS_HSE: u32 = 1 << 2;
pub const STS_EINT: u32 = 1 << 3;
pub const STS_PCD: u32 = 1 << 4;
pub const STS_CNR: u32 = 1 << 11;
pub const STS_HCE: u32 = 1 << 12;

/// Interrupter 0 (from RTSOFF).
const IR0: usize = 0x20;
pub const IMAN: usize = IR0;
pub const IMOD: usize = IR0 + 0x04;
pub const ERSTSZ: usize = IR0 + 0x08;
pub const ERSTBA: usize = IR0 + 0x10;
pub const ERDP: usize = IR0 + 0x18;
pub const IMAN_IP: u32 = 1 << 0;
pub const IMAN_IE: u32 = 1 << 1;
pub const ERDP_EHB: u64 = 1 << 3;

pub const PORT_CCS: u32 = 1 << 0;
pub const PORT_PED: u32 = 1 << 1;
pub const PORT_PR: u32 = 1 << 4;
pub const PORT_PP: u32 = 1 << 9;
pub const PORT_CSC: u32 = 1 << 17;
pub const PORT_PRC: u32 = 1 << 21;
/// CSC, PEC, WRC, OCC, PRC, PLC, CEC — write 1 to clear.
pub const PORT_CHANGES: u32 = 0x7f << 17;
/// Read-only bits and the bits a write must carry over (link state, power, indicators, wake).
const PORT_RO: u32 = PORT_CCS | (1 << 3) | (0xf << 10) | (1 << 30);
const PORT_RWS: u32 = (0xf << 5) | PORT_PP | (0x3 << 14) | (0x7 << 25);

/// A PORTSC value that changes nothing when written: no change bit cleared, the port not
/// disabled (PED is write-1-to-disable), no reset.
#[must_use]
pub const fn port_neutral(portsc: u32) -> u32 {
    portsc & (PORT_RO | PORT_RWS)
}

/// The port speed ID (PORTSC bits 13:10).
#[must_use]
pub const fn port_speed(portsc: u32) -> u8 {
    ((portsc >> 10) & 0xf) as u8
}

/// The most root ports the driver tracks.
pub const MAX_PORTS: usize = 16;
/// Extended capabilities walked before the list counts as malformed.
const MAX_XCAPS: usize = 64;
const XCAP_SUPPORTED_PROTOCOL: u32 = 2;

/// What the capability registers say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// HCIVERSION (BCD).
    pub version: u16,
    /// Where the operational registers start (CAPLENGTH).
    pub op: usize,
    /// Where the runtime registers start (RTSOFF).
    pub rt: usize,
    /// Where the doorbell array starts (DBOFF).
    pub db: usize,
    /// Device slots.
    pub max_slots: u8,
    /// Root ports.
    pub max_ports: u8,
    /// Scratchpad buffers the controller wants.
    pub scratchpads: u16,
    /// Bytes per context (32, or 64 with HCCPARAMS1.CSZ).
    pub context_size: usize,
    /// Ports are powered by software (HCCPARAMS1.PPC).
    pub port_power: bool,
    /// The first extended capability (bytes from the base), 0 for none.
    pub xecp: usize,
}

impl Caps {
    /// The capability registers.
    pub fn read<B: Bus>(bus: &B) -> Self {
        let head = bus.read(CAPLENGTH);
        let sp1 = bus.read(HCSPARAMS1);
        let sp2 = bus.read(HCSPARAMS2);
        let cp1 = bus.read(HCCPARAMS1);
        let scratch_hi = (sp2 >> 21) & 0x1f;
        let scratch_lo = (sp2 >> 27) & 0x1f;
        Self {
            version: (head >> 16) as u16,
            op: (head & 0xff) as usize,
            rt: (bus.read(RTSOFF) & !0x1f) as usize,
            db: (bus.read(DBOFF) & !0x3) as usize,
            max_slots: (sp1 & 0xff) as u8,
            max_ports: (sp1 >> 24) as u8,
            scratchpads: ((scratch_hi << 5) | scratch_lo) as u16,
            context_size: if cp1 & (1 << 2) != 0 { 64 } else { 32 },
            port_power: cp1 & (1 << 3) != 0,
            xecp: (((cp1 >> 16) & 0xffff) as usize) * 4,
        }
    }
}

/// The controller's registers behind a bus.
pub struct Regs<B: Bus> {
    bus: B,
    caps: Caps,
}

impl<B: Bus> Regs<B> {
    /// The registers of the controller at `bus`.
    pub fn new(bus: B) -> Self {
        let caps = Caps::read(&bus);
        Self { bus, caps }
    }

    /// The capability registers as read at [`Regs::new`].
    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    /// An operational register.
    pub fn op(&self, reg: usize) -> u32 {
        self.bus.read(self.caps.op + reg)
    }

    /// Write an operational register.
    pub fn set_op(&self, reg: usize, value: u32) {
        self.bus.write(self.caps.op + reg, value);
    }

    /// Write a 64-bit operational register, low word first.
    pub fn set_op64(&self, reg: usize, value: u64) {
        self.bus.write(self.caps.op + reg, value as u32);
        self.bus.write(self.caps.op + reg + 4, (value >> 32) as u32);
    }

    /// An interrupter-0 register.
    pub fn rt(&self, reg: usize) -> u32 {
        self.bus.read(self.caps.rt + reg)
    }

    /// Write an interrupter-0 register.
    pub fn set_rt(&self, reg: usize, value: u32) {
        self.bus.write(self.caps.rt + reg, value);
    }

    /// Write a 64-bit interrupter-0 register, low word first.
    pub fn set_rt64(&self, reg: usize, value: u64) {
        self.bus.write(self.caps.rt + reg, value as u32);
        self.bus.write(self.caps.rt + reg + 4, (value >> 32) as u32);
    }

    /// PORTSC of root port `port` (1-based).
    pub fn portsc(&self, port: u8) -> u32 {
        self.bus.read(self.caps.op + PORTSC + 0x10 * (usize::from(port) - 1))
    }

    /// Write PORTSC of root port `port` (start from [`port_neutral`]).
    pub fn set_portsc(&self, port: u8, value: u32) {
        self.bus.write(self.caps.op + PORTSC + 0x10 * (usize::from(port) - 1), value);
    }

    /// Ring doorbell `slot` (0 = the command ring) for `target` (an endpoint's DCI).
    pub fn ring(&self, slot: u8, target: u8) {
        self.bus.write(self.caps.db + 4 * usize::from(slot), u32::from(target));
    }

    /// The USB major revision of every root port (index = port − 1), from the Supported
    /// Protocol capabilities; 0 where no capability names the port.
    pub fn port_revisions(&self) -> [u8; MAX_PORTS] {
        let mut out = [0u8; MAX_PORTS];
        let mut at = self.caps.xecp;
        for _ in 0..MAX_XCAPS {
            if at == 0 {
                break;
            }
            let head = self.bus.read(at);
            if head & 0xff == XCAP_SUPPORTED_PROTOCOL {
                let major = (head >> 24) as u8;
                let ports = self.bus.read(at + 8);
                let (first, count) = ((ports & 0xff) as usize, ((ports >> 8) & 0xff) as usize);
                for port in first..first + count {
                    if let Some(slot) = port.checked_sub(1).and_then(|i| out.get_mut(i)) {
                        *slot = major;
                    }
                }
            }
            let next = ((head >> 8) & 0xff) as usize * 4;
            at = if next == 0 { 0 } else { at + next };
        }
        out
    }
}
