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
    // The code names the wait (1 = ready), and the words it read last come first.
    assert_eq!(rig.sink.fails, [(Step::Controller, 1)]);
    let stuck = rig.sink.notes("ControllerStuck");
    assert_eq!(stuck.len(), 1);
    assert!(stuck[0].starts_with("ControllerStuck { phase: 1, usbcmd:"), "{}", stuck[0]);
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

/// The board's USB 3 root port trains on its own time (cycle 18: before the run phase, with the
/// hub's SuperSpeed twin on it): its change arrives before `run` — and the port is still left
/// alone, never reset or enumerated as a USB 2 device (the SuperSpeed hub STALLs the USB 2 hub
/// descriptor request). The ports' revisions are known from the start, not from the run.
#[test]
fn a_superspeed_port_that_trains_before_the_run_is_left_alone() {
    use nexus_usb::Speed;
    use xhcid_model::dev::{self, Dev};
    let m = machine(Config::board());
    let mut rig = Rig::new(&m);
    rig.xhci.start(rig.now, &mut rig.sink);
    m.borrow_mut().plug(2, Dev::Hub(dev::hub(Speed::Super, 4, 0)));
    rig.settle();
    assert_eq!(rig.sink.notes("SuperSpeedPort { port: 2 }").len(), 1, "left alone, said once");
    assert!(rig.sink.notes("Enumerated").is_empty(), "nothing enumerated on the USB 3 port");
    assert!(rig.sink.notes("Fail").is_empty());
}
