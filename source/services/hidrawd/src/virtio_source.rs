// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: hidrawd's virtio-input source (OS only; TASK-0253, behind the source contract since
//! TASK-0253B): every virtio-input window init granted, opened once — the grants precede the
//! service, so there is nothing to re-probe — each device's line bound to hidrawd's one
//! interrupt endpoint. A wake acks the devices' latches, drains their used rings into
//! evdev-shaped events, settles each device's class from what it sends (`virtio_class`, host-
//! tested), and normalizes into HID events for the one batch path; then the lines are completed
//! and the loop waits again.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the visible QEMU lanes (input ladder, pixel proof); the normalization is
//!   host-tested in `tests/contract.rs`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::{format, vec::Vec};

use hid::{HidEvent, TimestampNs};
use nexus_abi::{cap_clone, cap_close, debug_println, irq_bind, irq_complete, MsgHeader};
use nexus_service_topology::{slots, INPUT_MMIO_SLOTS};
use virtio_input::{
    DeviceRole, DeviceSlot, InputEventKind, MappedVirtioInputDevice, RawInputEvent,
};

use crate::source::{Emit, HidSource};
use crate::virtio_class::{VirtioClass, VirtioDevice};
use crate::{
    normalize_ingress_into, DeviceId, PointerSource, RawIngressEvent, RawIngressEventKind,
};

/// Interrupt notifications consumed per wake (one per line until it is completed).
const NOTIFY_MAX: usize = 8;

/// The virtio-input devices and the buffers their drains reuse (no allocation once warm).
pub(crate) struct VirtioSource {
    devices: Vec<LiveDevice>,
    raw_input: Vec<RawInputEvent>,
    raw_ingress: Vec<RawIngressEvent>,
    hid: Vec<HidEvent>,
    raw_seen: bool,
}

impl VirtioSource {
    /// Every granted window opened, every line bound to the interrupt endpoint.
    pub(crate) fn open() -> Self {
        let mut devices = Vec::new();
        let mut mmio_said = false;
        for (idx, slot) in INPUT_MMIO_SLOTS.into_iter().enumerate() {
            if !slot_present(slot) {
                continue;
            }
            let driver = match MappedVirtioInputDevice::open(slot, DeviceSlot::new(idx as u8)) {
                Ok(driver) => driver,
                Err(err) => {
                    let _ =
                        debug_println(&format!("hidrawd: input open fail slot={slot} err={err}"));
                    continue;
                }
            };
            if !mmio_said {
                let _ = debug_println("hidrawd: virtio-input mmio ready");
                mmio_said = true;
            }
            let abs_max_x = driver.absolute_x().map_or(0, |info| info.max());
            let abs_max_y = driver.absolute_y().map_or(0, |info| info.max());
            let announced = match driver.role() {
                DeviceRole::Keyboard => VirtioClass::Keyboard,
                DeviceRole::AbsolutePointer => VirtioClass::Pointer(PointerSource::TabletAbsolute),
                DeviceRole::RelativePointer => VirtioClass::relative_pointer(abs_max_x, abs_max_y),
            };
            let device_id = DeviceId::new((idx + 1) as u16);
            devices.push(LiveDevice {
                driver,
                state: VirtioDevice::new(device_id, announced, abs_max_x, abs_max_y),
                // RFC-0098 C3: the line the granted capability carries (init read it from the
                // node's `interrupts`); 0 = none, never derived from an address.
                irq: nexus_abi::device_irq(slot),
            });
        }
        let bound = devices
            .iter()
            .filter(|d| d.irq != 0 && irq_bind(d.irq, slots::hidrawd::IRQ_NOTIFY).is_ok())
            .count();
        if bound > 0 {
            let _ = debug_println("hidrawd: irq endpoint bound (reactive input)");
        }
        Self {
            devices,
            raw_input: Vec::with_capacity(64),
            raw_ingress: Vec::with_capacity(64),
            hid: Vec::with_capacity(64),
            raw_seen: false,
        }
    }
}

impl HidSource for VirtioSource {
    fn wake(&self) -> u32 {
        slots::hidrawd::IRQ_NOTIFY
    }

    fn live(&self) -> bool {
        !self.devices.is_empty()
    }

    /// The level-triggered drain protocol (the input-rate ceiling fix): ack every latch FIRST,
    /// then drain — an event landing during the drain sets its latch afresh and is caught by
    /// the next pass, or stays pending for immediate redelivery after the lines are completed.
    /// A pass that drains nothing ends it (bounded: each pass consumes real ring entries).
    fn drain(&mut self, now: TimestampNs, emit: &mut dyn Emit) {
        let (mut hdr, mut note) = (MsgHeader::new(0, 0, 0, 0, 0), [0u8; 16]);
        for _ in 0..NOTIFY_MAX {
            if nexus_abi::ipc_recv_v1_nb(slots::hidrawd::IRQ_NOTIFY, &mut hdr, &mut note, true)
                .is_err()
            {
                break;
            }
        }
        loop {
            for device in &self.devices {
                device.driver.ack_interrupt();
            }
            let mut drained = false;
            for device in &mut self.devices {
                if !device.driver.poll_batch_into(&mut self.raw_input).unwrap_or(false) {
                    continue;
                }
                drained = true;
                self.raw_ingress.clear();
                self.raw_ingress.extend(self.raw_input.iter().copied().map(raw_ingress_event));
                if !self.raw_seen && !self.raw_ingress.is_empty() {
                    let _ = debug_println("hidrawd: virtio-input raw event seen");
                    self.raw_seen = true;
                }
                let (class, frame, settled) = device.state.settle(&self.raw_ingress);
                if let Some(class) = settled {
                    say_class(class);
                }
                let evidence =
                    normalize_ingress_into(class.role(), &self.raw_ingress, now, &mut self.hid);
                emit.emit(&frame, evidence.raw_event_count(), &self.hid);
            }
            if !drained {
                break;
            }
        }
        for device in &self.devices {
            let _ = irq_complete(device.irq);
        }
    }
}

struct LiveDevice {
    driver: MappedVirtioInputDevice,
    /// Its class and axis maxima (host-tested: `virtio_class`).
    state: VirtioDevice,
    /// PLIC interrupt source for this device, as carried by its MMIO capability (RFC-0098 C3).
    /// 0 = the tree lists none.
    irq: u32,
}

fn say_class(class: VirtioClass) {
    let lines: &[&str] = match class {
        VirtioClass::Keyboard => &["hidrawd: device kbd", "hidrawd: virtio-input keyboard ready"],
        VirtioClass::Pointer(PointerSource::MouseRelative) => &[
            "hidrawd: device mouse",
            "hidrawd: source mouse-relative",
            "hidrawd: virtio-input pointer ready",
        ],
        VirtioClass::Pointer(PointerSource::TabletAbsolute) => &[
            "hidrawd: device tablet",
            "hidrawd: source tablet-absolute",
            "hidrawd: virtio-input pointer ready",
        ],
        VirtioClass::Pointer(PointerSource::TouchAbsolute) => &[
            "hidrawd: device touch",
            "hidrawd: source touch-absolute",
            "hidrawd: virtio-input pointer ready",
        ],
    };
    for line in lines {
        let _ = debug_println(line);
    }
}

fn raw_ingress_event(event: RawInputEvent) -> RawIngressEvent {
    let kind = match event.kind() {
        InputEventKind::Key => RawIngressEventKind::Key,
        InputEventKind::Relative => RawIngressEventKind::Relative,
        InputEventKind::Absolute => RawIngressEventKind::Absolute,
        InputEventKind::Syn | InputEventKind::Unknown(_) => RawIngressEventKind::Key,
    };
    RawIngressEvent::new(kind, event.code(), event.value())
}

fn slot_present(slot: u32) -> bool {
    match cap_clone(slot) {
        Ok(tmp) => {
            let _ = cap_close(tmp);
            true
        }
        Err(_) => false,
    }
}
