// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0068 — windowd's screen-capture machine on the host: only screencapd may
//! capture, never at the greeter, one capture at a time; a freeze takes the pointer out of the
//! frame for exactly the presents before its readback, freezes from the moment the readback is
//! sent, and ends by THAW or by its own bound; a failed or unanswered readback fails the
//! request and undoes the freeze.
//! OWNERS: @ui

use nexus_display_proto::readback::{Readback, READBACK_FREEZE};
use nexus_display_proto::surface_capture::{
    CaptureRect, CaptureRequest, CaptureWindow, CAPTURE_ATTACH, CAPTURE_BUSY, CAPTURE_DENIED,
    CAPTURE_FAILED, CAPTURE_FREEZE, CAPTURE_MALFORMED, CAPTURE_NO_DISPLAY, CAPTURE_OK,
    CAPTURE_PROBE, CAPTURE_THAW, CAPTURE_WINDOWS_MAX,
};
use windowd::capture_gate::{
    admit, capture_windows, Facts, Machine, Phase, Plan, Step, FREEZE_MAX_NS, READ_MAX_NS,
};

const MODE: (u32, u32) = (1280, 800);

fn facts() -> Facts {
    Facts {
        from_screencapd: true,
        greeter: false,
        mode: Some(MODE),
        vmo: true,
        pointer_in_frame: true,
    }
}

fn req(cmd: u8) -> CaptureRequest {
    CaptureRequest { cmd, nonce: 7, rect: CaptureRect { x: 600, y: 380, w: 64, h: 4 } }
}

const FULL: Readback = Readback { x: 0, y: 0, w: 1280, h: 800, flags: READBACK_FREEZE };

/// A freeze with the pointer drawn into the frame: hidden first, the readback waits for a
/// present composed without it, the screen is frozen from the send, the pointer is back with it.
#[test]
fn a_freeze_hides_the_pointer_for_one_present_then_freezes_from_the_send() {
    let plan = admit(&req(CAPTURE_FREEZE), &facts(), Phase::Idle).expect("admitted");
    assert_eq!(plan, Plan::Read { readback: FULL, hide_pointer: true });
    let mut m = Machine::new();
    assert_eq!(m.start(FULL, true, None, 0), Step::Wait);
    assert!(m.hide_owed(), "gpud's queue was full: the hide is owed");
    assert_eq!(m.presented(40, 1), Step::Wait, "no present counts before the hide went out");
    m.hide_sent(41);
    assert!(!m.hide_owed());
    assert!(m.pointer_hidden() && !m.frozen(), "hidden, not yet frozen");
    assert_eq!(m.presented(40, 1), Step::Wait, "a present issued before the hide does not count");
    assert_eq!(m.presented(41, 2), Step::SendReadback(FULL));
    assert!(m.frozen() && !m.pointer_hidden(), "frozen from the send; the pointer returns");
    assert_eq!(m.readback_done(CAPTURE_OK, 3), Step::Answer { status: CAPTURE_OK, thaw: false });
    assert_eq!(m.phase(), Phase::Frozen { since_ns: 3 });
    assert_eq!(admit(&req(CAPTURE_THAW), &facts(), m.phase()), Ok(Plan::Thaw));
    m.thaw();
    assert!(!m.frozen() && m.phase() == Phase::Idle);
}

/// A hardware pointer overlay is never in the frame: the readback goes out at once.
#[test]
fn a_hardware_pointer_reads_at_once() {
    let hw = Facts { pointer_in_frame: false, ..facts() };
    let plan = admit(&req(CAPTURE_FREEZE), &hw, Phase::Idle).expect("admitted");
    assert_eq!(plan, Plan::Read { readback: FULL, hide_pointer: false });
    let mut m = Machine::new();
    assert_eq!(m.start(FULL, false, None, 0), Step::SendReadback(FULL));
    assert!(m.frozen() && !m.pointer_hidden());
}

/// A probe reads its rectangle, never hides the pointer, never freezes, and is allowed while
/// the greeter owns the display (it yields a verdict to the selftest, no pixels to anyone).
#[test]
fn a_probe_reads_without_freezing_even_at_the_greeter() {
    let at_greeter = Facts { greeter: true, ..facts() };
    let probe = Readback { x: 600, y: 380, w: 64, h: 4, flags: 0 };
    let plan = admit(&req(CAPTURE_PROBE), &at_greeter, Phase::Idle).expect("admitted");
    assert_eq!(plan, Plan::Read { readback: probe, hide_pointer: false });
    let mut m = Machine::new();
    assert_eq!(m.start(probe, false, None, 0), Step::SendReadback(probe));
    assert!(!m.frozen());
    assert_eq!(m.readback_done(CAPTURE_OK, 1), Step::Answer { status: CAPTURE_OK, thaw: false });
    assert_eq!(m.phase(), Phase::Idle);
}

#[test]
fn test_reject_every_command_from_a_foreign_sender() {
    let foreign = Facts { from_screencapd: false, ..facts() };
    for cmd in [CAPTURE_FREEZE, CAPTURE_THAW, CAPTURE_PROBE, CAPTURE_ATTACH] {
        assert_eq!(admit(&req(cmd), &foreign, Phase::Idle), Err(CAPTURE_DENIED), "cmd {cmd}");
    }
}

#[test]
fn test_reject_a_freeze_while_the_greeter_owns_the_display() {
    let at_greeter = Facts { greeter: true, ..facts() };
    assert_eq!(admit(&req(CAPTURE_FREEZE), &at_greeter, Phase::Idle), Err(CAPTURE_DENIED));
}

#[test]
fn test_reject_a_second_capture_and_a_thaw_without_a_freeze() {
    let frozen = Phase::Frozen { since_ns: 0 };
    let reading = Phase::Reading { freeze: true, since_ns: 0 };
    let hiding = Phase::Hiding { seq: Some(1), readback: FULL, since_ns: 0 };
    for phase in [frozen, reading, hiding] {
        assert_eq!(admit(&req(CAPTURE_FREEZE), &facts(), phase), Err(CAPTURE_BUSY), "{phase:?}");
        assert_eq!(admit(&req(CAPTURE_PROBE), &facts(), phase), Err(CAPTURE_BUSY), "{phase:?}");
    }
    assert_eq!(admit(&req(CAPTURE_THAW), &facts(), Phase::Idle), Err(CAPTURE_BUSY));
    assert_eq!(admit(&req(CAPTURE_THAW), &facts(), reading), Err(CAPTURE_BUSY), "not yet frozen");
    assert_eq!(admit(&req(CAPTURE_ATTACH), &facts(), reading), Err(CAPTURE_BUSY), "VMO in use");
    assert_eq!(admit(&req(CAPTURE_ATTACH), &facts(), frozen), Ok(Plan::Attach));
}

#[test]
fn test_reject_probe_rectangles_outside_the_display() {
    for rect in [
        CaptureRect { x: 1270, y: 0, w: 11, h: 1 },
        CaptureRect { x: 0, y: 799, w: 1, h: 2 },
        CaptureRect { x: 0, y: 0, w: 0, h: 4 },
        CaptureRect { x: u16::MAX, y: u16::MAX, w: u16::MAX, h: u16::MAX },
    ] {
        let probe = CaptureRequest { cmd: CAPTURE_PROBE, nonce: 1, rect };
        assert_eq!(admit(&probe, &facts(), Phase::Idle), Err(CAPTURE_MALFORMED), "{rect:?}");
    }
    let unknown = CaptureRequest { cmd: 9, ..req(CAPTURE_PROBE) };
    assert_eq!(admit(&unknown, &facts(), Phase::Idle), Err(CAPTURE_MALFORMED));
}

/// Display-less is its own answer (nothing to read — a headless lane says nothing), a missing
/// frame VMO a failure.
#[test]
fn test_reject_a_capture_without_a_display_or_a_frame_vmo() {
    let headless = Facts { mode: None, ..facts() };
    let no_vmo = Facts { vmo: false, ..facts() };
    for (f, status) in [(headless, CAPTURE_NO_DISPLAY), (no_vmo, CAPTURE_FAILED)] {
        assert_eq!(admit(&req(CAPTURE_FREEZE), &f, Phase::Idle), Err(status));
        assert_eq!(admit(&req(CAPTURE_PROBE), &f, Phase::Idle), Err(status));
    }
}

/// A readback gpud refuses fails the request and undoes the freeze it began.
#[test]
fn test_reject_a_failed_readback_ends_the_freeze() {
    let mut m = Machine::new();
    let _ = m.start(FULL, false, None, 0);
    assert_eq!(m.readback_done(3, 1), Step::Answer { status: CAPTURE_FAILED, thaw: true });
    assert_eq!(m.phase(), Phase::Idle);
    assert!(!m.frozen());
    let mut sent = Machine::new();
    let _ = sent.start(FULL, false, None, 0);
    assert_eq!(sent.send_failed(), Step::Answer { status: CAPTURE_FAILED, thaw: true });
}

/// The bounds: an unanswered readback fails after `READ_MAX_NS`; a freeze nobody thaws ends
/// after `FREEZE_MAX_NS` — and not a nanosecond before.
#[test]
fn the_machine_bounds_an_unanswered_read_and_an_abandoned_freeze() {
    let mut m = Machine::new();
    let _ = m.start(FULL, true, Some(5), 100);
    assert_eq!(m.expire(100 + READ_MAX_NS - 1), Step::Wait);
    assert_eq!(m.expire(100 + READ_MAX_NS), Step::Answer { status: CAPTURE_FAILED, thaw: false });
    let mut f = Machine::new();
    let _ = f.start(FULL, false, None, 0);
    let _ = f.readback_done(CAPTURE_OK, 10);
    assert_eq!(f.expire(10 + FREEZE_MAX_NS - 1), Step::Wait);
    assert_eq!(f.expire(10 + FREEZE_MAX_NS), Step::Expired);
    assert_eq!(f.phase(), Phase::Idle);
    assert_eq!(f.readback_done(CAPTURE_OK, 11), Step::Wait, "a late answer finds nobody");
}

/// The window list: front to back, clamped into the wire, empty ones dropped, at most eight.
#[test]
fn the_window_list_is_clamped_and_bounded() {
    let (w, n) = capture_windows([(3, -40_000, 20, 640, 480), (4, 10, 10, 0, 100)]);
    assert_eq!(n, 1, "the empty window is dropped");
    assert_eq!(w[0], CaptureWindow { id: 3, x: i16::MIN, y: 20, w: 640, h: 480 });
    let many = (0..12).map(|i| (i, 0, 0, 100, 100));
    assert_eq!(capture_windows(many).1, CAPTURE_WINDOWS_MAX);
}
