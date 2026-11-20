// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of windowd's frame timeline (`hop_trace`): the seq map, the lines, the printing.

use super::*;
use nexus_ux_budgets::timeline::VOCABULARY;

const MS: u64 = 1_000_000;
/// The fixture's frame (app-host `s=7.41` sent at 5 s + 9.6 ms, gpud `f=10` received after).
const T: u64 = 5_000 * MS;

fn lines_of(
    trace: &mut HopTrace,
    f: impl FnOnce(&mut HopTrace, &mut dyn FnMut(&str)),
) -> Vec<String> {
    let mut out = Vec::new();
    f(trace, &mut |l: &str| out.push(l.to_string()));
    out
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let body = line.split_once(" (")?.1.strip_suffix(')')?;
    body.split(' ').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

/// A frame of client presents `from`, owed at `owed`, sent as `f` and shown `shown_after` its
/// send — slow when `shown_after` is past two steps and a half period.
fn frame(trace: &mut HopTrace, f: u32, from: &[(u32, u32, u64)], owed: u64, shown_after: u64) {
    for &(s, q, rx) in from {
        trace.client_present(s, q, rx);
    }
    trace.scene(owed + MS, 24, 3);
    trace.sent(f, Some(owed), owed + 2 * MS, 4096);
    let sent = owed + 2 * MS;
    let p = 16_667;
    trace.acked(f, sent + 3 * MS, (true, Some((sent + shown_after, p, p))), None);
}

#[test]
fn a_present_is_folded_into_two_frames_when_gpud_refuses_the_first() {
    let mut trace = HopTrace::new();
    trace.client_present(7, 41, T + 10 * MS);
    trace.scene(T + 11 * MS, 20, 2);
    trace.sent(10, Some(T + 10 * MS), T + 12 * MS, 0);
    // gpud refused 10: the next frame draws it again and names its presents too.
    trace.acked(10, T + 20 * MS, (false, None), None);
    trace.client_present(8, 3, T + 21 * MS);
    trace.sent(11, Some(T + 21 * MS), T + 22 * MS, 0);
    let ten = trace.ring.find_mut(10, 0).and_then(|r| one_line(r));
    let eleven = trace.ring.find_mut(11, 0).and_then(|r| one_line(r));
    assert_eq!(ten.as_ref().and_then(|l| field(l.as_str(), "from")), Some("7.41"));
    assert_eq!(eleven.as_ref().and_then(|l| field(l.as_str(), "from")), Some("7.41,8.3"));
    // 11 refused too: 12 names 11's presents, each once.
    trace.acked(11, T + 30 * MS, (false, None), None);
    trace.sent(12, None, T + 31 * MS, 0);
    let twelve = trace.ring.find_mut(12, 0).and_then(|r| one_line(r));
    assert_eq!(twelve.as_ref().and_then(|l| field(l.as_str(), "from")), Some("7.41,8.3"));
}

#[test]
fn more_clients_than_named_ids_are_counted_and_a_repeat_is_folded_once() {
    let mut trace = HopTrace::new();
    for s in 1..=6u32 {
        trace.client_present(s, 100 + s, T + u64::from(s) * MS);
    }
    trace.client_present(2, 102, T + 7 * MS);
    trace.sent(5, Some(T), T + 8 * MS, 0);
    let line = trace.ring.find_mut(5, 0).and_then(|r| one_line(r)).expect("a line");
    assert_eq!(field(line.as_str(), "from"), Some("1.101,2.102,3.103,4.104+2"));
    // t0 is the newest present's arrival: every app-host record it names was sent before it.
    assert_eq!(field(line.as_str(), "t0"), Some("5006000"));
    assert_eq!(field(line.as_str(), "rx>owed"), Some("0"), "owed before its newest present");
}

#[test]
fn the_worst_windowd_line_fits_the_cap_and_names_only_hop_fields() {
    let mut trace = HopTrace::new();
    for s in 0..6u32 {
        trace.client_present(u32::MAX - s, u32::MAX, 99_999_999_999_000);
    }
    trace.scene(99_999_999_999_000, 999, 999);
    trace.sent(u32::MAX, Some(1), 99_999_999_999_000 + 2_000_000_000, 9_999_999);
    let r = trace.ring.find_mut(u32::MAX, 0).expect("the record");
    (r.waits, r.wait_us, r.yields) = (u32::MAX, u64::MAX, u32::MAX);
    r.stamp(ACKED, u64::MAX / 2);
    r.stamp(SHOWN, 99_999_999_999_999);
    let line = one_line(r).expect("the worst line is never cut");
    assert!(line.as_str().len() < LINE_CAP, "{}", line.as_str());
    let from = field(line.as_str(), "from").expect("from");
    assert!(from.contains(&format!("{}.{}", u32::MAX, u32::MAX)) && from.contains('+'), "{from}");
    let body = line.as_str().split_once(" (").map(|(_, b)| b.trim_end_matches(')')).unwrap_or("");
    for kv in body.split(' ') {
        let key = kv.split_once('=').map_or(kv, |(k, _)| k);
        assert!(VOCABULARY.contains(&key), "{key} is no hop field");
    }
    let mut hist = [Hist::default(); HOPS.len()];
    for h in &mut hist {
        for us in [0, 1_500, 3_000, 5_000, 10_000, 20_000, 40_000, 90_000] {
            for _ in 0..20_000 {
                h.note(us);
            }
        }
    }
    let line = hist_line(&mut hist, ONE_PER_WINDOW, u32::MAX).expect("the worst hist line");
    assert!(line.as_str().len() < LINE_CAP, "{}", line.as_str());
}

#[test]
fn a_slow_frame_prints_once_when_idle_and_the_window_prints_hist_and_cost() {
    let mut trace = HopTrace::new();
    frame(&mut trace, 1, &[(3, 9, T)], T + MS, 5 * MS);
    frame(&mut trace, 2, &[(3, 10, T + 20 * MS)], T + 21 * MS, 60 * MS);
    let landed = T + 21 * MS + 5 * MS;
    let clock = || 0u64;
    assert!(lines_of(&mut trace, |t, p| t.idle(landed + MS, 0, p)).is_empty(), "not idle yet");
    assert!(lines_of(&mut trace, |t, p| t.idle(landed + IDLE_NS, 1, p)).is_empty(), "in flight");
    let one = lines_of(&mut trace, |t, p| t.idle(landed + IDLE_NS, 0, p));
    assert_eq!(one.len(), 1, "{one:?}");
    assert_eq!(field(&one[0], "f"), Some("2"));
    assert_eq!(field(&one[0], "sent>acked"), Some("3000"));
    let window = lines_of(&mut trace, |t, p| t.window(&clock, p));
    assert_eq!(window.len(), 2, "printed once: hist and the cost line, {window:?}");
    assert!(window[0].starts_with("windowd: hops hist (rx>owed=0/2/0/0/0/0/0/0 "), "{window:?}");
    assert!(window[0].ends_with(" one=1 lost=0)"), "{window:?}");
    assert_eq!(window[1], "windowd: hops cost (nsec_ns=0 record_ns=0 line_us=0)");
    let again = lines_of(&mut trace, |t, p| t.window(&clock, p));
    assert_eq!(again.len(), 1, "the cost line once: {again:?}");
}

#[test]
fn the_loop_s_wait_for_a_slot_is_the_owed_frame_s() {
    let mut trace = HopTrace::new();
    trace.sent(1, Some(T), T + MS, 0);
    trace.sent(2, Some(T + 2 * MS), T + 3 * MS, 0);
    // Frame 3 owed at T + 4 ms; two acks come before the ring has a slot.
    trace.acked(1, T + 9 * MS, (true, None), Some(T + 4 * MS));
    trace.acked(2, T + 12 * MS, (true, None), Some(T + 4 * MS));
    trace.sent(3, Some(T + 4 * MS), T + 13 * MS, 0);
    let line = trace.ring.find_mut(3, 0).and_then(|r| one_line(r)).expect("a line");
    assert_eq!(field(line.as_str(), "w"), Some("2/8000"));
    assert_eq!(field(line.as_str(), "t0"), Some("5004000"), "owed: no client present");
    assert!(field(line.as_str(), "from").is_none());
}

/// The fixture `nx hops --check` joins (with app-host's `s=7.41` and gpud's `f=10`).
#[test]
fn the_fixture_frame_prints_as_nx_hops_reads_it() {
    let mut trace = HopTrace::new();
    trace.client_present(7, 41, T + 9_700_000);
    trace.scene(T + 10 * MS, 24, 3);
    trace.sent(10, Some(T + 9_800_000), T + 10_400_000, 8192);
    let p = 16_667;
    trace.acked(10, T + 56 * MS, (true, Some((T + 55 * MS, p, p))), None);
    let r = trace.ring.find_mut(10, 0).expect("the record");
    assert!(r.slow, "shown 44.6 ms after it was sent");
    let line = one_line(r).expect("a line");
    println!("HOPFIXTURE {}", line.as_str());
    assert_eq!(field(line.as_str(), "from"), Some("7.41"));
}
