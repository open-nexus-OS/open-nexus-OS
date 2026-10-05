// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pipes (RFC-0099 §2): control transfers on EP0 — setup, data, status as three TRBs, the
//! status stage's event ends the transfer, a short data stage reports its residual — and the
//! interrupt-IN pipes, [`PIPE_TRBS`] normal TRBs always queued: each completion hands its bytes
//! on and requeues the same buffer at once, so the controller keeps polling the device at its
//! interval while the CPU sleeps. A halted endpoint is recovered (Reset Endpoint, then Set TR
//! Dequeue Pointer past the failed TD) before anything else happens on it.

use nexus_hal::Bus;
use nexus_usb::SetupPacket;

use crate::command::Issuer;
use crate::controller::Xhci;
use crate::device::{PipeKind, Recovery, RecoveryStep, Stage, CONTROL_BUF, CONTROL_MAX, PIPE_TRBS};
use crate::memory::DmaAlloc;
use crate::sink::{Note, Sink, Step};
use crate::trb::{Trb, CC_SHORT_PACKET, CC_SUCCESS};

/// Failed interrupt transfers in a row before the pipe is given up.
const MAX_PIPE_ERRORS: u8 = 3;

impl<B: Bus, A: DmaAlloc> Xhci<B, A> {
    /// Start a control transfer on the device's EP0.
    pub(crate) fn control(&mut self, idx: usize, setup: SetupPacket) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let len = setup.length.min(CONTROL_MAX as u16);
        let data_in = setup.is_in();
        if len > 0 {
            // No dirty line may sit over what the device writes.
            dev.core.mem.observe(CONTROL_BUF..CONTROL_BUF + usize::from(len));
        }
        dev.ep0.push(&mut dev.core, Trb::setup(setup.to_bytes(), len, data_in));
        let data_trb = (len > 0).then(|| {
            let buffer = dev.core.bus(CONTROL_BUF);
            dev.ep0.push(&mut dev.core, Trb::data(buffer, len, data_in))
        });
        let status_trb = dev.ep0.push(&mut dev.core, Trb::status_stage(len, data_in));
        dev.control = Some(crate::device::Control { setup, data_trb, status_trb, short: 0 });
        self.regs.ring(dev.slot, 1);
    }

    /// A transfer event: EP0's or an interrupt pipe's.
    pub(crate) fn transfer_event(&mut self, event: Trb, now: u64, sink: &mut impl Sink) {
        let Some(idx) = self.device_of(event.slot_id()) else { return };
        match event.endpoint_id() {
            1 => self.control_event(idx, event, now, sink),
            dci => self.pipe_event(idx, dci, event, now, sink),
        }
    }

    fn control_event(&mut self, idx: usize, event: Trb, now: u64, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let Some(mut c) = dev.control.filter(|_| dev.stage != Stage::Detaching) else { return };
        let cc = event.completion();
        if cc == CC_SHORT_PACKET && Some(event.param) == c.data_trb {
            c.short = event.residual();
            dev.control = Some(c);
            return;
        }
        if cc == CC_SUCCESS && event.param == c.status_trb {
            dev.control = None;
            let requested = usize::from(c.setup.length.min(CONTROL_MAX as u16));
            let n = requested.saturating_sub(c.short as usize);
            if c.setup.is_in() && n > 0 {
                dev.core.mem.observe(CONTROL_BUF..CONTROL_BUF + n);
            }
            return self.device_control_done(idx, Ok(n), now, sink);
        }
        if cc == CC_SUCCESS || cc == CC_SHORT_PACKET {
            return;
        }
        // EP0 halted: recover, then report the code.
        dev.recovery = Some(Recovery { dci: 1, step: RecoveryStep::Reset, failed: cc });
        let slot = dev.slot;
        self.queue.push(Trb::reset_endpoint(slot, 1), Issuer::Device(idx));
    }

    /// A recovery command completed: Set TR Dequeue Pointer after Reset Endpoint; after that,
    /// the transfer that halted is reported (EP0) or the pipe's TRBs are queued again.
    pub(crate) fn recovery_done(&mut self, idx: usize, now: u64, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let Some(r) = dev.recovery else { return };
        match r.step {
            RecoveryStep::Reset => {
                let dequeue = if r.dci == 1 {
                    Some(dev.ep0.dequeue_pointer(&dev.core))
                } else {
                    let pipe = dev.pipes.iter().flatten().find(|p| p.dci == r.dci);
                    pipe.zip(dev.pipes_mem.as_ref()).map(|(p, mem)| p.ring.dequeue_pointer(mem))
                };
                let Some(dequeue) = dequeue else {
                    dev.recovery = None;
                    return;
                };
                dev.recovery = Some(Recovery { step: RecoveryStep::Dequeue, ..r });
                let slot = dev.slot;
                self.queue.push(Trb::set_tr_dequeue(slot, r.dci, dequeue), Issuer::Device(idx));
            }
            RecoveryStep::Dequeue => {
                dev.recovery = None;
                if r.dci == 1 {
                    dev.control = None;
                    self.device_control_done(idx, Err(r.failed), now, sink);
                } else if let Some(i) =
                    dev.pipes.iter().position(|p| p.is_some_and(|p| p.dci == r.dci))
                {
                    self.start_pipe(idx, i);
                }
            }
        }
    }

    /// Queue [`PIPE_TRBS`] TRBs on pipe `i`, one per buffer, and ring its doorbell.
    pub(crate) fn start_pipe(&mut self, idx: usize, i: usize) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let (Some(mut pipe), Some(mem)) = (dev.pipes[i], dev.pipes_mem.as_mut()) else { return };
        for k in 0..PIPE_TRBS {
            let buffer = mem.bus(pipe.buffers + k * pipe.stride);
            let address = pipe.ring.push(mem, Trb::normal(buffer, pipe.max_packet));
            pipe.queued[k] = (address, k as u8);
        }
        pipe.head = 0;
        pipe.next_buffer = 0;
        dev.pipes[i] = Some(pipe);
        self.regs.ring(dev.slot, pipe.dci);
    }

    /// An interrupt pipe's TRB completed: hand the bytes on, requeue the buffer.
    fn pipe_event(&mut self, idx: usize, dci: u8, event: Trb, now: u64, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        if dev.stage == Stage::Detaching || dev.recovery.is_some() {
            return;
        }
        let Some(i) = dev.pipes.iter().position(|p| p.is_some_and(|p| p.dci == dci)) else {
            return;
        };
        let Some(mut pipe) = dev.pipes[i] else { return };
        let (address, buffer) = pipe.queued[pipe.head];
        if event.param != address {
            return;
        }
        let cc = event.completion();
        if cc != CC_SUCCESS && cc != CC_SHORT_PACKET {
            pipe.errors += 1;
            if pipe.errors > MAX_PIPE_ERRORS {
                dev.pipes[i] = None;
                return sink.note(Note::Fail { step: Step::Interrupt, code: cc });
            }
            dev.pipes[i] = Some(pipe);
            dev.recovery = Some(Recovery { dci, step: RecoveryStep::Reset, failed: cc });
            let slot = dev.slot;
            self.queue.push(Trb::reset_endpoint(slot, dci), Issuer::Device(idx));
            return;
        }
        pipe.errors = 0;
        let Some(mem) = dev.pipes_mem.as_mut() else { return };
        let n = usize::from(pipe.max_packet).saturating_sub(event.residual() as usize);
        let at = pipe.buffers + usize::from(buffer) * pipe.stride;
        mem.mem.observe(at..at + usize::from(pipe.max_packet));
        let slot = dev.slot;
        let mut bitmap = [0u8; 2];
        match pipe.kind {
            PipeKind::Hid { interface, role } => {
                let bytes = mem.mem.bytes().get(at..at + n).unwrap_or(&[]);
                sink.note(Note::Report { slot, interface, role, bytes });
            }
            PipeKind::HubStatus => {
                let bytes = mem.mem.bytes().get(at..at + n.min(2)).unwrap_or(&[]);
                bitmap[..bytes.len()].copy_from_slice(bytes);
            }
        }
        let bus = mem.bus(at);
        let requeued = pipe.ring.push(mem, Trb::normal(bus, pipe.max_packet));
        pipe.queued[pipe.head] = (requeued, buffer);
        pipe.head = (pipe.head + 1) % PIPE_TRBS;
        dev.pipes[i] = Some(pipe);
        self.regs.ring(slot, dci);
        if pipe.kind == PipeKind::HubStatus {
            self.hub_changed(idx, &bitmap[..n.min(2)], now);
        }
    }
}
