// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: xhcid against the behavioural machine (`xhcid-model`, non-coherent memory): the
//! bring-up and its bounded waits, enumeration through a full-speed hub (QEMU's shape) and
//! through the board's high-speed hub with the desk's devices as measured (descriptors from
//! `docs/board/measurements/2026-10-04-usb-boot-protocol/stock-descriptors.txt`, the TT fields
//! the machine checks, the mouse that STALLs SET_IDLE), reports end to end, an idle bus that
//! costs nothing, detach and re-attach, the refusals — and the HID class server's subscriber
//! (TASK-0253B): what it hears, in which order, with a client that is full or dead.
//! OWNERS: @runtime @drivers

// The modules live in `tests/xhci/` (one test target; `tests/*.rs` would make each its own).
#[path = "xhci/bringup.rs"]
mod bringup;
#[path = "xhci/class.rs"]
mod class;
#[path = "xhci/detach.rs"]
mod detach;
#[path = "xhci/enumerate.rs"]
mod enumerate;
#[path = "xhci/reject.rs"]
mod reject;

use nexus_usb::Speed;
use xhcid_model::dev::{self, Dev, Hub, HubPort};
use xhcid_model::{machine, Config, Rig, Shared};

const MEASURED: &str = include_str!(
    "../../../../../docs/board/measurements/2026-10-04-usb-boot-protocol/stock-descriptors.txt"
);

/// The bytes after the measurement file's line `== <name>…`.
pub fn measured(name: &str) -> Vec<u8> {
    let mut lines = MEASURED.lines();
    lines.find(|l| l.starts_with(&format!("== {name}"))).expect("a measured device");
    let hex = lines.next().expect("its bytes");
    hex.split_whitespace().map(|b| u8::from_str_radix(b, 16).expect("hex")).collect()
}

/// QEMU's shape: a full-speed hub with eight ports on root port 1, a boot keyboard on its
/// port 1 and a boot mouse on its port 2 — not started.
pub fn qemu_machine() -> Shared {
    let m = machine(Config::qemu());
    let mut hub = dev::hub(Speed::Full, 8, 0);
    hub.plug(1, Dev::Hid(dev::hid(Speed::Full, dev::hid_descriptors(0x0627, 0x0001, &[(1, 8)]))));
    hub.plug(2, Dev::Hid(dev::hid(Speed::Full, dev::hid_descriptors(0x0627, 0x0001, &[(2, 4)]))));
    m.borrow_mut().plug(1, Dev::Hub(hub));
    m
}

/// QEMU's shape, started and settled.
pub fn qemu_rig() -> (Shared, Rig) {
    let m = qemu_machine();
    let mut rig = Rig::new(&m);
    rig.start();
    (m, rig)
}

/// The board as measured: the on-board high-speed hub on root port 1, the keyboard on its port
/// 2, the mouse's receiver (which STALLs SET_IDLE on its mouse interface) on its port 3.
pub fn board_rig() -> (Shared, Rig) {
    let m = machine(Config::board());
    let mut hub = Hub {
        speed: Speed::High,
        descriptors: measured("2-1 "),
        hub_descriptor: measured("the hub's class descriptor"),
        ports: vec![HubPort::default(); 5],
        configured: 0,
    };
    let keyboard = dev::hid(Speed::Full, measured("2-1.2 "));
    let mut receiver = dev::hid(Speed::Full, measured("2-1.3 "));
    receiver.stall_set_idle = vec![1];
    hub.plug(2, Dev::Hid(keyboard));
    hub.plug(3, Dev::Hid(receiver));
    m.borrow_mut().plug(1, Dev::Hub(hub));
    let mut rig = Rig::new(&m);
    rig.start();
    (m, rig)
}
