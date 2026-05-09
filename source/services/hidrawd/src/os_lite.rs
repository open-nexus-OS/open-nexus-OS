// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: OS-lite `hidrawd` live backend owning virtio-input MMIO windows.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`

extern crate alloc;

use alloc::{format, vec::Vec};
use core::time::Duration;

use hid::TimestampNs;
use input_live_protocol::{
    encode_push_hid_batch, HID_KIND_KEYBOARD, HID_KIND_MOUSE, WireHidBatch,
};
use nexus_abi::{cap_clone, cap_close, debug_println, nsec, yield_};
use nexus_ipc::budget::{route_with_nonce_budgeted, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{Client as _, KernelClient, Wait};
use virtio_input::{DeviceRole, DeviceSlot, InputEventKind, MappedVirtioInputDevice, RawInputEvent};

use crate::{
    normalize_ingress_batch, resolve_absolute_axis_max, DeviceId, HidrawdService,
    IngressGateEvidence, IngressRole, PointerSource, RawIngressBatch, RawIngressEvent,
    RawIngressEventKind,
};

const INPUT_CAP_SLOTS: [u32; 3] = [50, 51, 52];
const INPUT_MMIO_VAS: [usize; 3] = [0x2003_0000, 0x2003_1000, 0x2003_2000];
const INPUT_QUEUE_VAS: [usize; 3] = [0x2004_0000, 0x2005_0000, 0x2006_0000];
const INPUT_BUFFER_VAS: [usize; 3] = [0x2007_0000, 0x2008_0000, 0x2009_0000];

pub fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    let mut service = HidrawdService::new();
    let mut live_devices = open_live_devices(&mut service);
    let mut client = route_inputd_blocking();
    let mut ready_emitted = false;
    let mut raw_gate_emitted = false;
    let mut normalized_gate_emitted = false;
    let mut chain = HidrawChainTelemetry::new();

    loop {
        if live_devices.is_empty() {
            live_devices = open_live_devices(&mut service);
        }
        if client.is_none() {
            client = route_inputd_blocking();
            if client.is_none() {
                chain.idle_yields = chain.idle_yields.saturating_add(1);
                chain.report_if_due();
                let _ = yield_();
                continue;
            }
            chain.route_rebinds = chain.route_rebinds.saturating_add(1);
        }
        if !ready_emitted && !live_devices.is_empty() && client.is_some() {
            debug_println("hidrawd: ready")?;
            debug_println("hidrawd: os service payload ready")?;
            ready_emitted = true;
        }
        if live_devices.is_empty() {
            let _ = yield_();
            continue;
        }

        let mut sent_any = false;
        for device in &mut live_devices {
            let Some(polled) = device.poll_batch(&mut service) else {
                continue;
            };
            chain.raw_batches = chain.raw_batches.saturating_add(1);
            chain.raw_events = chain
                .raw_events
                .saturating_add(u64::from(polled.evidence.raw_event_count()));
            chain.normalized_events = chain
                .normalized_events
                .saturating_add(u64::from(polled.evidence.normalized_event_count()));
            match polled.pointer_source {
                None => chain.keyboard_batches = chain.keyboard_batches.saturating_add(1),
                Some(PointerSource::MouseRelative) => {
                    chain.mouse_relative_batches = chain.mouse_relative_batches.saturating_add(1)
                }
                Some(PointerSource::TabletAbsolute) => {
                    chain.tablet_absolute_batches = chain.tablet_absolute_batches.saturating_add(1)
                }
                Some(PointerSource::TouchAbsolute) => {
                    chain.touch_absolute_batches = chain.touch_absolute_batches.saturating_add(1)
                }
            }
            if !raw_gate_emitted && polled.evidence.raw_event_count() > 0 {
                debug_println("hidrawd: virtio-input raw event seen")?;
                raw_gate_emitted = true;
            }
            if !normalized_gate_emitted && polled.evidence.normalized_event_count() > 0 {
                debug_println("hidrawd: ingress adapter ready")?;
                normalized_gate_emitted = true;
            }
            let Some(batch) = polled.wire_batch else {
                chain.wire_batches_skipped = chain.wire_batches_skipped.saturating_add(1);
                continue;
            };
            chain.wire_batches = chain.wire_batches.saturating_add(1);
            let frame = encode_push_hid_batch(&batch);
            let Some(current_client) = client.as_ref() else {
                break;
            };
            if current_client.send(&frame, Wait::Blocking).is_err()
                || current_client.recv(Wait::Blocking).is_err()
            {
                chain.send_failures = chain.send_failures.saturating_add(1);
                client = None;
                break;
            }
            chain.sent_batches = chain.sent_batches.saturating_add(1);
            sent_any = true;
        }

        if !sent_any {
            chain.idle_yields = chain.idle_yields.saturating_add(1);
            let _ = yield_();
        }
        chain.report_if_due();
    }
}

struct HidrawChainTelemetry {
    last_report_ns: u64,
    raw_batches: u64,
    wire_batches: u64,
    wire_batches_skipped: u64,
    sent_batches: u64,
    raw_events: u64,
    normalized_events: u64,
    keyboard_batches: u64,
    mouse_relative_batches: u64,
    tablet_absolute_batches: u64,
    touch_absolute_batches: u64,
    send_failures: u64,
    route_rebinds: u64,
    idle_yields: u64,
}

impl HidrawChainTelemetry {
    const REPORT_INTERVAL_NS: u64 = 1_000_000_000;

    fn new() -> Self {
        Self {
            last_report_ns: nsec().unwrap_or(0),
            raw_batches: 0,
            wire_batches: 0,
            wire_batches_skipped: 0,
            sent_batches: 0,
            raw_events: 0,
            normalized_events: 0,
            keyboard_batches: 0,
            mouse_relative_batches: 0,
            tablet_absolute_batches: 0,
            touch_absolute_batches: 0,
            send_failures: 0,
            route_rebinds: 0,
            idle_yields: 0,
        }
    }

    fn report_if_due(&mut self) {
        let now_ns = nsec().unwrap_or(0);
        if now_ns == 0 || self.last_report_ns == 0 {
            if now_ns != 0 {
                self.last_report_ns = now_ns;
            }
            return;
        }
        let elapsed = now_ns.saturating_sub(self.last_report_ns);
        if elapsed < Self::REPORT_INTERVAL_NS {
            return;
        }
        let ingress_hz = self.raw_batches.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        let sent_hz = self.sent_batches.saturating_mul(1_000_000_000).checked_div(elapsed).unwrap_or(0);
        // #region agent log
        let _ = debug_println(&format!(
            "fps: hidrawd ingress_hz={} sent_hz={} raw_batches={} wire_batches={} wire_skip={} raw_events={} norm_events={} kbd_batches={} mouse_rel={} tablet_abs={} touch_abs={} send_fail={} rebinds={} idle_yields={}",
            ingress_hz,
            sent_hz,
            self.raw_batches,
            self.wire_batches,
            self.wire_batches_skipped,
            self.raw_events,
            self.normalized_events,
            self.keyboard_batches,
            self.mouse_relative_batches,
            self.tablet_absolute_batches,
            self.touch_absolute_batches,
            self.send_failures,
            self.route_rebinds,
            self.idle_yields
        ));
        // #endregion
        self.last_report_ns = now_ns;
        self.raw_batches = 0;
        self.wire_batches = 0;
        self.wire_batches_skipped = 0;
        self.sent_batches = 0;
        self.raw_events = 0;
        self.normalized_events = 0;
        self.keyboard_batches = 0;
        self.mouse_relative_batches = 0;
        self.tablet_absolute_batches = 0;
        self.touch_absolute_batches = 0;
        self.send_failures = 0;
        self.route_rebinds = 0;
        self.idle_yields = 0;
    }
}

fn open_live_devices(service: &mut HidrawdService) -> Vec<LiveDevice> {
    let mut devices = Vec::new();
    let mut emitted_mmio_ready = false;
    for (idx, slot) in INPUT_CAP_SLOTS.into_iter().enumerate() {
        if !slot_present(slot) {
            let _ = debug_println(&format!("hidrawd: input slot missing {}", slot));
            continue;
        }
        let driver = match MappedVirtioInputDevice::open(
            slot,
            INPUT_MMIO_VAS[idx],
            INPUT_QUEUE_VAS[idx],
            INPUT_BUFFER_VAS[idx],
            DeviceSlot::new(idx as u8),
        ) {
            Ok(driver) => driver,
            Err(err) => {
                let _ = debug_println(&format!("hidrawd: input open fail slot={} err={err}", slot));
                continue;
            }
        };
        if !emitted_mmio_ready {
            let _ = debug_println("hidrawd: virtio-input mmio ready");
            emitted_mmio_ready = true;
        }
        let device_id = DeviceId::new((idx + 1) as u16);
        let abs_max_x = driver.absolute_x().map_or(0, |info| info.max());
        let abs_max_y = driver.absolute_y().map_or(0, |info| info.max());
        let provisional_class = match driver.role() {
            DeviceRole::Keyboard => LiveDeviceClass::Keyboard,
            DeviceRole::AbsolutePointer => LiveDeviceClass::Pointer(PointerSource::TabletAbsolute),
            DeviceRole::RelativePointer if abs_max_x > 0 && abs_max_y > 0 => {
                LiveDeviceClass::Pointer(PointerSource::TabletAbsolute)
            }
            DeviceRole::RelativePointer => LiveDeviceClass::Pointer(PointerSource::MouseRelative),
        };
        devices.push(LiveDevice {
            driver,
            device_id,
            provisional_class,
            confirmed_class: None,
            abs_max_x,
            abs_max_y,
        });
    }
    devices
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

fn route_inputd_blocking() -> Option<KernelClient> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    match route_with_nonce_budgeted(
        b"inputd",
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => {
            KernelClient::new_with_slots(send_slot, recv_slot).ok()
        }
        _ => None,
    }
}

struct LiveDevice {
    driver: MappedVirtioInputDevice,
    device_id: DeviceId,
    provisional_class: LiveDeviceClass,
    confirmed_class: Option<LiveDeviceClass>,
    abs_max_x: i32,
    abs_max_y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveDeviceClass {
    Keyboard,
    Pointer(PointerSource),
}

impl LiveDevice {
    fn poll_batch(&mut self, service: &mut HidrawdService) -> Option<PolledDeviceFrame> {
        let Ok(Some(polled)) = self.driver.poll_batch() else {
            return None;
        };
        let timestamp = TimestampNs::new(nsec().unwrap_or(0));
        let raw_events: Vec<RawIngressEvent> = polled.events().iter().copied().map(raw_ingress_event).collect();
        let active_class = infer_device_class(
            self.provisional_class,
            self.confirmed_class,
            &raw_events,
        );
        let active_pointer_source = pointer_source_for_class(active_class);
        let active_role = ingress_role_for_source(active_pointer_source);
        if self.confirmed_class != Some(active_class) {
            match active_class {
                LiveDeviceClass::Keyboard => {
                    service.register_keyboard(self.device_id);
                    let _ = debug_println("hidrawd: device kbd");
                    let _ = debug_println("hidrawd: virtio-input keyboard ready");
                }
                LiveDeviceClass::Pointer(PointerSource::MouseRelative) => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device mouse");
                    let _ = debug_println("hidrawd: source mouse-relative");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
                LiveDeviceClass::Pointer(PointerSource::TabletAbsolute) => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device tablet");
                    let _ = debug_println("hidrawd: source tablet-absolute");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
                LiveDeviceClass::Pointer(PointerSource::TouchAbsolute) => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device touch");
                    let _ = debug_println("hidrawd: source touch-absolute");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
            }
            self.confirmed_class = Some(active_class);
        }
        let raw_batch = RawIngressBatch::with_pointer_source(
            active_role,
            active_pointer_source,
            raw_events,
        );
        self.abs_max_x = resolve_absolute_axis_max(
            active_pointer_source,
            self.abs_max_x,
            raw_batch.events(),
            0,
        );
        self.abs_max_y = resolve_absolute_axis_max(
            active_pointer_source,
            self.abs_max_y,
            raw_batch.events(),
            1,
        );
        let Ok(normalized) = normalize_ingress_batch(
            service,
            self.device_id,
            &raw_batch,
            timestamp,
            self.abs_max_x,
            self.abs_max_y,
        ) else {
            return None;
        };
        let wire_batch = normalized.into_wire_batch().map(|mut batch| {
            batch.device_kind = wire_kind_for(active_role);
            batch
        });
        Some(PolledDeviceFrame {
            evidence: IngressGateEvidence::new(
                raw_batch.events().len().min(u16::MAX as usize) as u16,
                wire_batch.as_ref().map_or(0, |batch| batch.normalized_event_count),
            ),
            pointer_source: active_pointer_source,
            wire_batch,
        })
    }
}

struct PolledDeviceFrame {
    evidence: IngressGateEvidence,
    pointer_source: Option<PointerSource>,
    wire_batch: Option<WireHidBatch>,
}

fn infer_device_class(
    provisional_class: LiveDeviceClass,
    confirmed_class: Option<LiveDeviceClass>,
    raw_events: &[RawIngressEvent],
) -> LiveDeviceClass {
    if let Some(class) = confirmed_class {
        return class;
    }
    if raw_events.iter().any(|event| event.kind() == RawIngressEventKind::Absolute) {
        return match provisional_class {
            LiveDeviceClass::Pointer(PointerSource::TouchAbsolute) => {
                LiveDeviceClass::Pointer(PointerSource::TouchAbsolute)
            }
            _ => LiveDeviceClass::Pointer(PointerSource::TabletAbsolute),
        };
    }
    if raw_events.iter().any(|event| event.kind() == RawIngressEventKind::Relative) {
        return LiveDeviceClass::Pointer(PointerSource::MouseRelative);
    }
    if raw_events
        .iter()
        .any(|event| event.kind() == RawIngressEventKind::Key && event.code() >= 0x110)
    {
        return LiveDeviceClass::Pointer(PointerSource::MouseRelative);
    }
    provisional_class
}

const fn pointer_source_for_class(class: LiveDeviceClass) -> Option<PointerSource> {
    match class {
        LiveDeviceClass::Keyboard => None,
        LiveDeviceClass::Pointer(source) => Some(source),
    }
}

const fn ingress_role_for_source(pointer_source: Option<PointerSource>) -> IngressRole {
    match pointer_source {
        None => IngressRole::Keyboard,
        Some(PointerSource::MouseRelative) => IngressRole::RelativePointer,
        Some(PointerSource::TabletAbsolute) => IngressRole::AbsolutePointer,
        Some(PointerSource::TouchAbsolute) => IngressRole::AbsolutePointer,
    }
}

fn wire_kind_for(role: IngressRole) -> u8 {
    match role {
        IngressRole::Keyboard => HID_KIND_KEYBOARD,
        IngressRole::RelativePointer | IngressRole::AbsolutePointer => HID_KIND_MOUSE,
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
