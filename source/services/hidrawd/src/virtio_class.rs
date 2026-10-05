// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: what a virtio-input device is, from what it sends (TASK-0253; host-tested since
//! TASK-0253B). The class its driver announced is provisional — a pointer the driver calls
//! relative but that has both absolute axes is a tablet — and the first batch that tells settles
//! it: absolute motion makes a tablet (a touch device stays touch), relative motion or a button
//! key makes a mouse. Once settled it stays. The batch header follows from it: the pointer source
//! and the absolute axis maxima (QEMU's range when the device names none). The virtio source
//! (`virtio_source`, OS) holds one per device.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/virtio_class.rs`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::source::DeviceFrame;
use crate::{
    resolve_absolute_axis_max, DeviceId, IngressRole, PointerSource, RawIngressEvent,
    RawIngressEventKind,
};

/// What a device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioClass {
    Keyboard,
    Pointer(PointerSource),
}

impl VirtioClass {
    /// A pointer the driver calls relative: a tablet if it names both absolute axes.
    #[must_use]
    pub const fn relative_pointer(abs_max_x: i32, abs_max_y: i32) -> Self {
        if abs_max_x > 0 && abs_max_y > 0 {
            Self::Pointer(PointerSource::TabletAbsolute)
        } else {
            Self::Pointer(PointerSource::MouseRelative)
        }
    }

    /// The pointer source a batch of this device carries (`None`: a keyboard).
    #[must_use]
    pub const fn pointer(self) -> Option<PointerSource> {
        match self {
            Self::Keyboard => None,
            Self::Pointer(source) => Some(source),
        }
    }

    /// How its raw events normalize.
    #[must_use]
    pub const fn role(self) -> IngressRole {
        match self {
            Self::Keyboard => IngressRole::Keyboard,
            Self::Pointer(PointerSource::MouseRelative) => IngressRole::RelativePointer,
            Self::Pointer(PointerSource::TabletAbsolute | PointerSource::TouchAbsolute) => {
                IngressRole::AbsolutePointer
            }
        }
    }
}

/// One device's class — announced, then settled by what it sends — and its axis maxima.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtioDevice {
    device: DeviceId,
    announced: VirtioClass,
    settled: Option<VirtioClass>,
    abs_max_x: i32,
    abs_max_y: i32,
}

impl VirtioDevice {
    /// A device as its driver announced it (`abs_max_*`: the axis ranges it names, 0 for none).
    #[must_use]
    pub const fn new(
        device: DeviceId,
        announced: VirtioClass,
        abs_max_x: i32,
        abs_max_y: i32,
    ) -> Self {
        Self { device, announced, settled: None, abs_max_x, abs_max_y }
    }

    /// The class this batch tells (settled once, it stays) and the header the batch carries; the
    /// class is returned a second time when it settled just now (said once).
    pub fn settle(
        &mut self,
        raw: &[RawIngressEvent],
    ) -> (VirtioClass, DeviceFrame, Option<VirtioClass>) {
        let class = self.settled.unwrap_or_else(|| told(self.announced, raw));
        let just_now = (self.settled != Some(class)).then_some(class);
        self.settled = Some(class);
        let pointer = class.pointer();
        self.abs_max_x = resolve_absolute_axis_max(pointer, self.abs_max_x, raw, 0);
        self.abs_max_y = resolve_absolute_axis_max(pointer, self.abs_max_y, raw, 1);
        let frame = DeviceFrame {
            device: self.device,
            pointer,
            abs_max_x: self.abs_max_x,
            abs_max_y: self.abs_max_y,
        };
        (class, frame, just_now)
    }
}

/// What a first batch tells about a device the driver announced as `announced`.
fn told(announced: VirtioClass, raw: &[RawIngressEvent]) -> VirtioClass {
    let any = |kind| raw.iter().any(|event| event.kind() == kind);
    if any(RawIngressEventKind::Absolute) {
        return match announced {
            VirtioClass::Pointer(PointerSource::TouchAbsolute) => announced,
            _ => VirtioClass::Pointer(PointerSource::TabletAbsolute),
        };
    }
    if any(RawIngressEventKind::Relative)
        || raw.iter().any(|event| event.kind() == RawIngressEventKind::Key && event.code() >= 0x110)
    {
        return VirtioClass::Pointer(PointerSource::MouseRelative);
    }
    announced
}
