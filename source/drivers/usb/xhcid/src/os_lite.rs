// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The service loop (OS target, RFC-0099 §2): the controller's window mapped from the device
//! capability init granted (`device.mmio.usb` — QEMU's PCI xHCI or the board's tree node), its
//! line bound to xhcid's notify endpoint, DMA memory made for the device (contiguous, in its
//! reach, maintained with Zicbom when it does not snoop), and ONE waitset over the line, a
//! kernel one-shot and the server endpoint (TASK-0253B: the HID class's client subscribes
//! there, RFC-0099 §5): the loop sleeps until the controller interrupts, the next bounded wait
//! is due or a client asks — it never polls. After every wake the class gets what it is owed.
//! Markers are formatted on the stack; no heap in the loop. A tree without a host controller
//! gets an honest `usb plane none`, and xhcid still serves: its subscriber hears no interface.

use core::fmt::{self, Write as _};

use nexus_abi::{CapQuery, DmaCoherence, DmaVmo, IpcError, MmioWindow};
use nexus_driverkit::{DmaShared, Mmio, Zicbom};
use nexus_ipc::timer::{NotifyTimer, Waitset};
use nexus_ipc::{KernelServer, Server as _, Wait};
use nexus_service_topology::{slots, DEVICE_MMIO_SLOT};
use nexus_usb::{HidRole, Speed};
use nexus_wire::usb as wire;

use crate::{Channel, DmaAlloc, HidClass, Note, Pushed, Sink, Xhci};

pub type Result<T> = core::result::Result<T, &'static str>;

/// Notifications drained per wake (the line stays claimed until completed).
const DRAIN_MAX: usize = 8;
/// Requests served per wake (a class has one subscriber; anything more waits a wake).
const SERVE_MAX: usize = 4;
/// What a HID class subscriber must hold (policyd, deny-by-default).
const CAP_USB_HID: &[u8] = b"usb.hid";
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

/// The subscriber's push channel: the SEND capability its SUBSCRIBE moved here. Closed when
/// the class lets go of it (a new subscriber, a dead client, a refusal).
struct OsChannel {
    slot: u32,
}

impl Channel for OsChannel {
    fn push(&mut self, frame: &[u8]) -> Pushed {
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        match nexus_abi::ipc_send_v1(self.slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => Pushed::Sent,
            Err(IpcError::QueueFull | IpcError::NoSpace) => Pushed::Full,
            Err(_) => Pushed::Gone,
        }
    }
}

impl Drop for OsChannel {
    fn drop(&mut self) {
        let _ = nexus_abi::cap_close(self.slot);
    }
}

/// The markers (RFC-0099 §7), and every note handed to the HID class server.
struct OsSink {
    irq: u32,
    class: HidClass<OsChannel>,
    /// The client was full once (said once: the count is the class's).
    full_said: bool,
}

impl Sink for OsSink {
    fn note(&mut self, note: Note<'_>) {
        self.class.note(&note);
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
            Note::HidInterface { slot, interface, role: r, endpoint, max_packet, interval, .. } => {
                emit(format_args!(
                    "xhcid: hid boot interface (slot={slot} if={interface} role={} ep={endpoint:#04x} mps={max_packet} interval={interval})",
                    role(r)
                ));
            }
            Note::Report { .. } => {}
            Note::HidLost { slot, interface } => {
                emit(format_args!("xhcid: hid interface lost (slot={slot} if={interface})"));
            }
            Note::Detached { slot } => emit(format_args!("xhcid: device detached (slot={slot})")),
            Note::Fail { step, code } => {
                emit(format_args!("xhcid: FAIL (step={} cc={code})", step.name()));
            }
        }
    }
}

impl OsSink {
    /// What the class owes goes out; a client that was full is said once.
    fn flush(&mut self, now: u64) {
        self.class.flush(now);
        if !self.full_said && self.class.dropped() > 0 {
            emit(format_args!("xhcid: hid client full (reports dropped)"));
            self.full_said = true;
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

/// The controller, running: its driver, its line and the line's waitset member.
struct Controller {
    xhci: Xhci<Mmio, OsAlloc>,
    irq: u32,
    member: u32,
}

/// The window init granted, mapped; its line bound into the waitset; the controller started.
/// `None` — said — when the tree has no host controller or its line cannot be bound.
fn controller(waitset: &mut Waitset, sink: &mut OsSink) -> Option<Controller> {
    let device = DEVICE_MMIO_SLOT;
    let mut info = CapQuery::default();
    if nexus_abi::cap_query(device, &mut info).is_err() || info.kind_tag != 2 {
        emit(format_args!("xhcid: no host controller (usb plane none)"));
        return None;
    }
    let len = usize::try_from(info.len).ok()?;
    let window = MmioWindow::map(device, 0, len).ok()?;
    let coherence = nexus_abi::device_dma_coherence(device).ok()?;
    let irq_ep = slots::xhcid::IRQ_NOTIFY;
    let member = (info.irq != 0 && nexus_abi::irq_bind(info.irq, irq_ep).is_ok())
        .then(|| waitset.add(irq_ep).ok())
        .flatten();
    let Some(member) = member else {
        emit(format_args!("xhcid: FAIL (step=irq cc=0)"));
        return None;
    };
    sink.irq = info.irq;
    let mut xhci = Xhci::new(Mmio::new(window), OsAlloc { device, coherence });
    xhci.start(now(), sink);
    Some(Controller { xhci, irq: info.irq, member })
}

/// SUBSCRIBEs on the server endpoint: policy first (`usb.hid` of the kernel-attributed sender,
/// never a payload), then the class. The answer goes on the moved channel; a frame without one
/// is answered on the shared response endpoint.
fn serve(server: &KernelServer, sink: &mut OsSink, now: u64) {
    let mut frame = [0u8; 16];
    for _ in 0..SERVE_MAX {
        let Ok((n, sender, moved)) =
            server.recv_request_with_meta_into(Wait::NonBlocking, &mut frame)
        else {
            return;
        };
        let Some(cap) = moved else {
            let _ =
                server.send(&wire::encode_subscribed(wire::STATUS_MALFORMED), Wait::NonBlocking);
            continue;
        };
        let channel = OsChannel { slot: cap.slot() };
        let status = match frame.get(..n).and_then(wire::decode_subscribe) {
            None => wire::STATUS_MALFORMED,
            Some(wire::CLASS_HID_BOOT) if admitted(sender) => wire::STATUS_OK,
            Some(wire::CLASS_HID_BOOT) => wire::STATUS_DENIED,
            Some(_) => wire::STATUS_UNSUPPORTED,
        };
        if status == wire::STATUS_OK {
            drop(sink.class.subscribe(channel));
            sink.flush(now);
            emit(format_args!(
                "xhcid: hid class subscribed (interfaces={})",
                sink.class.attached()
            ));
        } else {
            HidClass::refuse(channel, status);
            emit(format_args!("xhcid: hid class subscriber refused (status={status})"));
        }
    }
}

/// policyd's verdict on `usb.hid` for the subscriber; anything but an allow — an unattributed
/// sender, an unreachable authority — is a refusal.
fn admitted(sender: u64) -> bool {
    sender != 0
        && matches!(
            nexus_ipc::policyd::check_cap_on(
                slots::xhcid::POLICYD.send,
                slots::xhcid::REPLY.send,
                slots::xhcid::REPLY.recv,
                sender,
                CAP_USB_HID,
            ),
            nexus_ipc::policyd::CapDecision::Allow
        )
}

/// The service.
pub fn service_main_loop() -> Result<()> {
    let mut timer = NotifyTimer::bind(slots::xhcid::TIMER).map_err(|_| "timer")?;
    let mut waitset = Waitset::new().map_err(|_| "waitset")?;
    let timer_member = waitset.add(timer.recv_slot()).map_err(|_| "waitset")?;
    let server = KernelServer::new_with_slots(slots::xhcid::SERVER.recv, slots::xhcid::SERVER.send)
        .map_err(|_| "server slots")?;
    let server_member = waitset.add(slots::xhcid::SERVER.recv).map_err(|_| "waitset")?;
    // RFC-0093: a server of the DisplayReady stage — it serves from here on, with a controller
    // or without one.
    let _ = nexus_service_entry::ready("xhcid: serving (class=hid-boot)");
    let mut sink = OsSink { irq: 0, class: HidClass::new(), full_said: false };
    let mut controller = controller(&mut waitset, &mut sink);
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut notification = [0u8; 16];
    loop {
        let xhci_due = controller.as_ref().and_then(|c| c.xhci.deadline());
        match earliest(xhci_due, sink.class.deadline()) {
            Some(at) => timer.arm_at(at),
            None => timer.disarm(),
        }
        let member = waitset.wait().map_err(|_| "waitset")?;
        let t = now();
        match controller.as_mut() {
            Some(c) if member == c.member => {
                for _ in 0..DRAIN_MAX {
                    if nexus_abi::ipc_recv_v1_nb(
                        slots::xhcid::IRQ_NOTIFY,
                        &mut hdr,
                        &mut notification,
                        true,
                    )
                    .is_err()
                    {
                        break;
                    }
                }
                c.xhci.on_interrupt(t, &mut sink);
                let _ = nexus_abi::irq_complete(c.irq);
            }
            _ if member == timer_member => {
                let _ = timer.drain();
                if let Some(c) = controller.as_mut() {
                    c.xhci.on_timer(t, &mut sink);
                }
                sink.class.on_timer(t);
            }
            _ if member == server_member => serve(&server, &mut sink, t),
            _ => {}
        }
        sink.flush(t);
    }
}

/// The earlier of two deadlines.
fn earliest(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}
