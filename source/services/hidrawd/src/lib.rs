// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `hidrawd` service crate: the HID ingress — one loop over its input sources
//! (virtio-input, USB HID boot interfaces via xhcid; TASK-0253B) into the one batch path to
//! inputd, with the bounded boot-protocol parsers in one device table.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod adapter;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod batch;
mod error;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod os_lite;
mod service;
pub mod source;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod telemetry;
mod types;
pub mod usb_source;
pub mod virtio_class;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod virtio_source;

pub use adapter::{
    normalize_ingress_batch, normalize_ingress_into, resolve_absolute_axis_max,
    IngressGateEvidence, IngressNormalization, IngressRole, PointerSource, RawIngressBatch,
    RawIngressEvent, RawIngressEventKind, QEMU_ABSOLUTE_AXIS_FALLBACK_MAX,
};
pub use error::HidrawdError;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub use os_lite::service_main_loop;
pub use service::{
    classify_live_route_send_error, HidrawdService, LiveRouteSendAction, LiveRouteSendErrorClass,
    MAX_DEVICES,
};
pub use types::{DeviceId, HidBatch, HidDeviceKind};

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    println!("hidrawd: ready");
}
