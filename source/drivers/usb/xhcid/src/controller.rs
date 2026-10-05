// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The controller (RFC-0099 §2, §4): its bring-up as a state machine paced by the one-shot
//! timer (ready → halted → reset → set up → running → ports powered — each wait re-read at a
//! millisecond, bounded), and the reactive half — per interrupt the event ring is drained and
//! every event goes to its owner: a command completion to the device that issued it, a
//! transfer event to the pipe it belongs to, a port status change to the root-port logic.

use nexus_hal::Bus;
use nexus_usb::Speed;

use crate::command::{InFlight, Issuer, Queue, COMMAND_NS};
use crate::context::{speed_of, Layout};
use crate::device::{Device, Stage, MAX_DEVICES};
use crate::memory::{DmaAlloc, Region};
use crate::regs::{
    port_neutral, port_speed, Regs, CMD_HCRST, CMD_INTE, CMD_RUN, CONFIG, CRCR, DCBAAP, ERDP,
    ERDP_EHB, ERSTBA, ERSTSZ, IMAN, IMAN_IE, IMAN_IP, IMOD, MAX_PORTS, PORT_CCS, PORT_CHANGES,
    PORT_CSC, PORT_PED, PORT_PP, PORT_PRC, STS_CNR, STS_EINT, STS_HCE, STS_HCH, STS_HSE, STS_PCD,
    USBCMD, USBSTS,
};
use crate::ring::{EventRing, Producer};
use crate::sink::{Note, Sink, Step};
use crate::trb::{self, Trb, CC_TIMEOUT};

/// The controller region: DCBAA, the scratchpad array, the ERST, the two rings.
const CTRL_LEN: usize = 8192;
pub(crate) const DCBAA: usize = 0x0000;
const SCRATCH_ARRAY: usize = 0x0100;
const ERST: usize = 0x0200;
const COMMAND_RING: usize = 0x0400;
const COMMAND_TRBS: usize = 64;
const EVENT_RING: usize = 0x1000;
const EVENT_TRBS: usize = 256;
/// Scratchpad pages at most (one region).
const MAX_SCRATCHPADS: u16 = 16;
const PAGE: usize = 4096;
/// How often a bring-up wait re-reads its register.
const STEP_NS: u64 = 1_000_000;
const READY_NS: u64 = 1_000_000_000;
const HALT_NS: u64 = 20_000_000;
const RUN_NS: u64 = 20_000_000;
/// Root ports' power-good after PP (xHCI 1.2 §4.19.4).
const POWER_NS: u64 = 20_000_000;
/// IMODI in 250 ns: 1 ms (RFC-0099 §2).
const IMOD_INTERVAL: u32 = 4000;
/// Events handled per interrupt before ERDP moves (the ring's size).
const EVENTS_PER_WAKE: usize = EVENT_TRBS;
/// Port tasks waiting at most.
const TASKS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    WaitReady { give_up: u64, next: u64 },
    Halting { give_up: u64, next: u64 },
    Resetting { give_up: u64, next: u64 },
    Starting { give_up: u64, next: u64 },
    Powering { until: u64 },
    Running,
    Failed,
}

/// A port a device is found on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortRef {
    /// A root port.
    Root(u8),
    /// Port `port` of the hub at device index `hub`.
    Hub { hub: usize, port: u8 },
}

/// The one port being reset and addressed (two devices never share the default address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Active {
    /// The port resets; a hub port is re-read every `next` until `give_up`.
    Resetting { port: PortRef, give_up: u64, next: u64 },
    /// The reset finished; the device gets its slot after the recovery time.
    Recovering { port: PortRef, speed: Speed, until: u64 },
    /// Enable Slot / Address Device in flight for `device`.
    Addressing { device: usize },
}

/// The controller.
pub struct Xhci<B: Bus, A: DmaAlloc> {
    pub(crate) regs: Regs<B>,
    pub(crate) alloc: A,
    pub(crate) layout: Layout,
    phase: Phase,
    pub(crate) ctrl: Option<Region<A::Mem, A::Cache>>,
    scratch: Option<Region<A::Mem, A::Cache>>,
    pub(crate) commands: Producer,
    events: EventRing,
    pub(crate) queue: Queue,
    revisions: [u8; MAX_PORTS],
    pub(crate) devices: [Option<Device<A::Mem, A::Cache>>; MAX_DEVICES],
    /// The device on each root port (index = port − 1).
    pub(crate) root: [Option<usize>; MAX_PORTS],
    tasks: [Option<(PortRef, u64)>; TASKS],
    task_head: usize,
    task_count: usize,
    pub(crate) active: Option<Active>,
}

impl<B: Bus, A: DmaAlloc> Xhci<B, A> {
    /// The controller behind `bus`, its memory made by `alloc`.
    pub fn new(bus: B, alloc: A) -> Self {
        let regs = Regs::new(bus);
        let layout = Layout { size: regs.caps().context_size };
        Self {
            regs,
            alloc,
            layout,
            phase: Phase::Idle,
            ctrl: None,
            scratch: None,
            commands: Producer::new(COMMAND_RING, COMMAND_TRBS),
            events: EventRing::new(EVENT_RING, EVENT_TRBS),
            queue: Queue::new(),
            revisions: [0; MAX_PORTS],
            devices: core::array::from_fn(|_| None),
            root: [None; MAX_PORTS],
            tasks: [None; TASKS],
            task_head: 0,
            task_count: 0,
            active: None,
        }
    }

    /// Begin the bring-up.
    pub fn start(&mut self, now: u64, sink: &mut impl Sink) {
        self.phase = Phase::WaitReady { give_up: now + READY_NS, next: now };
        self.step(now, sink);
    }

    /// The controller runs and every root port is powered.
    #[must_use]
    pub fn running(&self) -> bool {
        self.phase == Phase::Running
    }

    /// The bring-up failed.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.phase == Phase::Failed
    }

    /// The earliest deadline a wait has (the one-shot to arm), `None` when nothing waits.
    #[must_use]
    pub fn deadline(&self) -> Option<u64> {
        let phase = match self.phase {
            Phase::WaitReady { next, .. }
            | Phase::Halting { next, .. }
            | Phase::Resetting { next, .. }
            | Phase::Starting { next, .. } => Some(next),
            Phase::Powering { until } => Some(until),
            _ => None,
        };
        let command = self.queue.in_flight.map(|c| c.deadline);
        let active = match self.active {
            Some(Active::Resetting { next, .. }) => Some(next),
            Some(Active::Recovering { until, .. }) => Some(until),
            _ => None,
        };
        let task = (self.active.is_none() && self.task_count > 0)
            .then(|| self.tasks[self.task_head].map(|(_, at)| at))
            .flatten();
        let power = self.devices.iter().flatten().find_map(|d| match d.stage {
            Stage::HubPowerGood { until } => Some(until),
            _ => None,
        });
        [phase, command, active, task, power].into_iter().flatten().min()
    }

    /// The one-shot fired (or a deadline passed).
    pub fn on_timer(&mut self, now: u64, sink: &mut impl Sink) {
        self.step(now, sink);
        if let Some(c) = self.queue.in_flight.filter(|c| c.deadline <= now) {
            self.queue.in_flight = None;
            if let Issuer::Device(idx) = c.issuer {
                self.device_command_done(idx, CC_TIMEOUT, 0, now, sink);
            }
        }
        self.active_timer(now, sink);
        for idx in 0..MAX_DEVICES {
            let due = matches!(
                self.devices[idx].as_ref().map(|d| d.stage),
                Some(Stage::HubPowerGood { until }) if until <= now
            );
            if due {
                self.hub_powered(idx, now, sink);
            }
        }
        self.pump(now);
    }

    /// The controller's line fired: drain the event ring.
    pub fn on_interrupt(&mut self, now: u64, sink: &mut impl Sink) {
        if self.ctrl.is_none() {
            return;
        }
        let status = self.regs.op(USBSTS);
        self.regs.set_op(USBSTS, status & (STS_EINT | STS_PCD | STS_HSE));
        self.regs.set_rt(IMAN, IMAN_IP | IMAN_IE);
        if status & (STS_HSE | STS_HCE) != 0 {
            sink.note(Note::Fail { step: Step::Controller, code: 0 });
            self.phase = Phase::Failed;
            return;
        }
        for _ in 0..EVENTS_PER_WAKE {
            let Some(ctrl) = self.ctrl.as_mut() else { return };
            let Some(event) = self.events.next(ctrl) else { break };
            self.dispatch(event, now, sink);
        }
        if let Some(ctrl) = self.ctrl.as_ref() {
            self.regs.set_rt64(ERDP, self.events.dequeue_pointer(ctrl) | ERDP_EHB);
        }
        self.pump(now);
    }

    fn dispatch(&mut self, event: Trb, now: u64, sink: &mut impl Sink) {
        match event.kind() {
            trb::COMMAND_COMPLETION => {
                let Some(c) = self.queue.in_flight.filter(|c| c.address == event.param) else {
                    return;
                };
                self.queue.in_flight = None;
                if let Issuer::Device(idx) = c.issuer {
                    self.device_command_done(idx, event.completion(), event.slot_id(), now, sink);
                }
            }
            trb::TRANSFER_EVENT => self.transfer_event(event, now, sink),
            trb::PORT_STATUS_CHANGE => self.root_port_changed(event.port_id(), now, sink),
            trb::HOST_CONTROLLER => {
                sink.note(Note::Fail { step: Step::Controller, code: event.completion() });
            }
            _ => {}
        }
    }

    /// Submit the next command and start the next port task.
    pub(crate) fn pump(&mut self, now: u64) {
        if let Some((command, issuer)) = self.queue.take_next() {
            if let Some(ctrl) = self.ctrl.as_mut() {
                let address = self.commands.push(ctrl, command);
                let deadline = now + COMMAND_NS;
                self.queue.in_flight = Some(InFlight { address, issuer, deadline });
                self.regs.ring(0, 0);
            }
        }
        self.next_task(now);
    }

    fn step(&mut self, now: u64, sink: &mut impl Sink) {
        loop {
            match self.phase {
                Phase::WaitReady { give_up, next } if next <= now => {
                    if self.regs.op(USBSTS) & STS_CNR == 0 {
                        let command = self.regs.op(USBCMD);
                        if command & CMD_RUN != 0 {
                            self.regs.set_op(USBCMD, command & !CMD_RUN);
                            self.phase = Phase::Halting { give_up: now + HALT_NS, next: now };
                            continue;
                        }
                        return self.reset(now);
                    }
                    self.wait_or_fail(give_up, now, sink);
                }
                Phase::Halting { give_up, next } if next <= now => {
                    if self.regs.op(USBSTS) & STS_HCH != 0 {
                        return self.reset(now);
                    }
                    self.wait_or_fail(give_up, now, sink);
                }
                Phase::Resetting { give_up, next } if next <= now => {
                    let busy = self.regs.op(USBCMD) & CMD_HCRST != 0
                        || self.regs.op(USBSTS) & STS_CNR != 0;
                    if !busy {
                        if let Err(step) = self.set_up() {
                            sink.note(Note::Fail { step, code: 0 });
                            self.phase = Phase::Failed;
                            return;
                        }
                        self.phase = Phase::Starting { give_up: now + RUN_NS, next: now };
                        continue;
                    }
                    self.wait_or_fail(give_up, now, sink);
                }
                Phase::Starting { give_up, next } if next <= now => {
                    if self.regs.op(USBSTS) & STS_HCH == 0 {
                        let caps = *self.regs.caps();
                        sink.note(Note::ControllerOk {
                            version: caps.version,
                            ports: caps.max_ports,
                            slots: caps.max_slots,
                            context_size: caps.context_size as u8,
                            scratchpads: caps.scratchpads,
                        });
                        if caps.port_power {
                            for port in 1..=self.port_count() {
                                let sc = self.regs.portsc(port);
                                self.regs.set_portsc(port, port_neutral(sc) | PORT_PP);
                            }
                            self.phase = Phase::Powering { until: now + POWER_NS };
                            return;
                        }
                        return self.run(now, sink);
                    }
                    self.wait_or_fail(give_up, now, sink);
                }
                Phase::Powering { until } if until <= now => return self.run(now, sink),
                _ => return,
            }
            return;
        }
    }

    /// Re-read at the next step, or give up.
    fn wait_or_fail(&mut self, give_up: u64, now: u64, sink: &mut impl Sink) {
        if now >= give_up {
            sink.note(Note::Fail { step: Step::Controller, code: 0 });
            self.phase = Phase::Failed;
            return;
        }
        let next = now + STEP_NS;
        self.phase = match self.phase {
            Phase::WaitReady { give_up, .. } => Phase::WaitReady { give_up, next },
            Phase::Halting { give_up, .. } => Phase::Halting { give_up, next },
            Phase::Resetting { give_up, .. } => Phase::Resetting { give_up, next },
            Phase::Starting { give_up, .. } => Phase::Starting { give_up, next },
            other => other,
        };
    }

    fn reset(&mut self, now: u64) {
        self.regs.set_op(USBCMD, CMD_HCRST);
        // A controller may not be read within a millisecond of HCRST: the first re-read waits.
        self.phase = Phase::Resetting { give_up: now + READY_NS, next: now + STEP_NS };
    }

    /// The controller's memory and registers after a reset (xHCI 1.2 §4.2).
    fn set_up(&mut self) -> Result<(), Step> {
        let caps = *self.regs.caps();
        let mut ctrl = self.alloc.shared(CTRL_LEN).and_then(Region::new).ok_or(Step::Memory)?;
        if caps.scratchpads > MAX_SCRATCHPADS {
            return Err(Step::Memory);
        }
        if caps.scratchpads > 0 {
            let pages = usize::from(caps.scratchpads);
            let len = (pages * PAGE).next_power_of_two();
            let scratch = self.alloc.shared(len).and_then(Region::new).ok_or(Step::Memory)?;
            for page in 0..pages {
                ctrl.put(SCRATCH_ARRAY + 8 * page, &scratch.bus(page * PAGE).to_le_bytes());
            }
            ctrl.put(DCBAA, &ctrl.bus(SCRATCH_ARRAY).to_le_bytes());
            self.scratch = Some(scratch);
        }
        let slots = caps.max_slots.min(MAX_DEVICES as u8);
        self.regs.set_op(CONFIG, u32::from(slots));
        self.regs.set_op64(DCBAAP, ctrl.bus(DCBAA));
        self.commands = Producer::new(COMMAND_RING, COMMAND_TRBS);
        self.regs.set_op64(CRCR, self.commands.start(&ctrl));
        self.events = EventRing::new(EVENT_RING, EVENT_TRBS);
        let (segment, size) = self.events.segment(&ctrl);
        let mut entry = [0u8; 16];
        entry[..8].copy_from_slice(&segment.to_le_bytes());
        entry[8..12].copy_from_slice(&size.to_le_bytes());
        ctrl.put(ERST, &entry);
        self.regs.set_rt(ERSTSZ, 1);
        self.regs.set_rt64(ERDP, segment);
        self.regs.set_rt64(ERSTBA, ctrl.bus(ERST));
        self.regs.set_rt(IMOD, IMOD_INTERVAL);
        self.regs.set_rt(IMAN, IMAN_IP | IMAN_IE);
        self.regs.set_op(USBCMD, CMD_RUN | CMD_INTE);
        self.ctrl = Some(ctrl);
        Ok(())
    }

    fn port_count(&self) -> u8 {
        self.regs.caps().max_ports.min(MAX_PORTS as u8)
    }

    /// Running: every connected USB 2 root port gets a task.
    fn run(&mut self, now: u64, sink: &mut impl Sink) {
        self.phase = Phase::Running;
        self.revisions = self.regs.port_revisions();
        let mut connected = 0;
        for port in 1..=self.port_count() {
            let sc = self.regs.portsc(port);
            if sc & PORT_CCS == 0 {
                continue;
            }
            connected += 1;
            self.regs.set_portsc(port, port_neutral(sc) | PORT_CSC);
            if self.revisions[usize::from(port) - 1] == 3 {
                sink.note(Note::SuperSpeedPort { port });
                continue;
            }
            self.queue_port(PortRef::Root(port), now);
        }
        sink.note(Note::Ready { ports: self.port_count(), connected });
        self.pump(now);
    }

    fn root_port_changed(&mut self, port: u8, now: u64, sink: &mut impl Sink) {
        if port == 0 || port > self.port_count() {
            return;
        }
        let sc = self.regs.portsc(port);
        self.regs.set_portsc(port, port_neutral(sc) | (sc & PORT_CHANGES));
        let usb3 = self.revisions[usize::from(port) - 1] == 3;
        if sc & PORT_CSC != 0 {
            if let Some(device) = self.root[usize::from(port) - 1].take() {
                self.detach(device, sink);
            }
            if sc & PORT_CCS != 0 {
                if usb3 {
                    sink.note(Note::SuperSpeedPort { port });
                } else {
                    self.queue_port(PortRef::Root(port), now);
                }
            }
        }
        let resetting = matches!(
            self.active,
            Some(Active::Resetting { port: PortRef::Root(p), .. }) if p == port
        );
        if resetting && sc & PORT_PRC != 0 {
            match speed_of(port_speed(sc)).filter(|_| sc & PORT_PED != 0) {
                Some(speed) => self.port_reset_done(PortRef::Root(port), speed, now),
                None => {
                    sink.note(Note::Fail { step: Step::PortReset, code: 0 });
                    self.active = None;
                }
            }
        }
    }

    /// Queue a port for reset and addressing after the attach debounce — once: a port whose
    /// connection the root-port scan and a change event both report, or a hub's change pipe and
    /// its status read, is queued one time, and never while it is the active task or already
    /// has its device (a second reset would re-enumerate a device that holds a slot).
    pub(crate) fn queue_port(&mut self, port: PortRef, now: u64) {
        let waiting = (0..self.task_count)
            .any(|i| matches!(self.tasks[(self.task_head + i) % TASKS], Some((p, _)) if p == port));
        if waiting || self.task_count == TASKS || self.port_taken(port) {
            return;
        }
        self.tasks[(self.task_head + self.task_count) % TASKS] = Some((port, now + DEBOUNCE_NS));
        self.task_count += 1;
    }

    /// `port` is being reset or addressed, or a device sits on it.
    fn port_taken(&self, port: PortRef) -> bool {
        let active = match self.active {
            Some(Active::Resetting { port: p, .. } | Active::Recovering { port: p, .. }) => {
                p == port
            }
            Some(Active::Addressing { .. }) | None => false,
        };
        let attached = match port {
            PortRef::Root(p) => self.root.get(usize::from(p) - 1).is_some_and(Option::is_some),
            PortRef::Hub { hub, port: p } => self.devices[hub]
                .as_ref()
                .and_then(|d| d.hub)
                .and_then(|h| h.children.get(usize::from(p) - 1).copied().flatten())
                .is_some(),
        };
        active || attached
    }

    /// The next due port task, when no port is being reset or addressed.
    pub(crate) fn pop_task(&mut self, now: u64) -> Option<PortRef> {
        if self.active.is_some() || self.task_count == 0 {
            return None;
        }
        let (port, due) = self.tasks[self.task_head]?;
        if due > now {
            return None;
        }
        self.tasks[self.task_head] = None;
        self.task_head = (self.task_head + 1) % TASKS;
        self.task_count -= 1;
        Some(port)
    }

    /// The device index of slot `slot`.
    pub(crate) fn device_of(&self, slot: u8) -> Option<usize> {
        if slot == 0 {
            return None;
        }
        self.devices.iter().position(|d| d.as_ref().is_some_and(|d| d.slot == slot))
    }
}

/// USB 2.0 attach debounce before a port is reset.
pub(crate) const DEBOUNCE_NS: u64 = 100_000_000;
