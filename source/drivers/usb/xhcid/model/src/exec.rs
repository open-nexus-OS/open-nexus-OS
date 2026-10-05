// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the model controller does with commands and transfers.

use nexus_usb::{SetupPacket, Speed};
use xhcid::trb::{self, Trb};

use crate::ctrl::{Ep, EpState, SlotState};
use crate::dev::{Dev, Reply};
use crate::Machine;

const SUCCESS: u8 = 1;
const BABBLE: u8 = 3;
const TRANSACTION: u8 = 4;
const TRB_ERROR: u8 = 5;
const STALL: u8 = 6;
const NO_SLOTS: u8 = 9;
const SLOT_NOT_ENABLED: u8 = 11;
const SHORT: u8 = 13;
const PARAMETER: u8 = 17;
const CONTEXT_STATE: u8 = 19;

fn device_ref(ports: &[crate::ctrl::RootPort], root: u8, route: u32) -> Option<&Dev> {
    let mut dev = ports.get(usize::from(root).checked_sub(1)?)?.device.as_ref()?;
    for tier in 0..5 {
        let port = ((route >> (4 * tier)) & 0xf) as usize;
        if port == 0 {
            break;
        }
        let Dev::Hub(hub) = dev else { return None };
        let p = hub.ports.get(port - 1)?;
        if !p.enabled {
            return None;
        }
        dev = p.device.as_deref()?;
    }
    Some(dev)
}

fn device_mut(ports: &mut [crate::ctrl::RootPort], root: u8, route: u32) -> Option<&mut Dev> {
    let mut dev = ports.get_mut(usize::from(root).checked_sub(1)?)?.device.as_mut()?;
    for tier in 0..5 {
        let port = ((route >> (4 * tier)) & 0xf) as usize;
        if port == 0 {
            break;
        }
        let Dev::Hub(hub) = dev else { return None };
        let p = hub.ports.get_mut(port - 1)?;
        if !p.enabled {
            return None;
        }
        dev = p.device.as_deref_mut()?;
    }
    Some(dev)
}

fn transfer_event(at: u64, cc: u8, residual: u32, slot: u8, dci: u8) -> Trb {
    Trb {
        param: at,
        status: (u32::from(cc) << 24) | (residual & 0x00ff_ffff),
        control: (u32::from(trb::TRANSFER_EVENT) << 10)
            | (u32::from(dci) << 16)
            | (u32::from(slot) << 24),
    }
}

impl Machine {
    /// Execute one command: (completion code, slot).
    pub(crate) fn execute(&mut self, command: Trb) -> (u8, u8) {
        let slot = command.slot_id();
        let s = usize::from(slot);
        let enabled = self.hc.slots.get(s).is_some_and(Option::is_some);
        match command.kind() {
            trb::ENABLE_SLOT => {
                let max = ((self.hc.config & 0xff) as usize).min(self.hc.slots.len() - 1);
                match (1..=max).find(|&i| self.hc.slots[i].is_none()) {
                    Some(i) => {
                        self.hc.slots[i] = Some(SlotState::default());
                        (SUCCESS, i as u8)
                    }
                    None => (NO_SLOTS, 0),
                }
            }
            _ if !enabled => (SLOT_NOT_ENABLED, slot),
            trb::DISABLE_SLOT => {
                self.hc.slots[s] = None;
                (SUCCESS, slot)
            }
            trb::ADDRESS_DEVICE => (self.address_device(command.param, s), slot),
            trb::EVALUATE_CONTEXT => (self.evaluate(command.param, s), slot),
            trb::CONFIGURE_ENDPOINT => (self.configure(command.param, s), slot),
            trb::RESET_ENDPOINT => {
                let ep = &mut self.slot_mut(s).eps[usize::from(command.endpoint_id())];
                if ep.state != EpState::Halted {
                    return (CONTEXT_STATE, slot);
                }
                ep.state = EpState::Stopped;
                (SUCCESS, slot)
            }
            trb::SET_TR_DEQUEUE => {
                let ep = &mut self.slot_mut(s).eps[usize::from(command.endpoint_id())];
                if ep.state == EpState::Running || ep.state == EpState::Halted {
                    return (CONTEXT_STATE, slot);
                }
                (ep.dequeue, ep.dcs) = (command.param & !0xf, command.param & 1 != 0);
                (SUCCESS, slot)
            }
            _ => (TRB_ERROR, slot),
        }
    }

    fn slot_mut(&mut self, s: usize) -> &mut SlotState {
        self.hc.slots[s].as_mut().expect("an enabled slot")
    }

    /// The (TT hub slot, TT port) a full/low-speed device at `route` needs: the nearest
    /// high-speed hub above it.
    fn tt_of(&self, root: u8, route: u32) -> (u8, u8) {
        let Some(target) = device_ref(&self.ports, root, route) else { return (0, 0) };
        if !matches!(target.speed(), Speed::Full | Speed::Low) {
            return (0, 0);
        }
        let mut tt = (0, 0);
        let mut prefix = 0u32;
        for tier in 0..5 {
            let port = (route >> (4 * tier)) & 0xf;
            if port == 0 {
                break;
            }
            if let Some(Dev::Hub(h)) = device_ref(&self.ports, root, prefix) {
                if h.speed == Speed::High {
                    let hub_slot = self.hc.slots.iter().position(|s| {
                        s.as_ref().is_some_and(|s| {
                            s.addressed && s.root_port == root && s.route == prefix
                        })
                    });
                    tt = (hub_slot.unwrap_or(0) as u8, port as u8);
                }
            }
            prefix |= port << (4 * tier);
        }
        tt
    }

    fn address_device(&mut self, input: u64, s: usize) -> u8 {
        let csz = self.hc.cfg.context_size as u64;
        let control = self.read_words(input, 2);
        if control[1] & 0b11 != 0b11 {
            return PARAMETER;
        }
        let sl = self.read_words(input + csz, 4);
        let e = self.read_words(input + 2 * csz, 5);
        let (route, speed) = (sl[0] & 0xf_ffff, ((sl[0] >> 20) & 0xf) as u8);
        let root = ((sl[1] >> 16) & 0xff) as u8;
        let tt = ((sl[2] & 0xff) as u8, ((sl[2] >> 8) & 0xff) as u8);
        // The device context the DCBAA names must be in memory.
        if self.read_u64(self.hc.dcbaap + 8 * s as u64).unwrap_or(0) == 0 {
            return CONTEXT_STATE;
        }
        let expected_tt = self.tt_of(root, route);
        let Some(dev) = device_ref(&self.ports, root, route) else { return TRANSACTION };
        let dev_speed = match dev.speed() {
            Speed::Full => 1,
            Speed::Low => 2,
            Speed::High => 3,
            Speed::Super => 4,
        };
        if dev_speed != speed || expected_tt != tt {
            return TRANSACTION;
        }
        let ep0 = Ep {
            address: 0,
            kind: ((e[1] >> 3) & 7) as u8,
            max_packet: (e[1] >> 16) as u16,
            interval: 0,
            dequeue: (u64::from(e[2]) | (u64::from(e[3]) << 32)) & !0xf,
            dcs: e[2] & 1 != 0,
            state: EpState::Running,
        };
        if ep0.kind != 4 || ep0.dequeue == 0 {
            return PARAMETER;
        }
        let st = self.slot_mut(s);
        (st.addressed, st.root_port, st.route, st.speed_id, st.tt) = (true, root, route, speed, tt);
        st.eps[1] = ep0;
        SUCCESS
    }

    fn evaluate(&mut self, input: u64, s: usize) -> u8 {
        let csz = self.hc.cfg.context_size as u64;
        let control = self.read_words(input, 2);
        if control[1] & 0b10 == 0 {
            return PARAMETER;
        }
        let e = self.read_words(input + 2 * csz, 2);
        self.slot_mut(s).eps[1].max_packet = (e[1] >> 16) as u16;
        SUCCESS
    }

    fn configure(&mut self, input: u64, s: usize) -> u8 {
        let csz = self.hc.cfg.context_size as u64;
        let add = self.read_words(input, 2)[1];
        let (root, route) = {
            let st = self.slot_mut(s);
            (st.root_port, st.route)
        };
        let endpoints =
            device_ref(&self.ports, root, route).map(Dev::endpoints).unwrap_or_default();
        let speed = device_ref(&self.ports, root, route).map(Dev::speed);
        if add & 1 != 0 {
            let sl = self.read_words(input + csz, 4);
            let st = self.slot_mut(s);
            st.hub = (sl[0] >> 26) & 1 != 0;
            st.hub_ports = (sl[1] >> 24) as u8;
            st.ttt = ((sl[2] >> 16) & 3) as u8;
        }
        for dci in 2..32u64 {
            if add & (1 << dci) == 0 {
                continue;
            }
            let e = self.read_words(input + (dci + 1) * csz, 5);
            let address = (dci / 2) as u8 | if dci % 2 == 1 { 0x80 } else { 0 };
            let max_packet = (e[1] >> 16) as u16;
            let kind = ((e[1] >> 3) & 7) as u8;
            let interval = (e[0] >> 16) as u8;
            let Some(&(_, mp, b_interval)) = endpoints.iter().find(|ep| ep.0 == address) else {
                return PARAMETER;
            };
            // The service interval the controller schedules by: full/low speed counts frames
            // (the largest power of two not above bInterval, 2^3..2^10 microframes), high speed
            // carries the exponent plus one (xHCI 1.2 §6.2.3.6).
            let expected = match speed {
                Some(Speed::Full | Speed::Low) => (3 + b_interval.max(1).ilog2() as u8).min(10),
                _ => b_interval.clamp(1, 16) - 1,
            };
            if mp != max_packet || (address & 0x80 != 0 && kind != 7) || interval != expected {
                return PARAMETER;
            }
            self.slot_mut(s).eps[dci as usize] = Ep {
                address,
                kind,
                max_packet,
                interval: (e[0] >> 16) as u8,
                dequeue: (u64::from(e[2]) | (u64::from(e[3]) << 32)) & !0xf,
                dcs: e[2] & 1 != 0,
                state: EpState::Running,
            };
        }
        SUCCESS
    }

    pub(crate) fn ring_endpoint(&mut self, slot: u8, target: u8) {
        let (s, dci) = (usize::from(slot), usize::from(target));
        let Some(Some(st)) = self.hc.slots.get_mut(s) else { return };
        let Some(ep) = st.eps.get_mut(dci) else { return };
        match ep.state {
            EpState::Halted | EpState::Disabled => return,
            EpState::Stopped => ep.state = EpState::Running,
            EpState::Running => {}
        }
        if dci == 1 {
            self.run_control(s);
        } else {
            self.service();
        }
    }

    fn run_control(&mut self, s: usize) {
        for _ in 0..64 {
            let (root, route, ep) = {
                let st = self.slot_mut(s);
                (st.root_port, st.route, st.eps[1])
            };
            if ep.state != EpState::Running {
                return;
            }
            let (mut at, mut dcs) = (ep.dequeue, ep.dcs);
            let mut td = Vec::new();
            loop {
                let Some(t) = self.read_trb(at) else { return };
                if t.cycle() != dcs {
                    return;
                }
                if t.kind() == trb::LINK {
                    at = t.param & !0xf;
                    if t.control & 2 != 0 {
                        dcs = !dcs;
                    }
                    continue;
                }
                td.push((at, t));
                at += 16;
                if t.kind() == trb::STATUS || td.len() > 3 {
                    break;
                }
            }
            let (_, setup) = td[0];
            assert_eq!(setup.kind(), trb::SETUP, "a control TD starts with its setup stage");
            let packet = SetupPacket::from_bytes(setup.param.to_le_bytes());
            let data = td.iter().copied().find(|(_, t)| t.kind() == trb::DATA);
            let (status_at, status) = td[td.len() - 1];
            assert_eq!(status.kind(), trb::STATUS, "a control TD ends with its status stage");
            let slot = s as u8;
            // A data stage longer than one packet at the wrong EP0 max packet: the device sends
            // packets the controller did not size for — babble, and EP0 halts.
            let data_len = data.map_or(0, |(_, d)| (d.status & 0x1_ffff) as u16);
            let device_mp = device_ref(&self.ports, root, route).map(Dev::max_packet0);
            if data_len > 8 && device_mp.is_some_and(|mp| mp != ep.max_packet) {
                let (at_babble, _) = data.unwrap_or((status_at, status));
                self.slot_mut(s).eps[1].state = EpState::Halted;
                self.event(transfer_event(at_babble, BABBLE, 0, slot, 1));
                return;
            }
            let reply = match device_mut(&mut self.ports, root, route) {
                Some(dev) => dev.control(packet),
                None => Reply::Stall,
            };
            match reply {
                Reply::Stall => {
                    let at_stall = data.map_or(status_at, |(a, _)| a);
                    self.slot_mut(s).eps[1].state = EpState::Halted;
                    self.event(transfer_event(at_stall, STALL, 0, slot, 1));
                    return;
                }
                Reply::Data(bytes) => {
                    if let Some((data_at, d)) = data {
                        let len = (d.status & 0x1_ffff) as usize;
                        let n = len.min(bytes.len());
                        self.mem.write(d.param, &bytes[..n]).expect("the data buffer is in memory");
                        if n < len {
                            self.event(transfer_event(data_at, SHORT, (len - n) as u32, slot, 1));
                        }
                    }
                }
                Reply::Done => {}
            }
            let st = self.slot_mut(s);
            (st.eps[1].dequeue, st.eps[1].dcs) = (at, dcs);
            self.event(transfer_event(status_at, SUCCESS, 0, slot, 1));
        }
    }

    /// Deliver every waiting report to an armed interrupt TRB.
    pub fn service(&mut self) {
        for _ in 0..4096 {
            let mut progressed = false;
            for s in 1..self.hc.slots.len() {
                for dci in 2..32 {
                    progressed |= self.deliver(s, dci);
                }
            }
            if !progressed {
                return;
            }
        }
    }

    fn deliver(&mut self, s: usize, dci: usize) -> bool {
        let Some(Some(st)) = self.hc.slots.get(s) else { return false };
        let (root, route, ep) = (st.root_port, st.route, st.eps[dci]);
        if ep.state != EpState::Running || ep.kind != 7 {
            return false;
        }
        let (mut at, mut dcs) = (ep.dequeue, ep.dcs);
        let normal = loop {
            let Some(t) = self.read_trb(at) else { return false };
            if t.cycle() != dcs {
                return false;
            }
            if t.kind() == trb::LINK {
                at = t.param & !0xf;
                if t.control & 2 != 0 {
                    dcs = !dcs;
                }
                continue;
            }
            break t;
        };
        if normal.kind() != trb::NORMAL {
            return false;
        }
        let Some(report) =
            device_mut(&mut self.ports, root, route).and_then(|d| d.take_report(ep.address))
        else {
            return false;
        };
        let len = (normal.status & 0x1_ffff) as usize;
        let slot = s as u8;
        if report.len() > len {
            self.slot_mut(s).eps[dci].state = EpState::Halted;
            self.event(transfer_event(at, BABBLE, 0, slot, dci as u8));
            return true;
        }
        self.mem.write(normal.param, &report).expect("the report buffer is in memory");
        let (cc, residual) =
            if report.len() == len { (SUCCESS, 0) } else { (SHORT, (len - report.len()) as u32) };
        let ep = &mut self.slot_mut(s).eps[dci];
        (ep.dequeue, ep.dcs) = (at + 16, dcs);
        self.event(transfer_event(at, cc, residual, slot, dci as u8));
        true
    }

    /// Queue a report on the device at `route` below root port `root`, endpoint `address`.
    pub fn report(&mut self, root: u8, route: u32, address: u8, bytes: &[u8]) {
        if let Some(Dev::Hid(h)) = device_mut(&mut self.ports, root, route) {
            h.reports.push_back((address, bytes.to_vec()));
        }
        self.service();
    }

    /// The hub at `route` below root port `root`.
    pub fn hub_at(&mut self, root: u8, route: u32) -> Option<&mut crate::dev::Hub> {
        match device_mut(&mut self.ports, root, route)? {
            Dev::Hub(h) => Some(h),
            Dev::Hid(_) => None,
        }
    }
}
