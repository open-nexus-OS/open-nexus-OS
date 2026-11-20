// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of gpud's frame timeline (`hop_trace`).

use super::*;
use nexus_ux_budgets::timeline::{LINE_CAP, VOCABULARY};

const MS: u64 = 1_000_000;
/// The fixture's frame: windowd sent `f=10` at 5 s + 10.4 ms.
const T: u64 = 5_000 * MS;
const PERIOD: u64 = 16_666_666;

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let body = line.split_once(" (")?.1.strip_suffix(')')?;
    body.split(' ').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

fn printed(
    hops: &mut GpudHops,
    f: impl FnOnce(&mut GpudHops, &mut dyn FnMut(&str)),
) -> Vec<String> {
    let mut out = Vec::new();
    f(hops, &mut |l: &str| out.push(l.to_string()));
    out
}

/// The GPU's frame: planned 1 ms after `rx`, emitted and submitted in 2 and 1 ms, done 6 ms
/// later; two glass writes, one of a whole glass.
fn gpu_frame(rx: u64) -> FrameHop {
    let split = [MS, 2 * MS, MS, 5 * MS, 100_000];
    FrameHop {
        at: FrameHop::laid(rx + MS, split, rx + 10 * MS),
        waits: (2, 5_000),
        copied: 61_440,
        passes: 3,
        draws: 41,
        gw: 2,
        whole: 1,
        wpx: 160_000,
        by: [1, 0, 0, 1, 0, 0],
    }
}

#[test]
fn a_gpu_frame_prints_its_chain_from_received_to_shown() {
    let mut hops = GpudHops::new();
    let rx = T + 10_500_000;
    hops.drawn(Some(gpu_frame(rx)));
    hops.received(10, rx, rx + 11 * MS);
    // Shown at 5 s + 55 ms: the landing windowd's ack names for `f=10`.
    let shown = rx + 44_500_000;
    assert!(!hops.landed(10, (rx + 12 * MS, shown), (true, PERIOD), shown + MS));
    let lines = printed(&mut hops, |h, p| h.idle(shown + MS + IDLE_NS, p));
    assert_eq!(lines.len(), 1, "missed its slot: slow, {lines:?}");
    let l = &lines[0];
    println!("HOPFIXTURE {l}");
    let want = [
        ("f", "10"),
        ("t0", "5010500"),
        ("rx>plan", "1000"),
        ("plan>emit", "2000"),
        ("emit>sub", "1000"),
        ("sub>gpu", "6000"),
        ("gpu>latch", "2000"),
        ("latch>shown", "32500"),
        ("w", "2/5000"),
        ("cp", "61440"),
        ("passes", "3"),
        ("draws", "41"),
        ("gw", "2"),
        ("whole", "1"),
        ("wpx", "160000"),
        ("by", "1/0/0/1/0/0"),
    ];
    for (name, value) in want {
        assert_eq!(field(l, name), Some(value), "{name} in {l}");
    }
    assert!(printed(&mut hops, |h, p| h.idle(rx + 60 * MS + IDLE_NS, p)).is_empty(), "once");
}

#[test]
fn a_cpu_frame_s_work_is_its_first_hop_and_an_untimed_ack_has_no_flip() {
    let mut hops = GpudHops::new();
    hops.drawn(None);
    hops.received(4, T, T + 3 * MS);
    assert!(!hops.landed(4, (0, 0), (false, PERIOD), T + 3 * MS));
    let r = hops.ring.find_mut(4, 0).expect("the record");
    assert_eq!(r.hop_us(RX, PLAN), Some(0));
    assert_eq!(r.hop_us(SUB, GPU), Some(3000), "the CPU's work lands where the GPU's would");
    assert_eq!((r.stamps[LATCH], r.stamps[SHOWN], r.slow), (0, 0, false));
    let line = one_line(r).expect("a line");
    assert!(field(line.as_str(), "gpu>latch").is_none() && field(line.as_str(), "by").is_some());
}

#[test]
fn a_frame_handed_over_is_taken_once_and_stamps_keep_their_order() {
    let mut hops = GpudHops::new();
    hops.drawn(Some(gpu_frame(T)));
    hops.received(1, T + 2 * MS, T + 12 * MS);
    hops.received(2, T + 13 * MS, T + 14 * MS);
    let r = hops.ring.find_mut(1, 0).expect("the record");
    assert_eq!(r.stamps[PLAN], T + 2 * MS, "planned before it was received: received");
    let r = hops.ring.find_mut(2, 0).expect("the record");
    assert_eq!((r.copied, r.counts[PASSES]), (0, 0), "the next present drew on the CPU");
}

#[test]
fn the_window_closes_at_32_landings_with_the_histogram_and_the_cost_once() {
    let mut hops = GpudHops::new();
    let mut full = 0;
    for seq in 0..WINDOW {
        let rx = T + u64::from(seq) * PERIOD;
        hops.drawn(Some(gpu_frame(rx)));
        hops.received(seq, rx, rx + 11 * MS);
        full += u32::from(hops.landed(seq, (rx + 12 * MS, rx + 16 * MS), (false, PERIOD), rx));
    }
    assert_eq!(full, 1, "the 32nd landing fills the window");
    let clock = || 0u64;
    let lines = printed(&mut hops, |h, p| h.window(&clock, p));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("gpud: hops hist (rx>plan=0/32/0/0/0/0/0/0 "), "{lines:?}");
    assert!(lines[0].ends_with("latch>shown=0/0/0/32/0/0/0/0 one=0 lost=0)"), "{lines:?}");
    assert_eq!(lines[1], "gpud: hops cost (nsec_ns=0 record_ns=0 line_us=0)");
    assert_eq!(printed(&mut hops, |h, p| h.window(&clock, p)).len(), 1, "the cost once");
}

#[test]
fn the_worst_gpud_lines_fit_the_cap_and_name_only_hop_fields() {
    let mut hops = GpudHops::new();
    let big = FrameHop {
        at: [0; 4],
        waits: (u32::MAX, u64::MAX),
        copied: u64::MAX,
        passes: u32::MAX,
        draws: u32::MAX,
        gw: u32::MAX,
        whole: u32::MAX,
        wpx: u64::MAX,
        by: [u16::MAX; 6],
    };
    hops.drawn(Some(big));
    hops.received(u32::MAX, 99_999_999_999_000, 99_999_999_999_000);
    let r = hops.ring.find_mut(u32::MAX, 0).expect("the record");
    r.yields = u32::MAX;
    for k in PLAN..=SHOWN {
        r.stamp(k, 99_999_999_999_000 + k as u64 * 2_000_000_000);
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
        for _ in 0..200 {
            for us in [0, 1_500, 3_000, 5_000, 10_000, 20_000, 40_000, 90_000] {
                h.note(us);
            }
        }
    }
    let line = hist_line(&mut hist, ONE_PER_WINDOW, u32::MAX).expect("the worst hist line");
    assert!(line.as_str().len() < LINE_CAP, "{}", line.as_str());
}
