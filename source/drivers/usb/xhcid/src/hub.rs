// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Hubs (USB 2.0 §11, RFC-0099 §4): a configured hub gets its slot's hub fields, powers every
//! port, waits its power-good time, then runs its status-change pipe like a HID pipe; every
//! change it reports becomes GET_STATUS(port) on the hub's control pipe, and the answer
//! becomes work — clear the change, queue the port for reset and addressing, detach what was
//! there, or finish the reset the enumeration waits for. One control transfer at a time.

use nexus_hal::Bus;
use nexus_usb::hub::changed_ports;
use nexus_usb::{PortFeature, PortStatus, SetupPacket};

use crate::controller::{Active, PortRef, Xhci};
use crate::device::{HubWork, PipeKind, Stage, CONTROL_BUF};
use crate::memory::DmaAlloc;
use crate::sink::{Note, Sink};

/// The least power-good wait for a hub's ports.
const MIN_POWER_GOOD_NS: u64 = 20_000_000;

impl<B: Bus, A: DmaAlloc> Xhci<B, A> {
    /// Queue work on the hub at `hub`; false when it is no hub or its queue is full.
    pub(crate) fn hub_push(&mut self, hub: usize, work: HubWork) -> bool {
        self.devices[hub].as_mut().and_then(|d| d.hub.as_mut()).is_some_and(|h| h.push(work))
    }

    /// Start the hub's next work item if its control pipe is free.
    pub(crate) fn hub_work(&mut self, idx: usize) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        if dev.stage != Stage::Running || dev.control.is_some() || dev.recovery.is_some() {
            return;
        }
        let Some(hub) = dev.hub.as_mut() else { return };
        let Some(work) = hub.pop() else { return };
        hub.doing = Some(work);
        let setup = match work {
            HubWork::Status(p) => SetupPacket::hub_port_status(p),
            HubWork::Clear(p, f) => SetupPacket::hub_clear_port_feature(p, f),
            HubWork::Set(p, f) => SetupPacket::hub_set_port_feature(p, f),
        };
        self.control(idx, setup);
    }

    /// The hub's status-change pipe reported `bitmap`.
    pub(crate) fn hub_changed(&mut self, idx: usize, bitmap: &[u8], _now: u64) {
        let Some(ports) = self.devices[idx].as_ref().and_then(|d| d.hub).map(|h| h.ports) else {
            return;
        };
        for p in changed_ports(bitmap, ports) {
            self.hub_push(idx, HubWork::Status(p));
        }
        self.hub_work(idx);
    }

    /// A hub work item's control transfer finished.
    pub(crate) fn hub_done(
        &mut self,
        idx: usize,
        result: Result<usize, u8>,
        now: u64,
        sink: &mut impl Sink,
    ) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let doing = dev.hub.as_mut().and_then(|h| h.doing.take());
        if let (Some(HubWork::Status(p)), Ok(n)) = (doing, result) {
            let mut raw = [0u8; 4];
            let bytes =
                dev.core.mem.bytes().get(CONTROL_BUF..CONTROL_BUF + n.min(4)).unwrap_or(&[]);
            raw[..bytes.len()].copy_from_slice(bytes);
            if let Ok(status) = PortStatus::parse(&raw[..bytes.len()]) {
                self.hub_port_status(idx, p, status, now, sink);
            }
        }
        self.hub_work(idx);
    }

    /// Port `p` is mid-reset or its device is being addressed.
    fn port_busy(&self, idx: usize, p: u8) -> bool {
        let here = PortRef::Hub { hub: idx, port: p };
        match self.active {
            Some(Active::Resetting { port, .. } | Active::Recovering { port, .. }) => port == here,
            Some(Active::Addressing { device }) => {
                self.devices[device].as_ref().is_some_and(|d| d.parent == Some((idx, p)))
            }
            None => false,
        }
    }

    fn hub_port_status(
        &mut self,
        idx: usize,
        p: u8,
        status: PortStatus,
        now: u64,
        sink: &mut impl Sink,
    ) {
        let busy = self.port_busy(idx, p);
        let child = self.devices[idx]
            .as_ref()
            .and_then(|d| d.hub)
            .and_then(|h| h.children.get(usize::from(p) - 1).copied().flatten());
        if status.connection_changed() {
            self.hub_push(idx, HubWork::Clear(p, PortFeature::CConnection));
            if let (Some(child), false) = (child, busy) {
                self.detach(child, sink);
            }
        }
        let child = if status.connection_changed() && !busy { None } else { child };
        if status.connected() && child.is_none() && !busy {
            self.queue_port(PortRef::Hub { hub: idx, port: p }, now);
        }
        if status.reset_changed() {
            self.hub_push(idx, HubWork::Clear(p, PortFeature::CReset));
            if busy && status.enabled() {
                self.port_reset_done(PortRef::Hub { hub: idx, port: p }, status.speed(), now);
            }
        }
        if status.enable_changed() {
            self.hub_push(idx, HubWork::Clear(p, PortFeature::CEnable));
        }
        if status.over_current_changed() {
            self.hub_push(idx, HubWork::Clear(p, PortFeature::COverCurrent));
        }
    }

    /// SET_PORT_FEATURE(PORT_POWER) on port `p` finished: the next port, or the power-good wait.
    pub(crate) fn hub_power_done(&mut self, idx: usize, p: u8, now: u64) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let Some(hub) = dev.hub else { return };
        if p < hub.ports {
            let power = SetupPacket::hub_set_port_feature(p + 1, PortFeature::Power);
            return self.request(idx, Stage::HubPower(p + 1), power);
        }
        let wait = (u64::from(hub.power_good_ms) * 1_000_000).max(MIN_POWER_GOOD_NS);
        dev.stage = Stage::HubPowerGood { until: now + wait };
    }

    /// The hub's ports are powered: its status pipe runs, every port is read once.
    pub(crate) fn hub_powered(&mut self, idx: usize, _now: u64, sink: &mut impl Sink) {
        let Some(dev) = self.devices[idx].as_mut() else { return };
        let Some(hub) = dev.hub else { return };
        dev.stage = Stage::Running;
        let status =
            dev.pipes.iter().position(|p| p.is_some_and(|p| p.kind == PipeKind::HubStatus));
        sink.note(Note::Hub {
            slot: dev.slot,
            root_port: dev.root_port,
            speed: dev.speed,
            ports: hub.ports,
            ttt: hub.ttt,
        });
        if let Some(i) = status {
            self.start_pipe(idx, i);
        }
        for p in 1..=hub.ports {
            self.hub_push(idx, HubWork::Status(p));
        }
        self.hub_work(idx);
    }
}
