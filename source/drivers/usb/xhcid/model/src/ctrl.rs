// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The model controller: the registers (capability, operational with the port sets, interrupter
//! 0, doorbells, Supported Protocol capabilities), the command ring (Enable/Disable Slot,
//! Address Device, Evaluate Context, Configure Endpoint, Reset Endpoint, Set TR Dequeue
//! Pointer), control and interrupt transfers, and the event ring — every structure read from
//! and written to the DRAM model by bus address, so a driver that forgot a write-back hands it
//! stale memory and the test shows it. It checks what real hardware would refuse: an
//! unpublished DCBAA entry, a speed or TT field that does not match the device, an endpoint
//! the device does not have, a full event ring.

use nexus_usb::Speed;
use xhcid::trb::{self, Trb};

use crate::dev::Dev;
use crate::Machine;

pub const RTSOFF: usize = 0x600;
pub const DBOFF: usize = 0x800;
pub const XECP: usize = 0xa00;
const OP: usize = 0x20;
const PORTS: usize = OP + 0x400;
const IR0: usize = RTSOFF + 0x20;

const HCH: u32 = 1;
const HSE: u32 = 1 << 2;
const EINT: u32 = 1 << 3;
const CNR: u32 = 1 << 11;
const RUN: u32 = 1;
const HCRST: u32 = 1 << 1;
const INTE: u32 = 1 << 2;
const IP: u32 = 1;
const IE: u32 = 2;

/// The controller's shape.
#[derive(Clone, Debug)]
pub struct Config {
    pub version: u16,
    /// Each root port's USB major revision.
    pub revisions: Vec<u8>,
    pub context_size: usize,
    pub scratchpads: u16,
    pub port_power: bool,
    pub slots: u8,
}

impl Config {
    /// QEMU's `qemu-xhci`: 32-byte contexts, no scratchpad, four USB 2 and four USB 3 ports.
    #[must_use]
    pub fn qemu() -> Self {
        Self {
            version: 0x0100,
            revisions: vec![2, 2, 2, 2, 3, 3, 3, 3],
            context_size: 32,
            scratchpads: 0,
            port_power: false,
            slots: 64,
        }
    }

    /// The board's controller as measured: xHCI 1.10, 64-byte contexts, one scratchpad, port
    /// power control, a USB 2 and a USB 3 root port.
    #[must_use]
    pub fn board() -> Self {
        Self {
            version: 0x0110,
            revisions: vec![2, 3],
            context_size: 64,
            scratchpads: 1,
            port_power: true,
            slots: 64,
        }
    }
}

/// Faults a test injects.
#[derive(Clone, Debug, Default)]
pub struct Faults {
    /// USBSTS reads that still show CNR.
    pub not_ready_reads: u32,
    /// USBCMD reads that still show HCRST after a reset.
    pub reset_reads: u32,
}

/// A root port.
#[derive(Clone, Debug, Default)]
pub struct RootPort {
    pub revision: u8,
    pub device: Option<Dev>,
    pub ccs: bool,
    pub ped: bool,
    pub pp: bool,
    pub csc: bool,
    pub prc: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EpState {
    #[default]
    Disabled,
    Running,
    Halted,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ep {
    pub address: u8,
    pub kind: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub dequeue: u64,
    pub dcs: bool,
    pub state: EpState,
}

#[derive(Clone, Debug, Default)]
pub struct SlotState {
    pub addressed: bool,
    pub root_port: u8,
    pub route: u32,
    pub speed_id: u8,
    pub tt: (u8, u8),
    pub hub: bool,
    pub hub_ports: u8,
    pub ttt: u8,
    pub eps: [Ep; 32],
}

/// The controller's state.
#[derive(Debug)]
pub struct Hc {
    pub cfg: Config,
    pub usbcmd: u32,
    pub usbsts: u32,
    pub config: u32,
    pub dcbaap: u64,
    cmd_dequeue: u64,
    cmd_ccs: bool,
    pub iman: u32,
    pub imod: u32,
    pub erstsz: u32,
    pub erstba: u64,
    pub erdp: u64,
    ev_base: u64,
    ev_size: u32,
    ev_enqueue: u32,
    ev_pcs: bool,
    pub slots: Vec<Option<SlotState>>,
    pub faults: Faults,
    /// Commands executed, by TRB type, with their completion code.
    pub commands: Vec<(u8, u8)>,
    /// Register reads and writes.
    pub accesses: u64,
}

impl Hc {
    pub fn new(cfg: Config) -> Self {
        let slots = vec![None; usize::from(cfg.slots) + 1];
        Self {
            cfg,
            usbcmd: 0,
            usbsts: HCH,
            config: 0,
            dcbaap: 0,
            cmd_dequeue: 0,
            cmd_ccs: false,
            iman: 0,
            imod: 0,
            erstsz: 0,
            erstba: 0,
            erdp: 0,
            ev_base: 0,
            ev_size: 0,
            ev_enqueue: 0,
            ev_pcs: true,
            slots,
            faults: Faults::default(),
            commands: Vec::new(),
            accesses: 0,
        }
    }

    fn running(&self) -> bool {
        self.usbcmd & RUN != 0
    }
}

fn speed_id(speed: Speed) -> u8 {
    match speed {
        Speed::Full => 1,
        Speed::Low => 2,
        Speed::High => 3,
        Speed::Super => 4,
    }
}

impl Machine {
    /// A register read.
    pub fn read(&mut self, addr: usize) -> u32 {
        self.hc.accesses += 1;
        let hc = &mut self.hc;
        let ports = self.ports.len();
        match addr {
            0x00 => 0x20 | (u32::from(hc.cfg.version) << 16),
            0x04 => u32::from(hc.cfg.slots) | (1 << 8) | ((ports as u32) << 24),
            0x08 => {
                let n = u32::from(hc.cfg.scratchpads);
                ((n & 0x1f) << 27) | (((n >> 5) & 0x1f) << 21) | (0xf << 4)
            }
            0x0c => 0,
            0x10 => {
                1 | (u32::from(hc.cfg.context_size == 64) << 2)
                    | (u32::from(hc.cfg.port_power) << 3)
                    | (((XECP / 4) as u32) << 16)
            }
            0x14 => DBOFF as u32,
            0x18 => RTSOFF as u32,
            a if a == OP => {
                if hc.usbcmd & HCRST != 0 {
                    if hc.faults.reset_reads == 0 {
                        hc.usbcmd &= !HCRST;
                    } else {
                        hc.faults.reset_reads -= 1;
                    }
                }
                hc.usbcmd
            }
            a if a == OP + 0x04 => {
                if hc.faults.not_ready_reads > 0 {
                    hc.faults.not_ready_reads -= 1;
                    return hc.usbsts | CNR;
                }
                hc.usbsts
            }
            a if a == OP + 0x08 => 1,
            a if a == OP + 0x38 => hc.config,
            a if (PORTS..PORTS + 0x10 * ports).contains(&a) && (a - PORTS) % 0x10 == 0 => {
                self.portsc((a - PORTS) / 0x10)
            }
            a if a == IR0 => hc.iman,
            a if a == IR0 + 0x04 => hc.imod,
            a if a == IR0 + 0x08 => hc.erstsz,
            a if (XECP..XECP + 0x100).contains(&a) => self.xcap(a - XECP),
            _ => 0,
        }
    }

    fn xcap(&self, offset: usize) -> u32 {
        // One Supported Protocol capability per run of ports with the same revision.
        let mut runs: Vec<(u8, usize, usize)> = Vec::new();
        for (i, p) in self.ports.iter().enumerate() {
            match runs.last_mut() {
                Some((rev, _, count)) if *rev == p.revision => *count += 1,
                _ => runs.push((p.revision, i + 1, 1)),
            }
        }
        let (cap, word) = (offset / 0x10, (offset % 0x10) / 4);
        let Some(&(rev, first, count)) = runs.get(cap) else { return 0 };
        let next = if cap + 1 < runs.len() { 4 } else { 0 };
        match word {
            0 => 2 | (next << 8) | (u32::from(rev) << 24),
            1 => u32::from_le_bytes(*b"USB "),
            2 => first as u32 | ((count as u32) << 8),
            _ => 0,
        }
    }

    fn portsc(&self, i: usize) -> u32 {
        let p = &self.ports[i];
        let speed = p.device.as_ref().filter(|_| p.ccs).map_or(0, |d| speed_id(d.speed()));
        u32::from(p.ccs)
            | (u32::from(p.ped) << 1)
            | (u32::from(p.pp) << 9)
            | (u32::from(speed) << 10)
            | (u32::from(p.csc) << 17)
            | (u32::from(p.prc) << 21)
    }

    /// A register write.
    pub fn write(&mut self, addr: usize, value: u32) {
        self.hc.accesses += 1;
        let ports = self.ports.len();
        match addr {
            a if a == OP => self.write_usbcmd(value),
            a if a == OP + 0x04 => self.hc.usbsts &= !(value & (EINT | HSE | (1 << 4))),
            a if a == OP + 0x18 => {
                self.hc.cmd_dequeue =
                    (self.hc.cmd_dequeue & !0xffff_ffff) | u64::from(value & !0x3f);
                self.hc.cmd_ccs = value & 1 != 0;
            }
            a if a == OP + 0x1c => {
                self.hc.cmd_dequeue =
                    (self.hc.cmd_dequeue & 0xffff_ffff) | (u64::from(value) << 32);
            }
            a if a == OP + 0x30 => {
                self.hc.dcbaap = (self.hc.dcbaap & !0xffff_ffff) | u64::from(value)
            }
            a if a == OP + 0x34 => {
                self.hc.dcbaap = (self.hc.dcbaap & 0xffff_ffff) | (u64::from(value) << 32);
            }
            a if a == OP + 0x38 => self.hc.config = value,
            a if (PORTS..PORTS + 0x10 * ports).contains(&a) && (a - PORTS) % 0x10 == 0 => {
                self.write_portsc((a - PORTS) / 0x10, value);
            }
            a if a == IR0 => {
                if value & IP != 0 {
                    self.hc.iman &= !IP;
                }
                self.hc.iman = (self.hc.iman & !IE) | (value & IE);
            }
            a if a == IR0 + 0x04 => self.hc.imod = value,
            a if a == IR0 + 0x08 => self.hc.erstsz = value,
            a if a == IR0 + 0x10 => {
                self.hc.erstba = (self.hc.erstba & !0xffff_ffff) | u64::from(value)
            }
            a if a == IR0 + 0x14 => {
                self.hc.erstba = (self.hc.erstba & 0xffff_ffff) | (u64::from(value) << 32);
                self.load_erst();
            }
            a if a == IR0 + 0x18 => {
                self.hc.erdp = (self.hc.erdp & !0xffff_ffff) | u64::from(value & !0xf)
            }
            a if a == IR0 + 0x1c => {
                self.hc.erdp = (self.hc.erdp & 0xffff_ffff) | (u64::from(value) << 32);
            }
            a if (DBOFF..DBOFF + 4 * 256).contains(&a) => {
                let slot = ((a - DBOFF) / 4) as u8;
                if slot == 0 {
                    self.run_commands();
                } else {
                    self.ring_endpoint(slot, value as u8);
                }
            }
            _ => {}
        }
    }

    fn write_usbcmd(&mut self, value: u32) {
        if value & HCRST != 0 {
            let cfg = self.hc.cfg.clone();
            let faults = self.hc.faults.clone();
            let commands = std::mem::take(&mut self.hc.commands);
            let accesses = self.hc.accesses;
            self.hc = Hc::new(cfg);
            self.hc.faults = faults;
            self.hc.commands = commands;
            self.hc.accesses = accesses;
            self.hc.usbcmd = HCRST;
            let powered = !self.hc.cfg.port_power;
            for p in &mut self.ports {
                p.pp = powered;
                p.ccs = powered && p.device.is_some();
                p.csc = p.ccs;
                p.ped = false;
                p.prc = false;
            }
            return;
        }
        self.hc.usbcmd = value;
        if value & RUN != 0 {
            self.hc.usbsts &= !HCH;
            // The scratchpad array the driver handed over must be in memory.
            for i in 0..u64::from(self.hc.cfg.scratchpads) {
                let array = self.read_u64(self.hc.dcbaap).unwrap_or(0);
                let entry = self.read_u64(array + 8 * i).unwrap_or(0);
                assert_ne!(entry, 0, "scratchpad {i} not handed to the controller");
            }
        } else {
            self.hc.usbsts |= HCH;
        }
    }

    fn write_portsc(&mut self, i: usize, value: u32) {
        let running = self.hc.running();
        let p = &mut self.ports[i];
        if value & (1 << 17) != 0 {
            p.csc = false;
        }
        if value & (1 << 21) != 0 {
            p.prc = false;
        }
        let mut changed = false;
        if value & (1 << 9) != 0 && !p.pp {
            p.pp = true;
            if p.device.is_some() {
                (p.ccs, p.csc, changed) = (true, true, true);
            }
        }
        if value & (1 << 1) != 0 {
            p.ped = false;
        }
        if value & (1 << 4) != 0 && p.ccs {
            (p.ped, p.prc, changed) = (true, true, true);
        }
        if changed && running {
            self.port_event(i);
        }
    }

    fn port_event(&mut self, i: usize) {
        let event = Trb {
            param: ((i as u64 + 1) & 0xff) << 24,
            status: u32::from(trb::CC_SUCCESS) << 24,
            control: u32::from(trb::PORT_STATUS_CHANGE) << 10,
        };
        self.event(event);
    }

    /// Plug `device` into root port `port`.
    pub fn plug(&mut self, port: u8, device: Dev) {
        let i = usize::from(port) - 1;
        let p = &mut self.ports[i];
        p.device = Some(device);
        if p.pp {
            (p.ccs, p.csc) = (true, true);
            if self.hc.running() {
                self.port_event(i);
            }
        }
    }

    fn load_erst(&mut self) {
        let base = self.read_u64(self.hc.erstba).unwrap_or(0);
        let mut size = [0u8; 4];
        let _ = self.mem.read(self.hc.erstba + 8, &mut size);
        self.hc.ev_base = base;
        self.hc.ev_size = u32::from_le_bytes(size) & 0xffff;
        self.hc.ev_enqueue = 0;
        self.hc.ev_pcs = true;
    }

    pub(crate) fn read_u64(&self, bus: u64) -> Option<u64> {
        let mut b = [0u8; 8];
        self.mem.read(bus, &mut b)?;
        Some(u64::from_le_bytes(b))
    }

    pub(crate) fn read_trb(&self, bus: u64) -> Option<Trb> {
        let mut b = [0u8; 16];
        self.mem.read(bus, &mut b)?;
        Some(Trb::from_bytes(&b))
    }

    pub(crate) fn read_words(&self, bus: u64, n: usize) -> Vec<u32> {
        let mut b = vec![0u8; 4 * n];
        let _ = self.mem.read(bus, &mut b);
        b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    }

    /// Write an event; the line rises.
    pub(crate) fn event(&mut self, mut event: Trb) {
        let hc = &mut self.hc;
        assert!(hc.ev_size > 0 && hc.running(), "an event before the event ring runs");
        let dequeue = ((hc.erdp & !0xf).wrapping_sub(hc.ev_base) / 16) as u32;
        assert_ne!(
            (hc.ev_enqueue + 1) % hc.ev_size,
            dequeue,
            "event ring full (ERDP not advanced)"
        );
        event.control = (event.control & !1) | u32::from(hc.ev_pcs);
        let at = hc.ev_base + 16 * u64::from(hc.ev_enqueue);
        hc.ev_enqueue += 1;
        if hc.ev_enqueue == hc.ev_size {
            hc.ev_enqueue = 0;
            hc.ev_pcs = !hc.ev_pcs;
        }
        hc.iman |= IP;
        hc.usbsts |= EINT;
        self.mem.write(at, &event.to_bytes()).expect("the event ring is in memory");
    }

    /// The interrupt line.
    #[must_use]
    pub fn irq(&self) -> bool {
        self.hc.iman & IP != 0 && self.hc.iman & IE != 0 && self.hc.usbcmd & INTE != 0
    }

    fn run_commands(&mut self) {
        for _ in 0..256 {
            let at = self.hc.cmd_dequeue;
            let Some(command) = self.read_trb(at) else { return };
            if command.cycle() != self.hc.cmd_ccs {
                return;
            }
            if command.kind() == trb::LINK {
                self.hc.cmd_dequeue = command.param & !0xf;
                if command.control & 2 != 0 {
                    self.hc.cmd_ccs = !self.hc.cmd_ccs;
                }
                continue;
            }
            let (cc, slot) = self.execute(command);
            self.hc.commands.push((command.kind(), cc));
            self.hc.cmd_dequeue = at + 16;
            self.event(Trb {
                param: at,
                status: u32::from(cc) << 24,
                control: (u32::from(trb::COMMAND_COMPLETION) << 10) | (u32::from(slot) << 24),
            });
        }
    }
}
