// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of app-host's frame timeline (`hop_record`).

use super::*;
use nexus_ux_budgets::timeline::{LINE_CAP, VOCABULARY};

const MS: u64 = 1_000_000;
/// The fixture's frame: windowd received `s=7.41` at 5 s + 9.7 ms.
const T: u64 = 5_000 * MS;

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let body = line.split_once(" (")?.1.strip_suffix(')')?;
    body.split(' ').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

fn printed(hops: &mut AppHops, f: impl FnOnce(&mut AppHops, &mut dyn FnMut(&str))) -> Vec<String> {
    let mut out = Vec::new();
    f(hops, &mut |l: &str| out.push(l.to_string()));
    out
}

/// The input received at `rx`, dispatched, laid out, rastered and sent at `sent`.
fn input_frame(hops: &mut AppHops, seq: u32, rx: u64, sent: u64) {
    hops.dispatched(rx + MS, false);
    hops.laid_out(rx + 2 * MS);
    hops.frame_begin(rx + 2 * MS + 500_000);
    hops.painted(100, 80_000);
    hops.rastered(sent - 1_600_000);
    hops.copied(4096);
    hops.sent((7, seq), sent, Some(rx));
}

fn one_of(hops: &mut AppHops) -> Vec<String> {
    let clock = || 0u64;
    printed(hops, |h, p| h.window(&clock, p)).into_iter().filter(|l| l.contains("one (")).collect()
}

#[test]
fn a_frame_answering_an_input_prints_its_chain_in_order() {
    let mut hops = AppHops::new();
    input_frame(&mut hops, 41, T - 10 * MS, T + 9_600_000);
    let lines = one_of(&mut hops);
    assert_eq!(lines.len(), 1, "19.6 ms from the input to the present: slow");
    assert_eq!(hops.idle_deadline(), None, "printed at the window: nothing left to wake for");
    let l = &lines[0];
    println!("HOPFIXTURE {l}");
    assert_eq!(field(l, "s"), Some("7.41"));
    assert_eq!(field(l, "t0"), Some("4990000"));
    assert_eq!(field(l, "in>disp"), Some("1000"));
    assert_eq!(field(l, "disp>layout"), Some("1000"));
    assert_eq!(field(l, "layout>raster"), Some("16000"));
    assert_eq!(field(l, "raster>sent"), Some("1600"));
    assert_eq!(
        (field(l, "cp"), field(l, "rows"), field(l, "px")),
        (Some("4096"), Some("100"), Some("80000"))
    );
}

#[test]
fn a_frame_within_a_period_or_without_an_input_is_not_slow() {
    let mut hops = AppHops::new();
    input_frame(&mut hops, 1, T, T + 9 * MS);
    // A pulse's frame: nothing dispatched, its raster is its frame.
    hops.frame_begin(T + 20 * MS);
    hops.rastered(T + 50 * MS);
    hops.sent((7, 2), T + 51 * MS, None);
    assert!(one_of(&mut hops).is_empty());
    let r = hops.ring.find_mut(2, 7).expect("the record");
    assert_eq!(r.stamps[IN], T + 20 * MS, "t0: the frame's first stamp");
    let line = one_line(r).expect("a line");
    assert!(field(line.as_str(), "in>disp").is_none(), "no input: no in>disp");
    assert_eq!(field(line.as_str(), "layout>raster"), Some("30000"));
}

#[test]
fn a_stale_stamp_takes_its_predecessor_s_time_and_nothing_shown_resets() {
    let mut hops = AppHops::new();
    hops.dispatched(T, true);
    hops.nothing_shown();
    hops.rastered(T + 3 * MS);
    hops.sent((7, 3), T + 4 * MS, Some(T + 2 * MS));
    let r = hops.ring.find_mut(3, 7).expect("the record");
    // The input came after the dispatch that showed nothing: its frame starts at the input.
    assert_eq!(r.stamps[..=SENT], [T + 2 * MS, T + 2 * MS, T + 2 * MS, T + 3 * MS, T + 4 * MS]);
}

#[test]
fn work_that_waited_for_the_previous_present_s_ack_is_a_wait() {
    let mut hops = AppHops::new();
    hops.sent((7, 4), T, None);
    hops.dispatched(T + MS, true);
    hops.acked(T + 5 * MS, Some(T + 500_000));
    hops.rastered(T + 6 * MS);
    hops.sent((7, 5), T + 7 * MS, Some(T + 500_000));
    let r = hops.ring.find_mut(5, 7).expect("the record");
    assert_eq!((r.waits, r.wait_us), (1, 4500), "from the input that waited to the ack");
    hops.acked(T + 8 * MS, None);
    hops.sent((7, 6), T + 9 * MS, None);
    let r = hops.ring.find_mut(6, 7).expect("the record");
    assert_eq!(r.waits, 0, "nothing waited for that ack");
}

#[test]
fn a_slow_frame_prints_once_after_a_tenth_of_a_second_idle() {
    let mut hops = AppHops::new();
    assert_eq!(hops.idle_deadline(), None);
    input_frame(&mut hops, 7, T - 20 * MS, T);
    assert_eq!(hops.idle_deadline(), Some(T + IDLE_NS));
    assert!(printed(&mut hops, |h, p| h.idle_fired(T + 50 * MS, p)).is_empty(), "not due");
    hops.sent((7, 8), T + 80 * MS, None);
    assert_eq!(hops.idle_deadline(), Some(T + IDLE_NS), "presents do not move the deadline");
    assert!(printed(&mut hops, |h, p| h.idle_fired(T + IDLE_NS, p)).is_empty(), "busy");
    assert_eq!(hops.idle_deadline(), Some(T + 80 * MS + IDLE_NS), "moved once");
    let lines = printed(&mut hops, |h, p| h.idle_fired(T + 80 * MS + IDLE_NS, p));
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(field(&lines[0], "s"), Some("7.7"));
    assert_eq!(hops.idle_deadline(), None);
    assert!(one_of(&mut hops).is_empty(), "printed once");
}

#[test]
fn the_window_prints_the_histogram_and_the_cost_once() {
    let mut hops = AppHops::new();
    input_frame(&mut hops, 1, T, T + 3 * MS);
    let clock = || 0u64;
    let lines = printed(&mut hops, |h, p| h.window(&clock, p));
    assert_eq!(
        lines,
        [
            "apphost: hops hist (in>disp=0/1/0/0/0/0/0/0 disp>layout=0/1/0/0/0/0/0/0 \
             layout>raster=1/0/0/0/0/0/0/0 raster>sent=0/1/0/0/0/0/0/0 one=0 lost=0)",
            "apphost: hops cost (nsec_ns=0 record_ns=0 line_us=0)",
        ]
    );
    assert_eq!(printed(&mut hops, |h, p| h.window(&clock, p)).len(), 1, "the cost once");
}

#[test]
fn the_worst_apphost_lines_fit_the_cap_and_name_only_hop_fields() {
    let mut hops = AppHops::new();
    hops.painted(u32::MAX, u64::MAX);
    hops.copied(u64::MAX / 2);
    hops.sent((u32::MAX, u32::MAX), 99_999_999_999_000, Some(1));
    let r = hops.ring.find_mut(u32::MAX, u32::MAX).expect("the record");
    (r.waits, r.wait_us, r.yields) = (u32::MAX, u64::MAX, u32::MAX);
    r.counts[ROWS] = 9_999;
    (r.copied, r.counts[PX]) = (99_999_999, 99_999_999);
    // t0 in eleven digits of µs (~25 h), every hop a stall printed saturated.
    for k in IN..=SENT {
        r.stamp(k, 90_000_000_000_000 + k as u64 * 2_000_000_000);
    }
    let line = one_line(r).expect("the worst line is never cut");
    assert!(line.as_str().len() < LINE_CAP, "{}", line.as_str());
    let body = line.as_str().split_once(" (").map(|(_, b)| b.trim_end_matches(')')).unwrap_or("");
    for kv in body.split(' ') {
        let key = kv.split_once('=').map_or(kv, |(k, _)| k);
        assert!(VOCABULARY.contains(&key), "{key} is no hop field");
    }
    let mut hist = [Hist::default(); HOPS.len()];
    for h in &mut hist {
        for _ in 0..1_000 {
            for us in [0, 1_500, 3_000, 5_000, 10_000, 20_000, 40_000, 90_000] {
                h.note(us);
            }
        }
    }
    let line = hist_line(&mut hist, ONE_PER_WINDOW, u32::MAX).expect("the worst hist line");
    assert!(line.as_str().len() < LINE_CAP, "{}", line.as_str());
}
