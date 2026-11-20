// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for `nx hops`: the three-process join, a frame without an
//!          app-host record, the 2 s reuse window, cut lines, out-of-order stamps, class p50/p90.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 integration tests over fixtures/hops/*.txt.
//!
//! TEST_SCOPE:
//!   - stitches_a_frame_across_three_processes
//!   - a_frame_without_an_apphost_record_is_a_timeline
//!   - test_reject_joining_two_frames_by_a_reused_seq
//!   - a_line_cut_at_the_console_cap_is_reported_not_parsed
//!   - stamps_out_of_order_fail_the_check
//!   - bare_and_loaded_classes_report_p50_and_p90
//!
//! ADR: docs/rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md

use std::path::PathBuf;
use std::process::{Command, Output};

fn nx(fixture: &str, extra: &[&str]) -> Output {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hops").join(fixture);
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .arg("hops")
        .arg(path)
        .args(extra)
        .output()
        .expect("nx runs")
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn stitches_a_frame_across_three_processes() {
    let o = nx("three.txt", &["--check"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("frame f=7 class=bare"), "{t}");
    assert!(t.contains("apphost") && t.contains("s=3.11") && t.contains("raster>sent=400"), "{t}");
    assert!(t.contains("windowd") && t.contains("sent>acked=2000"), "{t}");
    assert!(
        t.contains("gpud") && t.contains("sub>gpu=4000") && t.contains("by=1/2/3/4/5/6"),
        "{t}"
    );
    assert!(t.contains("longest hop: gpud.sub>gpu=4000us"), "{t}");
    assert!(
        t.contains("gap apphost=") && t.contains("gap windowd=") && t.contains("gap gpud="),
        "{t}"
    );
    assert!(!t.contains("not slow"), "{t}");
    assert!(t.contains("frames without apphost=0"), "{t}");
}

#[test]
fn a_frame_without_an_apphost_record_is_a_timeline() {
    let o = nx("no_apphost.txt", &[]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("frame f=7"), "{t}");
    assert!(t.contains("apphost: not slow"), "{t}");
    assert!(t.contains("sub>gpu=4000"), "{t}");
}

#[test]
fn test_reject_joining_two_frames_by_a_reused_seq() {
    // f=7 appears at t0 ~1 s and ~11 s; gpud printed it only near 11 s.
    let o = nx("reused_seq.txt", &[]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("windowd without gpud=1"), "the early f=7 must not take the late gpud: {t}");
    assert!(t.contains("gpud without windowd=0"), "{t}");
    // Exactly one frame has a gpud row.
    assert_eq!(t.matches("\n  gpud ").count(), 1, "{t}");
    assert_eq!(t.matches("gpud: no record").count(), 1, "{t}");
}

#[test]
fn a_line_cut_at_the_console_cap_is_reported_not_parsed() {
    let o = nx("cut.txt", &[]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("cut lines=1"), "{t}");
    assert!(t.contains("cut: gpud"), "{t}");
    assert!(!t.contains("frame f=8"), "a cut line is never a frame: {t}");
    let c = nx("cut.txt", &["--check"]);
    assert_eq!(c.status.code(), Some(3), "{}", text(&c));
    assert!(text(&c).contains("line cut at the console cap"), "{}", text(&c));
}

#[test]
fn stamps_out_of_order_fail_the_check() {
    let o = nx("out_of_order.txt", &["--check"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(3), "{t}");
    assert!(t.contains("apphost sent") && t.contains("after windowd rx"), "{t}");
    // Without --check it only decodes.
    assert_eq!(nx("out_of_order.txt", &[]).status.code(), Some(0));
}

#[test]
fn bare_and_loaded_classes_report_p50_and_p90() {
    let o = nx("classes.txt", &[]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("class bare: 4 frame(s)"), "{t}");
    assert!(t.contains("class loaded: 2 frame(s)"), "{t}");
    let line = |class: &str| {
        let from = t.find(&format!("class {class}")).expect("class block");
        t[from..].lines().find(|l| l.contains("windowd.scene>sent")).expect("hop line").to_string()
    };
    // bare 10,20,30,40 -> p50 20, p90 40; loaded 100,200 -> p50 100, p90 200.
    assert!(line("bare").contains("p50=20us p90=40us n=4"), "{}", line("bare"));
    assert!(line("loaded").contains("p50=100us p90=200us n=2"), "{}", line("loaded"));
}
