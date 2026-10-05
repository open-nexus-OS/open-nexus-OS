// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use xhcid::Step;
use xhcid_model::{machine, Config, Faults, Rig};

use super::{board_rig, qemu_rig};

#[test]
fn qemu_controller_comes_up() {
    let (_m, rig) = qemu_rig();
    assert!(rig.xhci.running());
    assert_eq!(
        rig.sink.notes("ControllerOk"),
        ["ControllerOk { version: 256, ports: 8, slots: 64, context_size: 32, scratchpads: 0 }"]
    );
    assert_eq!(rig.sink.notes("Ready"), ["Ready { ports: 8, connected: 1 }"]);
    assert!(rig.sink.fails.is_empty(), "{:?}", rig.sink.fails);
}

#[test]
fn board_controller_comes_up_with_its_scratchpad_and_powered_ports() {
    let (m, rig) = board_rig();
    assert_eq!(
        rig.sink.notes("ControllerOk"),
        ["ControllerOk { version: 272, ports: 2, slots: 64, context_size: 64, scratchpads: 1 }"]
    );
    // Port power control: the ports were powered by the driver, after the run.
    assert!(m.borrow().ports.iter().all(|p| p.pp));
    assert_eq!(rig.sink.notes("Ready"), ["Ready { ports: 2, connected: 1 }"]);
}

#[test]
fn the_bring_up_waits_are_timer_paced_and_bounded() {
    let m = machine(Config::qemu());
    m.borrow_mut().hc.faults = Faults { not_ready_reads: 5, reset_reads: 3 };
    let mut rig = Rig::new(&m);
    rig.start();
    assert!(rig.xhci.running());
    assert!(rig.now >= 3_000_000, "re-read at 1 ms steps, not spun: {} ns", rig.now);
    // A controller that never becomes ready is given up after a second, not waited on forever.
    let m = machine(Config::qemu());
    m.borrow_mut().hc.faults = Faults { not_ready_reads: u32::MAX, reset_reads: 0 };
    let mut rig = Rig::new(&m);
    rig.start();
    assert!(rig.xhci.failed());
    assert_eq!(rig.sink.fails, [(Step::Controller, 0)]);
    assert!((1_000_000_000..1_100_000_000).contains(&rig.now), "{} ns", rig.now);
}

#[test]
fn an_idle_bus_costs_nothing() {
    let (m, mut rig) = qemu_rig();
    // Settled: nothing waits — no deadline, no interrupt — so the OS loop sleeps.
    assert_eq!(rig.xhci.deadline(), None);
    let before = m.borrow().hc.accesses;
    rig.settle();
    assert_eq!(m.borrow().hc.accesses, before, "not one register access while idle");
}
