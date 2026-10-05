// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: hidrawd's input sources behind one contract (TASK-0253B). A transport turns what it
//! delivered into normalized `hid::HidEvent`s per device and hands them to the ONE batch path
//! ([`Emit`]), which frames them for inputd (`WireHidBatch`, unchanged). Two transports: the
//! virtio-input devices QEMU's virt machine gives (`virtio_source`) and the USB HID boot
//! interfaces xhcid serves (`usb_source`, RFC-0099 §5). Each source owns the endpoint that
//! wakes it; the loop waits on all of them at once and drains the one that woke — no timer, no
//! polling.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd` (the USB source against xhcid's frames)
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::{HidEvent, TimestampNs};

use crate::{DeviceId, PointerSource};

/// Whose events a batch carries: the header inputd checks them against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceFrame {
    /// The device's name on the wire.
    pub device: DeviceId,
    /// `None`: a keyboard; a pointer names its source.
    pub pointer: Option<PointerSource>,
    /// An absolute pointer's axis maxima (0 for the others).
    pub abs_max_x: i32,
    pub abs_max_y: i32,
}

/// The one batch path: where every source's normalized events go.
pub trait Emit {
    /// `raw` transport units (virtio events, USB reports) became `events` of `frame`'s device —
    /// possibly none (nothing that normalizes), which the raw count still counts.
    fn emit(&mut self, frame: &DeviceFrame, raw: u16, events: &[HidEvent]);
}

/// A transport hidrawd reads.
pub trait HidSource {
    /// The endpoint (its RECV slot) that wakes this source — a member of the loop's waitset.
    fn wake(&self) -> u32;
    /// Everything pending on the transport, drained into `emit`.
    fn drain(&mut self, now: TimestampNs, emit: &mut dyn Emit);
    /// A device of this source is live.
    fn live(&self) -> bool;
}
