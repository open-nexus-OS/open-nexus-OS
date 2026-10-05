// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the virtio source's decisions against fake event batches (TASK-0253B): what a device
//! is — announced by its driver, settled by its first telling batch, then fixed — and the header
//! its batches carry (pointer source, axis maxima with QEMU's range as the fallback).
//! OWNERS: @runtime @ui

use hidrawd::virtio_class::{VirtioClass, VirtioDevice};
use hidrawd::{
    DeviceId, IngressRole, PointerSource, RawIngressEvent, RawIngressEventKind,
    QEMU_ABSOLUTE_AXIS_FALLBACK_MAX,
};

fn abs(axis: u16, value: i32) -> RawIngressEvent {
    RawIngressEvent::new(RawIngressEventKind::Absolute, axis, value)
}

fn rel(axis: u16, value: i32) -> RawIngressEvent {
    RawIngressEvent::new(RawIngressEventKind::Relative, axis, value)
}

fn key(code: u16, value: i32) -> RawIngressEvent {
    RawIngressEvent::new(RawIngressEventKind::Key, code, value)
}

const TABLET: VirtioClass = VirtioClass::Pointer(PointerSource::TabletAbsolute);
const MOUSE: VirtioClass = VirtioClass::Pointer(PointerSource::MouseRelative);
const TOUCH: VirtioClass = VirtioClass::Pointer(PointerSource::TouchAbsolute);

#[test]
fn a_relative_pointer_naming_both_absolute_axes_is_a_tablet() {
    assert_eq!(VirtioClass::relative_pointer(32_767, 32_767), TABLET);
    assert_eq!(VirtioClass::relative_pointer(0, 0), MOUSE);
    assert_eq!(VirtioClass::relative_pointer(32_767, 0), MOUSE, "one axis is no tablet");
    assert_eq!(TABLET.role(), IngressRole::AbsolutePointer);
    assert_eq!(TOUCH.role(), IngressRole::AbsolutePointer);
    assert_eq!(MOUSE.role(), IngressRole::RelativePointer);
    assert_eq!(VirtioClass::Keyboard.role(), IngressRole::Keyboard);
}

/// The first batch that tells settles the class — said once — and later batches cannot flip it.
#[test]
fn the_class_settles_once_from_what_the_device_sends() {
    let mut device = VirtioDevice::new(DeviceId::new(2), VirtioClass::Keyboard, 0, 0);
    let (class, frame, settled) = device.settle(&[key(0x110, 1)]);
    assert_eq!((class, settled), (MOUSE, Some(MOUSE)), "a button key makes a mouse");
    assert_eq!(frame.pointer, Some(PointerSource::MouseRelative));
    assert_eq!(frame.device, DeviceId::new(2));
    let (class, _, settled) = device.settle(&[abs(0, 10), abs(1, 10)]);
    assert_eq!((class, settled), (MOUSE, None), "settled stays, and is said once");
}

/// Absolute motion makes a tablet; the axis range is QEMU's when the device named none.
#[test]
fn absolute_motion_makes_a_tablet_with_an_axis_range() {
    let mut device = VirtioDevice::new(DeviceId::new(3), MOUSE, 0, 0);
    let (class, frame, settled) = device.settle(&[abs(0, 100), abs(1, 200)]);
    assert_eq!((class, settled), (TABLET, Some(TABLET)));
    assert_eq!(
        (frame.abs_max_x, frame.abs_max_y),
        (QEMU_ABSOLUTE_AXIS_FALLBACK_MAX, QEMU_ABSOLUTE_AXIS_FALLBACK_MAX)
    );
    // A range the device names is kept.
    let mut named = VirtioDevice::new(DeviceId::new(3), TABLET, 4095, 4095);
    let (_, frame, _) = named.settle(&[abs(0, 1)]);
    assert_eq!((frame.abs_max_x, frame.abs_max_y), (4095, 4095));
}

#[test]
fn a_touch_device_stays_touch_and_relative_motion_makes_a_mouse() {
    let mut touch = VirtioDevice::new(DeviceId::new(1), TOUCH, 0, 0);
    assert_eq!(touch.settle(&[abs(0, 5), abs(1, 5)]).0, TOUCH);
    let mut mouse = VirtioDevice::new(DeviceId::new(1), TABLET, 0, 0);
    assert_eq!(mouse.settle(&[rel(0, 3)]).0, MOUSE);
}

/// Keys below the button range tell nothing new: the keyboard stays what it was announced.
#[test]
fn a_keyboard_that_types_stays_a_keyboard() {
    let mut device = VirtioDevice::new(DeviceId::new(1), VirtioClass::Keyboard, 0, 0);
    let (class, frame, settled) = device.settle(&[key(30, 1), key(30, 0)]);
    assert_eq!((class, settled), (VirtioClass::Keyboard, Some(VirtioClass::Keyboard)));
    assert_eq!((frame.pointer, frame.abs_max_x, frame.abs_max_y), (None, 0, 0));
}
