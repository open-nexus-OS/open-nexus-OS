// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Enumeration (RFC-0099 §4): one port at a time is reset and addressed (two devices never
//! answer the default address together); then each device walks its own stages, advanced by
//! command completions and control transfers — slot, address, EP0's max packet from the first
//! eight bytes, the device and configuration descriptors (bounded by `nexus-usb`), the pipes
//! it will use, SET_CONFIGURATION, and then the hub or the HID set-up. A device that goes away
//! keeps its memory until Disable Slot completes.

use nexus_hal::Bus;
use nexus_usb::descriptor::max_packet0;
use nexus_usb::{Configuration, DeviceDescriptor, PortFeature, RouteString, SetupPacket, Speed};

use crate::command::Issuer;
use crate::context::{
    dci, input_control, interval_exponent, speed_id, Endpoint, Slot, EP_CONTROL, EP_INTERRUPT_IN,
};
use crate::controller::{Active, PortRef, Xhci, DCBAA};
use crate::device::{
    Device, HubWork, Pipe, PipeKind, Stage, CONTROL_BUF, CORE_LEN, EP0_RING, EP0_TRBS, IN_CONTEXT,
    MAX_PIPES, OUT_CONTEXT, PIPE_RING_TRBS, PIPE_TRBS,
};
use crate::memory::{DmaAlloc, Region};
use crate::regs::{port_neutral, PORT_CCS, PORT_PR};
use crate::ring::{Producer, TRB_LEN};
use crate::sink::{Note, Sink, Step};
use crate::trb::{Trb, CC_SUCCESS};

/// A root port's reset (the controller drives it).
const ROOT_RESET_NS: u64 = 500_000_000;
/// A hub port's reset, re-read every `RESET_POLL_NS` (a hub's change pipe may report only every
/// 256 ms — measured on the board's hub).
const HUB_RESET_NS: u64 = 500_000_000;
const RESET_POLL_NS: u64 = 10_000_000;
/// USB 2.0 reset recovery before the device is addressed.
const RECOVERY_NS: u64 = 10_000_000;
/// The hub descriptor is requested with room for 15 ports.
const HUB_DESCRIPTOR_LEN: u16 = 16;

/// What an input context is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    Address,
    MaxPacket,
    Configure,
    HubSlot,
}

/// A pipe a configuration asks for.
#[derive(Clone, Copy, Debug)]
struct Plan {
    kind: PipeKind,
    endpoint: u8,
    max_packet: u16,
    interval: u8,
}

impl<B: Bus, A: DmaAlloc> Xhci<B, A> {
    /// Start the next due port task: a root port's reset is the controller's, a hub port's the
    /// hub's (re-read until it finishes).
    pub(crate) fn next_task(&mut self, now: u64) {
        while let Some(port) = self.pop_task(now) {
            match port {
                PortRef::Root(p) => {
                    let sc = self.regs.portsc(p);
                    if sc & PORT_CCS == 0 {
                        continue;
                    }
                    self.regs.set_portsc(p, port_neutral(sc) | PORT_PR);
                    let give_up = now + ROOT_RESET_NS;
                    self.active = Some(Active::Resetting { port, give_up, next: give_up });
                    return;
                }
                PortRef::Hub { hub, port: p } => {
                    if !self.hub_push(hub, HubWork::Set(p, PortFeature::Reset)) {
                        continue;
                    }
                    let (give_up, next) = (now + HUB_RESET_NS, now + RESET_POLL_NS);
                    self.active = Some(Active::Resetting { port, give_up, next });
                    self.hub_work(hub);
                    return;
                }
            }
        }
    }

    /// The reset/recovery deadlines of the active port.
    pub(crate) fn active_timer(&mut self, now: u64, sink: &mut impl Sink) {
        match self.active {
            Some(Active::Resetting { port, give_up, next }) if next <= now => {
                if now >= give_up {
                    sink.note(Note::Fail { step: Step::PortReset, code: 0 });
                    self.active = None;
                    return;
                }
                if let PortRef::Hub { hub, port: p } = port {
                    self.hub_push(hub, HubWork::Status(p));
                    self.active =
                        Some(Active::Resetting { port, give_up, next: now + RESET_POLL_NS });
                    self.hub_work(hub);
                }
            }
            Some(Active::Recovering { port, speed, until }) if until <= now => {
                self.active = None;
                self.give_slot(port, speed, sink);
            }
            _ => {}
        }
    }

    /// `port` finished its reset with a device at `speed`.
    pub(crate) fn port_reset_done(&mut self, port: PortRef, speed: Speed, now: u64) {
        if matches!(self.active, Some(Active::Resetting { port: p, .. }) if p == port) {
            self.active = Some(Active::Recovering { port, speed, until: now + RECOVERY_NS });
        }
    }

    /// A table entry and a slot for the device on `port`.
    fn give_slot(&mut self, port: PortRef, speed: Speed, sink: &mut impl Sink) {
        let placed = match port {
            PortRef::Root(p) => Some((RouteString::ROOT, p, None, None)),
            PortRef::Hub { hub, port: p } => self.devices[hub].as_ref().and_then(|h| {
                let route = h.route.child(p).ok()?;
                let tt = match (speed.needs_tt(), h.speed) {
                    (false, _) => None,
                    (true, Speed::High) => Some((h.slot, p)),
                    (true, _) => h.tt,
                };
                Some((route, h.root_port, Some((hub, p)), tt))
            }),
        };
        let Some((route, root_port, parent, tt)) = placed else {
            return sink.note(Note::Fail { step: Step::Devices, code: 0 });
        };
        let Some(idx) = self.devices.iter().position(Option::is_none) else {
            return sink.note(Note::Fail { step: Step::Devices, code: 0 });
        };
        let Some(core) = self.alloc.shared(CORE_LEN).and_then(Region::new) else {
            return sink.note(Note::Fail { step: Step::Memory, code: 0 });
        };
        self.devices[idx] = Some(Device {
            slot: 0,
            route,
            root_port,
            speed,
            parent,
            tt,
            stage: Stage::EnableSlot,
            core,
            pipes_mem: None,
            ep0: Producer::new(EP0_RING, EP0_TRBS),
            max_packet0: speed.default_max_packet0(),
            control: None,
            vendor: 0,
            product: 0,
            class: 0,
            config_value: 0,
            pipes: [None; MAX_PIPES],
            hub: None,
            recovery: None,
            disabling: false,
        });
        match parent {
            None => self.root[usize::from(root_port) - 1] = Some(idx),
            Some((hub, p)) => {
                if let Some(h) = self.devices[hub].as_mut().and_then(|d| d.hub.as_mut()) {
                    h.children[usize::from(p) - 1] = Some(idx);
                }
            }
        }
        self.active = Some(Active::Addressing { device: idx });
        self.queue.push(Trb::enable_slot(), Issuer::Device(idx));
    }

    /// A command the device issued completed (`cc`), or timed out.
    pub(crate) fn device_command_done(
        &mut self,
        idx: usize,
        cc: u8,
        slot: u8,
        now: u64,
        sink: &mut impl Sink,
    ) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        if dev.stage == Stage::Detaching {
            return self.detached_command_done(idx);
        }
        if dev.recovery.is_some() {
            return self.recovery_done(idx, now, sink);
        }
        match dev.stage {
            Stage::EnableSlot => {
                if cc != CC_SUCCESS || slot == 0 {
                    return self.give_up(idx, Step::EnableSlot, cc, sink);
                }
                dev.slot = slot;
                let out = dev.core.bus(OUT_CONTEXT);
                if let Some(ctrl) = self.ctrl.as_mut() {
                    ctrl.put(DCBAA + 8 * usize::from(slot), &out.to_le_bytes());
                }
                self.command_with_input(idx, Input::Address, Stage::Address);
            }
            Stage::Address => {
                if matches!(self.active, Some(Active::Addressing { device }) if device == idx) {
                    self.active = None;
                }
                if cc != CC_SUCCESS {
                    return self.give_up(idx, Step::Address, cc, sink);
                }
                self.request(idx, Stage::Prefix, SetupPacket::device_descriptor(8));
            }
            Stage::Evaluate => {
                if cc != CC_SUCCESS {
                    return self.give_up(idx, Step::Configure, cc, sink);
                }
                self.request(idx, Stage::Device, SetupPacket::device_descriptor(18));
            }
            Stage::Configure => {
                if cc != CC_SUCCESS {
                    return self.give_up(idx, Step::Configure, cc, sink);
                }
                let value = dev.config_value;
                self.request(idx, Stage::SetConfiguration, SetupPacket::set_configuration(value));
            }
            Stage::HubSlot => {
                if cc != CC_SUCCESS {
                    return self.give_up(idx, Step::Configure, cc, sink);
                }
                let power = SetupPacket::hub_set_port_feature(1, PortFeature::Power);
                self.request(idx, Stage::HubPower(1), power);
            }
            _ => {}
        }
    }

    /// A control transfer the device's stages issued finished: `Ok(bytes)` or the completion
    /// code that halted EP0 (already recovered).
    pub(crate) fn device_control_done(
        &mut self,
        idx: usize,
        result: Result<usize, u8>,
        now: u64,
        sink: &mut impl Sink,
    ) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let stage = dev.stage;
        let n = match (stage, result) {
            // SET_IDLE is best effort (a measured mouse STALLs it); a hub's work reports its own.
            (Stage::SetIdle(i), _) => return self.hid_idle_done(idx, i, sink),
            (Stage::Running, _) => return self.hub_done(idx, result, now, sink),
            (Stage::HubPower(p), _) => return self.hub_power_done(idx, p, now),
            (Stage::SetProtocol(i), Err(cc)) => {
                // The interface cannot report in the boot protocol: it is not used.
                sink.note(Note::Fail { step: Step::Control, code: cc });
                dev.pipes[usize::from(i)] = None;
                return self.next_hid(idx, i + 1);
            }
            (_, Err(cc)) => return self.give_up(idx, Step::Control, cc, sink),
            (_, Ok(n)) => n,
        };
        let data = dev.core.mem.bytes().get(CONTROL_BUF..CONTROL_BUF + n).unwrap_or(&[]);
        match stage {
            Stage::Prefix => match max_packet0(data) {
                Ok(mp) if mp != dev.max_packet0 => {
                    dev.max_packet0 = mp;
                    self.command_with_input(idx, Input::MaxPacket, Stage::Evaluate);
                }
                Ok(_) => self.request(idx, Stage::Device, SetupPacket::device_descriptor(18)),
                Err(e) => self.give_up(idx, Step::Descriptor, e as u8, sink),
            },
            Stage::Device => match DeviceDescriptor::parse(data) {
                Ok(d) => {
                    (dev.vendor, dev.product, dev.class) = (d.vendor, d.product, d.class);
                    let header = SetupPacket::configuration_descriptor(9);
                    self.request(idx, Stage::ConfigHeader, header);
                }
                Err(e) => self.give_up(idx, Step::Descriptor, e as u8, sink),
            },
            Stage::ConfigHeader => match Configuration::total_length(data) {
                Ok(total) => {
                    let whole = SetupPacket::configuration_descriptor(total as u16);
                    self.request(idx, Stage::Config, whole);
                }
                Err(e) => self.give_up(idx, Step::Descriptor, e as u8, sink),
            },
            Stage::Config => self.configuration(idx, n, sink),
            Stage::SetConfiguration => {
                if dev.hub.is_some()
                    || matches!(dev.pipes[0], Some(p) if p.kind == PipeKind::HubStatus)
                {
                    let descriptor = SetupPacket::hub_descriptor(HUB_DESCRIPTOR_LEN);
                    return self.request(idx, Stage::HubDescriptor, descriptor);
                }
                self.next_hid(idx, 0);
            }
            Stage::SetProtocol(i) => {
                let Some(Pipe { kind: PipeKind::Hid { interface, .. }, .. }) =
                    dev.pipes[usize::from(i)]
                else {
                    return self.next_hid(idx, i + 1);
                };
                self.request(idx, Stage::SetIdle(i), SetupPacket::hid_set_idle_infinite(interface));
            }
            Stage::HubDescriptor => match nexus_usb::HubDescriptor::parse(data) {
                Ok(h) => {
                    let ttt = if dev.speed == Speed::High { h.tt_think_time() } else { 0 };
                    dev.hub = Some(crate::device::Hub::new(h.ports, ttt, h.power_good_ms));
                    self.command_with_input(idx, Input::HubSlot, Stage::HubSlot);
                }
                Err(e) => self.give_up(idx, Step::Descriptor, e as u8, sink),
            },
            _ => {}
        }
    }

    /// The configuration read — the `received` bytes the data stage moved, never the buffer
    /// behind them: choose the pipes, size their memory, Configure Endpoint.
    fn configuration(&mut self, idx: usize, received: usize, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let data = dev.core.mem.bytes().get(CONTROL_BUF..CONTROL_BUF + received).unwrap_or(&[]);
        let (value, class, plans) = match plan(data, dev.class) {
            Ok(p) => p,
            Err(code) => return self.give_up(idx, Step::Descriptor, code, sink),
        };
        dev.config_value = value;
        sink.note(Note::Enumerated {
            slot: dev.slot,
            route: dev.route.raw(),
            speed: dev.speed,
            vendor: dev.vendor,
            product: dev.product,
            class,
        });
        if plans.iter().all(Option::is_none) {
            dev.stage = Stage::Unsupported;
            return;
        }
        let stride_of = |p: &Plan| usize::from(p.max_packet).next_multiple_of(64).max(64);
        let rings = MAX_PIPES * PIPE_RING_TRBS * TRB_LEN;
        let buffers: usize = plans.iter().flatten().map(|p| PIPE_TRBS * stride_of(p)).sum();
        let len = (rings + buffers).next_power_of_two().max(4096);
        let Some(mem) = self.alloc.shared(len).and_then(Region::new) else {
            return self.give_up(idx, Step::Memory, 0, sink);
        };
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let mut at = rings;
        for (i, plan) in plans.iter().enumerate() {
            dev.pipes[i] = plan.map(|p| {
                let stride = stride_of(&p);
                let buffers = at;
                at += PIPE_TRBS * stride;
                Pipe {
                    kind: p.kind,
                    endpoint: p.endpoint,
                    dci: dci(p.endpoint),
                    max_packet: p.max_packet,
                    interval: p.interval,
                    ring: Producer::new(i * PIPE_RING_TRBS * TRB_LEN, PIPE_RING_TRBS),
                    buffers,
                    stride,
                    queued: [(0, 0); PIPE_TRBS],
                    head: 0,
                    next_buffer: 0,
                    errors: 0,
                }
            });
        }
        dev.pipes_mem = Some(mem);
        self.command_with_input(idx, Input::Configure, Stage::Configure);
    }

    /// The next HID pipe from `from` on: SET_PROTOCOL(boot) — or, with none left, running.
    pub(crate) fn next_hid(&mut self, idx: usize, from: u8) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let next = (usize::from(from)..MAX_PIPES).find_map(|i| match dev.pipes[i] {
            Some(Pipe { kind: PipeKind::Hid { interface, .. }, .. }) => Some((i as u8, interface)),
            _ => None,
        });
        match next {
            Some((i, interface)) => {
                let boot = SetupPacket::hid_set_protocol(interface, nexus_usb::HidProtocol::Boot);
                self.request(idx, Stage::SetProtocol(i), boot);
            }
            None => dev.stage = Stage::Running,
        }
    }

    /// SET_IDLE on pipe `i` finished (either way): its reports start.
    fn hid_idle_done(&mut self, idx: usize, i: u8, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_ref() else { return };
        if let Some(Pipe {
            kind: PipeKind::Hid { interface, role },
            endpoint,
            max_packet,
            interval,
            ..
        }) = dev.pipes[usize::from(i)]
        {
            let slot = dev.slot;
            self.start_pipe(idx, usize::from(i));
            sink.note(Note::HidInterface { slot, interface, role, endpoint, max_packet, interval });
        }
        self.next_hid(idx, i + 1);
    }

    /// Issue a control request and move to `stage`.
    pub(crate) fn request(&mut self, idx: usize, stage: Stage, setup: SetupPacket) {
        if let Some(dev) = self.devices[idx].as_mut() {
            dev.stage = stage;
            self.control(idx, setup);
        }
    }

    /// Build the input context for `what`, move to `stage`, queue its command.
    fn command_with_input(&mut self, idx: usize, what: Input, stage: Stage) {
        let layout = self.layout;
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let mut buf = [0u8; 33 * 64];
        let input = &mut buf[..layout.input_len()];
        let mut slot = Slot {
            route: dev.route.raw(),
            speed_id: speed_id(dev.speed),
            entries: 1,
            root_port: dev.root_port,
            ..Slot::default()
        };
        if let Some((tt_slot, tt_port)) = dev.tt {
            (slot.tt_slot, slot.tt_port) = (tt_slot, tt_port);
        }
        if let (Input::HubSlot, Some(hub)) = (what, dev.hub) {
            (slot.hub, slot.ports, slot.ttt) = (true, hub.ports, hub.ttt);
        }
        let ep0 = Endpoint {
            kind: EP_CONTROL,
            max_packet: dev.max_packet0,
            dequeue: dev.ep0.start(&dev.core),
            average_trb: 8,
            ..Endpoint::default()
        };
        let add = match what {
            Input::Address => 0b11,
            Input::MaxPacket => 0b10,
            Input::Configure | Input::HubSlot => {
                let mut add = 1u32;
                for pipe in dev.pipes.iter().flatten() {
                    slot.entries = slot.entries.max(pipe.dci);
                    if what == Input::Configure {
                        add |= 1 << pipe.dci;
                    }
                    let Some(mem) = dev.pipes_mem.as_ref() else { continue };
                    let ep = Endpoint {
                        kind: EP_INTERRUPT_IN,
                        max_packet: pipe.max_packet,
                        interval: interval_exponent(dev.speed, pipe.interval),
                        dequeue: pipe.ring.start(mem),
                        average_trb: pipe.max_packet,
                        max_esit: pipe.max_packet,
                    };
                    let at = layout.input(pipe.dci);
                    ep.write(&mut input[at..at + layout.size]);
                }
                add
            }
        };
        input_control(&mut input[..layout.size], 0, add);
        slot.write(&mut input[layout.input(0)..layout.input(0) + layout.size]);
        ep0.write(&mut input[layout.input(1)..layout.input(1) + layout.size]);
        dev.core.put(IN_CONTEXT, input);
        let address = dev.core.bus(IN_CONTEXT);
        let id = dev.slot;
        dev.stage = stage;
        let command = match what {
            Input::Address => Trb::address_device(address, id),
            Input::MaxPacket => Trb::evaluate_context(address, id),
            Input::Configure | Input::HubSlot => Trb::configure_endpoint(address, id),
        };
        self.queue.push(command, Issuer::Device(idx));
    }

    /// Enumeration of `idx` gave up at `step`.
    pub(crate) fn give_up(&mut self, idx: usize, step: Step, code: u8, sink: &mut impl Sink) {
        if matches!(self.active, Some(Active::Addressing { device }) if device == idx) {
            self.active = None;
        }
        if let Some(dev) = self.devices[idx].as_mut() {
            dev.stage = Stage::Failed;
        }
        sink.note(Note::Fail { step, code });
    }

    /// The device (and every device behind it, for a hub) went away.
    pub(crate) fn detach(&mut self, idx: usize, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        if dev.stage == Stage::Detaching {
            return;
        }
        dev.stage = Stage::Detaching;
        dev.control = None;
        dev.recovery = None;
        let (slot, children) = (dev.slot, dev.hub.map(|h| h.children));
        if let Some((hub, p)) = dev.parent {
            if let Some(h) = self.devices[hub].as_mut().and_then(|d| d.hub.as_mut()) {
                h.children[usize::from(p) - 1] = None;
            }
        }
        if matches!(self.active, Some(Active::Addressing { device }) if device == idx) {
            self.active = None;
        }
        for child in children.into_iter().flatten().flatten() {
            self.detach(child, sink);
        }
        if slot != 0 {
            sink.note(Note::Detached { slot });
        }
        if !self.queue.forget(Issuer::Device(idx)) {
            self.disable(idx);
        }
    }

    /// A detached device's command completed: the in-flight one it had, or its Disable Slot.
    fn detached_command_done(&mut self, idx: usize) {
        let Some(dev) = self.devices[idx].as_ref() else { return };
        if dev.disabling {
            if let Some(ctrl) = self.ctrl.as_mut() {
                ctrl.put(DCBAA + 8 * usize::from(dev.slot), &0u64.to_le_bytes());
            }
            self.devices[idx] = None;
            return;
        }
        self.disable(idx);
    }

    /// Disable the slot of a detached device (or free it, if it never got one).
    fn disable(&mut self, idx: usize) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        if dev.slot == 0 {
            self.devices[idx] = None;
            return;
        }
        dev.disabling = true;
        let slot = dev.slot;
        self.queue.push(Trb::disable_slot(slot), Issuer::Device(idx));
    }
}

/// The pipes a configuration asks for: a hub's status-change pipe, or every HID boot
/// interface's report pipe (alternate setting 0); the configuration value and the class the
/// marker names.
fn plan(data: &[u8], device_class: u8) -> Result<(u8, u8, [Option<Plan>; MAX_PIPES]), u8> {
    let cfg = Configuration::parse(data).map_err(|e| e as u8)?;
    let mut plans = [None; MAX_PIPES];
    let mut n = 0;
    let mut class = device_class;
    for iface in cfg.interface_list().filter(|i| i.alternate == 0) {
        if class == 0 {
            class = iface.class;
        }
        let kind = if device_class == 9 || iface.class == 9 {
            PipeKind::HubStatus
        } else if let Some(role) = iface.hid_role() {
            PipeKind::Hid { interface: iface.number, role }
        } else {
            continue;
        };
        let Some(ep) = iface.interrupt_in() else { continue };
        if n == MAX_PIPES {
            break;
        }
        plans[n] = Some(Plan {
            kind,
            endpoint: ep.address,
            max_packet: ep.max_packet_size(),
            interval: ep.interval,
        });
        n += 1;
        if kind == PipeKind::HubStatus {
            class = 9;
            break;
        }
    }
    Ok((cfg.value, class, plans))
}
