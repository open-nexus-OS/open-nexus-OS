// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of the read floor (`present_acks.rs`, RFC-0114 §5 amended, TASK-0355 P2): gpud's
//! drawn notice frees a client's other slots before the ack lands, through the same slot ledger
//! the runtime asks (`ClientSurfaces::take_releases`). The notice is credited once, only for an
//! outstanding undrawn present, and is never a frame completion.

use super::*;
use crate::client_surface::{ClientSurfaces, SurfaceBuffer};
use nexus_display_proto::client_surface::FORMAT_BGRA8888;
use nexus_display_proto::frame_time::{encode_present_ack_shown, Shown};
use nexus_display_proto::{encode_present_ack, encode_present_drawn, STATUS_OK, STATUS_REVEALED};

/// A surface with a two-slot buffer bound, its first present (slot 0) released.
fn surface_on_slot_0(t: &mut ClientSurfaces) -> u32 {
    let id = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("creates");
    let buffer = SurfaceBuffer {
        source: id + 100,
        stride: 320 * 4,
        width: 320,
        rows: 240,
        slots: 2,
        cache_block: 0,
    };
    t.bind_buffer(id, buffer, Some(40)).expect("binds");
    t.present(id, (1, 0), &[], 0).expect("first present");
    assert_eq!(releases(t, 0), [(id, 1)], "the first present is released at once");
    id
}

fn releases(t: &mut ClientSurfaces, floor: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    t.take_releases(floor, &mut out);
    out.into_iter().map(|(id, seq, _)| (id, seq)).collect()
}

/// The runtime's order: windowd present `seq` reads the surface's newest slot (0), then the app
/// presents into slot 1 — its ack is owed until no frame reads slot 0.
fn frame_reads_slot_0_then_app_presents(
    w: &mut PresentWindow,
    t: &mut ClientSurfaces,
    id: u32,
) -> u32 {
    let seq = w.issue().expect("window has room");
    t.note_read(&[id], seq);
    t.present(id, (2, 1), &[], 0).expect("second present");
    seq
}

#[test]
fn a_drawn_present_frees_its_reads_before_its_ack() {
    let mut w = PresentWindow::new();
    let mut t = ClientSurfaces::new();
    let id = surface_on_slot_0(&mut t);
    let seq = frame_reads_slot_0_then_app_presents(&mut w, &mut t, id);
    assert_eq!(releases(&mut t, w.read_floor()), [], "frame {seq} still reads slot 0");
    assert_eq!(w.drawn(seq), Verdict::Credited);
    assert_eq!(w.in_flight(), 1, "drawn is not landed: the present is still in flight");
    assert_eq!(w.retired(), seq - 1, "the ack's floor did not move");
    assert_eq!(releases(&mut t, w.read_floor()), [(id, 2)], "released at drawn, before the ack");
    assert_eq!(w.ack(seq), Verdict::Credited, "the landed ack still credits once");
    assert_eq!(releases(&mut t, w.read_floor()), [], "released once");
    assert_eq!(w.in_flight(), 0);
}

/// A display that acks at once (the CPU path) sends no notice: the ack frees the reads.
#[test]
fn an_ack_without_a_drawn_notice_frees_its_reads() {
    let mut w = PresentWindow::new();
    let mut t = ClientSurfaces::new();
    let id = surface_on_slot_0(&mut t);
    let seq = frame_reads_slot_0_then_app_presents(&mut w, &mut t, id);
    assert_eq!(w.ack(seq), Verdict::Credited);
    assert_eq!(w.read_floor(), seq);
    assert_eq!(releases(&mut t, w.read_floor()), [(id, 2)]);
    assert_eq!(w.drawn(seq), Verdict::Duplicate, "a notice after its ack credits nothing");
}

/// A notice for a seq never issued, ahead of the last, before the reset floor or aged out of
/// the window is named, never credited — and moves no floor.
#[test]
fn test_reject_a_drawn_notice_for_no_present() {
    let mut w = PresentWindow::new();
    assert_eq!(w.drawn(1), Verdict::Unknown, "nothing issued yet");
    w.issue();
    assert_eq!(w.drawn(2), Verdict::Unknown, "ahead of the last issued");
    assert_eq!(w.drawn(0), Verdict::Unknown, "seq 0 is never issued");
    assert_eq!(w.drawn(0xC0DE_0001), Verdict::Unknown, "a cursor magic is never a present");
    assert_eq!((w.read_floor(), w.retired(), w.in_flight()), (0, 0, 1), "nothing moved");
    let mut seeded = PresentWindow::starting_after(500);
    seeded.issue();
    assert_eq!(seeded.drawn(500), Verdict::Unknown, "a predecessor's seq");
    assert_eq!(seeded.read_floor(), 500);
    let mut full = PresentWindow::new();
    for _ in 0..WINDOW {
        full.issue();
    }
    assert_eq!(full.ack(1), Verdict::Credited);
    full.issue();
    assert_eq!(full.drawn(1), Verdict::Unknown, "seq 1 aged out of the window");
}

#[test]
fn test_reject_a_duplicate_drawn_notice() {
    let mut w = PresentWindow::new();
    w.issue();
    w.issue();
    assert_eq!(w.drawn(1), Verdict::Credited);
    assert_eq!(w.drawn(1), Verdict::Duplicate);
    assert_eq!(w.read_floor(), 1, "the duplicate did not free present 2's reads");
    assert_eq!(w.in_flight(), 2, "notices never touch the in-flight gate");
}

/// After a client reset nothing in flight reads a slot: every owed release goes, and a late
/// notice for a pre-reset seq is unknown.
#[test]
fn a_reset_frees_every_read() {
    let mut w = PresentWindow::new();
    let mut t = ClientSurfaces::new();
    let id = surface_on_slot_0(&mut t);
    let seq = frame_reads_slot_0_then_app_presents(&mut w, &mut t, id);
    w.issue();
    assert_eq!(releases(&mut t, w.read_floor()), []);
    w.reset();
    assert_eq!(w.read_floor(), w.last_issued());
    assert_eq!(w.retired(), w.last_issued());
    assert_eq!(releases(&mut t, w.read_floor()), [(id, 2)]);
    assert_eq!(w.drawn(seq), Verdict::Unknown, "a pre-reset notice");
    assert_eq!(w.issue(), Some(seq + 2), "seqs climb on");
    assert_eq!(w.read_floor(), seq + 1, "the new present is undrawn");
}

/// The read floor stops before the oldest present neither drawn nor acked, stays at or above
/// the retired floor, and both only climb — out-of-order notices and acks included.
#[test]
fn the_read_floor_never_passes_an_undrawn_present() {
    const SEED: u32 = 9_000;
    let mut w = PresentWindow::starting_after(SEED);
    let mut seqs = [0u32; 5];
    for s in &mut seqs {
        *s = w.issue().expect("room");
    }
    assert_eq!(seqs, [SEED + 1, SEED + 2, SEED + 3, SEED + 4, SEED + 5]);
    assert_eq!((w.read_floor(), w.retired()), (SEED, SEED));
    // A shadow of the rule: per present, still undrawn / still outstanding.
    let mut undrawn = [true; 5];
    let mut outstanding = [true; 5];
    let floor_of = |set: &[bool; 5]| match set.iter().position(|&b| b) {
        Some(0) => SEED,
        Some(i) => seqs[i - 1],
        None => seqs[4],
    };
    let mut prev = (w.read_floor(), w.retired());
    // (drawn?, present index): notices and acks out of order, an ack without a notice.
    let steps = [(true, 2), (true, 1), (false, 3), (true, 0), (false, 0), (true, 4), (false, 2)];
    for (is_drawn, k) in steps {
        let v = if is_drawn { w.drawn(seqs[k]) } else { w.ack(seqs[k]) };
        assert_eq!(v, Verdict::Credited, "step drawn={is_drawn} k={k}");
        undrawn[k] = false;
        if !is_drawn {
            outstanding[k] = false;
        }
        let (read, retired) = (w.read_floor(), w.retired());
        assert_eq!(read, floor_of(&undrawn), "stops before the oldest undrawn present");
        assert_eq!(retired, floor_of(&outstanding), "the ack's floor, unchanged by notices");
        assert!(at_or_after(retired, read), "read floor {read} below retired {retired}");
        assert!(at_or_after(prev.0, read) && at_or_after(prev.1, retired), "monotone");
        prev = (read, retired);
    }
    assert_eq!(w.read_floor(), SEED + 5, "everything drawn: the last issued");
    assert_eq!(w.in_flight(), 2, "only acks leave the in-flight gate");
}

/// `b` is at or after `a` on the wrapping present sequence.
fn at_or_after(a: u32, b: u32) -> bool {
    b.wrapping_sub(a) < u32::MAX / 2
}

/// A pure classifier of gpud's reply: only a drawn notice is not a completion — an ack (plain,
/// with its shown time, revealed or nacked), a cursor reply and a bare status all clock frames.
#[test]
fn a_drawn_notice_is_not_a_frame_completion() {
    assert!(!is_frame_completion(&encode_present_drawn(7)));
    assert!(is_frame_completion(&encode_present_ack(STATUS_OK, 7)));
    assert!(is_frame_completion(&encode_present_ack(STATUS_REVEALED, 7)));
    let shown = Shown { at_ns: 1, period_ns: 16_666_667, cadence: 1 };
    assert!(is_frame_completion(&encode_present_ack_shown(STATUS_OK, 7, shown)));
    assert!(is_frame_completion(&[STATUS_OK]));
    let mut long = [0u8; 6];
    long[..5].copy_from_slice(&encode_present_drawn(7));
    assert!(is_frame_completion(&long), "not a notice: its length is not the notice's");
}
