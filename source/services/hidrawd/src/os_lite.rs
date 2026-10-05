// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OS-lite `hidrawd` (TASK-0253, its sources since TASK-0253B): ONE loop over the input
//! sources — the virtio-input devices (`virtio_source`) and the USB HID boot interfaces xhcid
//! serves (`usb_source` behind the subscription made here) — on ONE waitset of the endpoints
//! that wake them: the virtio lines' notify endpoint and the USB push channel. Whichever woke
//! is drained into the one batch path to inputd (`batch`). No timer and no polling: every grant
//! is in place before hidrawd runs, a USB device arrives as an attach frame, and an idle
//! hidrawd costs nothing.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`; QEMU: the visible lanes (virtio) and
//!   the `usb-visible` lane (USB)
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::format;

use hid::TimestampNs;
use nexus_abi::{debug_println, nsec, MsgHeader};
use nexus_ipc::timer::Waitset;
use nexus_service_topology::slots;
use nexus_wire::usb as wire;

use crate::batch::Batch;
use crate::source::{Emit, HidSource};
use crate::usb_source::{Heard, Rejected, UsbHid};
use crate::virtio_source::VirtioSource;

/// Push frames taken per wake (the waitset is level-triggered: a rest wakes the loop again).
const FRAMES_PER_WAKE: usize = 16;

pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    let load_span = nexus_abi::Span::begin();
    debug_println("hidrawd: os service payload ready")?;
    let mut batch = Batch::new();
    let mut virtio = VirtioSource::open();
    let mut usb = UsbSource::subscribe();
    let mut sources: [&mut dyn HidSource; 2] = [&mut virtio, &mut usb];
    let mut waitset = Waitset::new().map_err(|_| nexus_abi::AbiError::Unsupported)?;
    let mut members = [u32::MAX; 2];
    for (member, source) in members.iter_mut().zip(sources.iter()) {
        *member = waitset.add(source.wake()).map_err(|_| nexus_abi::AbiError::Unsupported)?;
    }
    let mut ready = false;
    // Whatever arrived before the waits were set up is drained once; from then on, wakes.
    for source in sources.iter_mut() {
        source.drain(now(), &mut batch);
    }
    loop {
        if !ready && sources.iter().any(|source| source.live()) {
            nexus_service_entry::ready("hidrawd: ready")?;
            let _ = debug_println(&format!(
                "hidrawd: timing entry_to_ready_ms={}",
                load_span.elapsed_ms()
            ));
            // RFC-0068: ready reached — emit the folded `hidrawd N/N` verdict (interactive only).
            nexus_abi::service_verdict_flush("hidrawd");
            ready = true;
        }
        let woken = waitset.wait().map_err(|_| nexus_abi::AbiError::Unsupported)?;
        batch.chain.note_wake_for_rate_line();
        for (member, source) in members.iter().zip(sources.iter_mut()) {
            if *member == woken {
                source.drain(now(), &mut batch);
            }
        }
        batch.chain.report_if_due();
    }
}

fn now() -> TimestampNs {
    TimestampNs::new(nsec().unwrap_or(0))
}

/// The USB source on the OS: the subscription to xhcid's HID boot class, and the push channel's
/// frames — taken with their kernel-attributed sender — through the core (`UsbHid`).
struct UsbSource {
    core: UsbHid,
    subscribed: bool,
    report_said: bool,
    refused_said: bool,
}

impl UsbSource {
    /// SUBSCRIBE once, moving the push channel's SEND half to xhcid (its only holder from then
    /// on); the answer arrives on the channel. A waited send without a clock: xhcid's endpoint
    /// exists before either service runs.
    fn subscribe() -> Self {
        let request = wire::encode_subscribe(wire::CLASS_HID_BOOT);
        let sent = nexus_ipc::exchange::send_with_cap(
            slots::hidrawd::XHCID.send,
            &request,
            slots::hidrawd::USB_HID.send,
        );
        if sent.is_err() {
            let _ = debug_println("hidrawd: usb hid subscribe FAIL (no route to xhcid)");
        }
        Self {
            core: UsbHid::new(nexus_abi::service_id_from_name(b"xhcid")),
            subscribed: false,
            report_said: false,
            refused_said: false,
        }
    }

    fn heard(&mut self, heard: Result<crate::usb_source::Heard, Rejected>) {
        match heard {
            Ok(Heard::Subscribed) => {
                self.subscribed = true;
                let _ = debug_println("hidrawd: usb hid subscribed");
            }
            Ok(Heard::Refused(status)) => {
                let _ =
                    debug_println(&format!("hidrawd: usb hid subscribe refused (status={status})"));
            }
            Ok(Heard::Attached(device)) => {
                let _ = debug_println(&format!(
                    "hidrawd: usb hid device (vid={:04x} pid={:04x} role={})",
                    device.vendor,
                    device.product,
                    role(device.kind)
                ));
            }
            Ok(Heard::Reports { refused, .. }) => {
                if !self.report_said {
                    let _ = debug_println("hidrawd: usb hid report seen");
                    self.report_said = true;
                }
                if refused > 0 && !self.refused_said {
                    let _ = debug_println("hidrawd: usb hid report refused (not a boot report)");
                    self.refused_said = true;
                }
            }
            Ok(Heard::Detached(device)) => {
                let _ = debug_println(&format!(
                    "hidrawd: usb hid device gone (vid={:04x} pid={:04x} role={})",
                    device.vendor,
                    device.product,
                    role(device.kind)
                ));
            }
            Err(rejected) => {
                let _ = debug_println(&format!("hidrawd: usb hid frame rejected ({rejected:?})"));
            }
        }
    }
}

impl HidSource for UsbSource {
    fn wake(&self) -> u32 {
        slots::hidrawd::USB_HID.recv
    }

    fn live(&self) -> bool {
        self.core.attached() > 0
    }

    fn drain(&mut self, now: TimestampNs, emit: &mut dyn Emit) {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut frame = [0u8; wire::FRAME_MAX];
        for _ in 0..FRAMES_PER_WAKE {
            let mut sender = 0u64;
            let received = nexus_abi::ipc_recv_v2(
                slots::hidrawd::USB_HID.recv,
                &mut hdr,
                &mut frame,
                &mut sender,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            );
            let Ok(n) = received else { return };
            let heard = self.core.on_frame(sender, &frame[..n.min(frame.len())], now, emit);
            self.heard(heard);
        }
    }
}

const fn role(kind: crate::HidDeviceKind) -> &'static str {
    match kind {
        crate::HidDeviceKind::Keyboard => "keyboard",
        crate::HidDeviceKind::Mouse => "mouse",
    }
}
