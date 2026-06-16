// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `hidrawd` service crate for bounded boot-protocol keyboard/mouse ingest.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod adapter;
mod error;
mod ingest;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod os_lite;
mod service;
mod types;

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
};
pub use types::{DeviceId, HidBatch, HidDeviceKind};

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    println!("hidrawd: ready");
}
