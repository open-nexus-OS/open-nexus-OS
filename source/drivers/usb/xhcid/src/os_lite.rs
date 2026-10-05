// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The service loop (OS target, RFC-0099 §2): the controller's window mapped from the device
//! capability init granted (`device.mmio.usb` — QEMU's PCI xHCI or the board's tree node), its
//! line bound to xhcid's notify endpoint, DMA memory made for the device (contiguous, in its
//! reach, maintained with Zicbom when it does not snoop), and ONE waitset over the line and a
//! kernel one-shot: the loop sleeps until the controller interrupts or the next bounded wait is
//! due — it never polls. Markers are formatted on the stack; no heap in the loop. A tree
//! without a host controller gets an honest `usb plane none` and xhcid parks.

use core::fmt::{self, Write as _};

use nexus_abi::{CapQuery, DmaCoherence, DmaVmo, MmioWindow};
use nexus_driverkit::{DmaShared, Mmio, Zicbom};
use nexus_ipc::timer::{NotifyTimer, Waitset};
use nexus_service_topology::{slots, DEVICE_MMIO_SLOT};
use nexus_usb::{HidRole, Speed};

use crate::{DmaAlloc, Note, Sink, Xhci};

pub type Result<T> = core::result::Result<T, &'static str>;

/// Notifications drained per wake (the line stays claimed until completed).
const DRAIN_MAX: usize = 8;
const LINE_MAX: usize = 192;

/// A marker line on the stack.
struct Line {
    buf: [u8; LINE_MAX],
    len: usize,
}

impl Line {
    const fn new() -> Self {
        Self { buf: [0; LINE_MAX], len: 0 }
    }

    fn emit(&self) {
        let text = core::str::from_utf8(&self.buf[..self.len]).unwrap_or("xhcid: marker not utf-8");
        let _ = nexus_abi::debug_println(text);
    }
}

impl fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = (self.len + s.len()).min(LINE_MAX);
        let take = end - self.len;
        self.buf[self.len..end].copy_from_slice(&s.as_bytes()[..take]);
        self.len = end;
        Ok(())
    }
}

fn emit(args: fmt::Arguments<'_>) {
    let mut line = Line::new();
    let _ = line.write_fmt(args);
    line.emit();
}

const fn speed(s: Speed) -> &'static str {
    match s {
        Speed::Low => "low",
        Speed::Full => "full",
        Speed::High => "high",
        Speed::Super => "super",
    }
}

const fn role(r: HidRole) -> &'static str {
    match r {
        HidRole::Keyboard => "keyboard",
        HidRole::Mouse => "mouse",
    }
}

/// The markers (RFC-0099 §7); reports are counted until the HID client takes them (U2).
struct OsSink {
    irq: u32,
    reports: u64,
}

impl Sink for OsSink {
    fn note(&mut self, note: Note<'_>) {
        match note {
            Note::ControllerOk { version, ports, slots, context_size, scratchpads } => emit(format_args!(
                "xhcid: controller ok (version={}.{:02x} ports={} slots={} csz={} scratch={} irq={})",
                version >> 8,
                version & 0xff,
                ports,
                slots,
                context_size,
                scratchpads,
                self.irq
            )),
            Note::Ready { ports, connected } => {
                emit(format_args!("xhcid: ready (ports={ports} connected={connected})"));
            }
            Note::SuperSpeedPort { port } => {
                emit(format_args!("xhcid: superspeed port left alone (port={port})"));
            }
            Note::Hub { slot, root_port, speed: s, ports, ttt } => emit(format_args!(
                "xhcid: hub (slot={slot} port={root_port} speed={} ports={ports} ttt={ttt})",
                speed(s)
            )),
            Note::Enumerated { slot, route, speed: s, vendor, product, class } => emit(format_args!(
                "xhcid: device enumerated (slot={slot} route={route:#x} speed={} vid={vendor:04x} pid={product:04x} class={class})",
                speed(s)
            )),
            Note::HidInterface { slot, interface, role: r, endpoint, max_packet, interval } => {
                emit(format_args!(
                    "xhcid: hid boot interface (slot={slot} if={interface} role={} ep={endpoint:#04x} mps={max_packet} interval={interval})",
                    role(r)
                ));
            }
            Note::Report { .. } => self.reports += 1,
            Note::Detached { slot } => emit(format_args!("xhcid: device detached (slot={slot})")),
            Note::Fail { step, code } => {
                emit(format_args!("xhcid: FAIL (step={} cc={code})", step.name()));
            }
        }
    }
}

/// The controller's memory: contiguous objects made for its device capability.
struct OsAlloc {
    device: u32,
    coherence: DmaCoherence,
}

impl DmaAlloc for OsAlloc {
    type Mem = DmaVmo;
    type Cache = Zicbom;

    fn shared(&mut self, len: usize) -> Option<DmaShared<DmaVmo, Zicbom>> {
        let vmo = DmaVmo::contiguous(self.device, len).ok()?;
        DmaShared::new(vmo, self.coherence, Zicbom).ok()
    }
}

fn now() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// Wait on an empty waitset's one member forever: nothing to drive, no CPU spent.
fn park(timer: &mut NotifyTimer, waitset: &Waitset) -> Result<()> {
    timer.disarm();
    loop {
        waitset.wait().map_err(|_| "waitset")?;
        let _ = timer.drain();
    }
}

/// The service.
pub fn service_main_loop() -> Result<()> {
    let device = DEVICE_MMIO_SLOT;
    let mut timer = NotifyTimer::bind(slots::xhcid::TIMER).map_err(|_| "timer")?;
    let mut waitset = Waitset::new().map_err(|_| "waitset")?;
    let timer_member = waitset.add(timer.recv_slot()).map_err(|_| "waitset")?;
    let mut info = CapQuery::default();
    if nexus_abi::cap_query(device, &mut info).is_err() || info.kind_tag != 2 {
        emit(format_args!("xhcid: no host controller (usb plane none)"));
        return park(&mut timer, &waitset);
    }
    let len = usize::try_from(info.len).map_err(|_| "window")?;
    let window = MmioWindow::map(device, 0, len).map_err(|_| "window")?;
    let coherence = nexus_abi::device_dma_coherence(device).map_err(|_| "coherence")?;
    let irq_ep = slots::xhcid::IRQ_NOTIFY;
    let irq_member = (info.irq != 0 && nexus_abi::irq_bind(info.irq, irq_ep).is_ok())
        .then(|| waitset.add(irq_ep).ok())
        .flatten();
    let mut sink = OsSink { irq: info.irq, reports: 0 };
    let Some(irq_member) = irq_member else {
        emit(format_args!("xhcid: FAIL (step=irq cc=0)"));
        return park(&mut timer, &waitset);
    };
    let mut xhci = Xhci::new(Mmio::new(window), OsAlloc { device, coherence });
    xhci.start(now(), &mut sink);
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut frame = [0u8; 16];
    loop {
        match xhci.deadline() {
            Some(at) => timer.arm_at(at),
            None => timer.disarm(),
        }
        let member = waitset.wait().map_err(|_| "waitset")?;
        if member == irq_member {
            for _ in 0..DRAIN_MAX {
                if nexus_abi::ipc_recv_v1_nb(irq_ep, &mut hdr, &mut frame, true).is_err() {
                    break;
                }
            }
            xhci.on_interrupt(now(), &mut sink);
            let _ = nexus_abi::irq_complete(info.irq);
        } else if member == timer_member {
            let _ = timer.drain();
            xhci.on_timer(now(), &mut sink);
        }
    }
}
