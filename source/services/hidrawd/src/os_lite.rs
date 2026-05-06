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
    normalize_ingress_batch, DeviceId, HidrawdService, IngressGateEvidence, IngressRole,
    RawIngressBatch, RawIngressEvent, RawIngressEventKind,
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

    // #region agent log
    agent_log(
        "H4",
        "source/services/hidrawd/src/os_lite.rs:35",
        "hidrawd initial route to inputd",
        &format!(
            "has_client={} live_devices={}",
            u8::from(client.is_some()),
            live_devices.len()
        ),
    );
    // #endregion

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
            "fps: hidrawd ingress_hz={} sent_hz={} raw_batches={} wire_batches={} wire_skip={} raw_events={} norm_events={} send_fail={} rebinds={} idle_yields={}",
            ingress_hz,
            sent_hz,
            self.raw_batches,
            self.wire_batches,
            self.wire_batches_skipped,
            self.raw_events,
            self.normalized_events,
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
        let (provisional_role, abs_max_x, abs_max_y) = match driver.role() {
            DeviceRole::Keyboard => (IngressRole::Keyboard, 0, 0),
            DeviceRole::RelativePointer => (IngressRole::RelativePointer, 0, 0),
            DeviceRole::AbsolutePointer => (
                IngressRole::AbsolutePointer,
                driver.absolute_x().map_or(0, |info| info.max()),
                driver.absolute_y().map_or(0, |info| info.max()),
            ),
        };
        devices.push(LiveDevice {
            driver,
            device_id,
            provisional_role,
            confirmed_role: None,
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

fn agent_log(hypothesis_id: &'static str, location: &'static str, message: &'static str, data: &str) {
    let _ = debug_println(&format!("agent8cde1d|{hypothesis_id}|{location}|{message}|{data}"));
}

struct LiveDevice {
    driver: MappedVirtioInputDevice,
    device_id: DeviceId,
    provisional_role: IngressRole,
    confirmed_role: Option<IngressRole>,
    abs_max_x: i32,
    abs_max_y: i32,
}

impl LiveDevice {
    fn poll_batch(&mut self, service: &mut HidrawdService) -> Option<PolledDeviceFrame> {
        let Ok(Some(polled)) = self.driver.poll_batch() else {
            return None;
        };
        let timestamp = TimestampNs::new(nsec().unwrap_or(0));
        let raw_events: Vec<RawIngressEvent> = polled.events().iter().copied().map(raw_ingress_event).collect();
        let active_role = infer_ingress_role(self.provisional_role, self.confirmed_role, &raw_events);
        if self.confirmed_role != Some(active_role) {
            match active_role {
                IngressRole::Keyboard => {
                    service.register_keyboard(self.device_id);
                    let _ = debug_println("hidrawd: device kbd");
                    let _ = debug_println("hidrawd: virtio-input keyboard ready");
                }
                IngressRole::RelativePointer | IngressRole::AbsolutePointer => {
                    service.register_mouse(self.device_id);
                    let _ = debug_println("hidrawd: device mouse");
                    let _ = debug_println("hidrawd: virtio-input pointer ready");
                }
            }
            self.confirmed_role = Some(active_role);
        }
        let raw_batch = RawIngressBatch::new(
            active_role,
            raw_events,
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
            wire_batch,
        })
    }
}

struct PolledDeviceFrame {
    evidence: IngressGateEvidence,
    wire_batch: Option<WireHidBatch>,
}

fn infer_ingress_role(
    provisional_role: IngressRole,
    confirmed_role: Option<IngressRole>,
    raw_events: &[RawIngressEvent],
) -> IngressRole {
    if let Some(role) = confirmed_role {
        return role;
    }
    if raw_events.iter().any(|event| event.kind() == RawIngressEventKind::Absolute) {
        return IngressRole::AbsolutePointer;
    }
    if raw_events.iter().any(|event| event.kind() == RawIngressEventKind::Relative) {
        return IngressRole::RelativePointer;
    }
    provisional_role
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
