// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host tests of the surface table (`client_surface.rs`): the bounds, the buffer a surface takes,
//! the present's checks and the release rule (RFC-0114 §5, `surface_release_*`).

use super::*;

fn rect(x: u16, y: u16, w: u16, h: u16) -> DamageRect {
    DamageRect { x, y, width: w, height: h }
}

/// A two-slot buffer holding `w`×`rows` (gpud's size class rounds up; this is exact).
fn buffer(source: u32, w: u16, rows: u16) -> SurfaceBuffer {
    SurfaceBuffer { source, stride: u32::from(w) * 4, width: w, rows, slots: 2, cache_block: 0 }
}

/// A surface with its buffer bound, ready to present.
fn bound(t: &mut ClientSurfaces, w: u16, h: u16, rows: u16, store_w: u16) -> u32 {
    let id = t.create(w, h, (rows, store_w), FORMAT_BGRA8888).expect("creates");
    t.bind_buffer(id, buffer(id + 100, w, rows), Some(40)).expect("binds");
    id
}

fn releases(t: &mut ClientSurfaces, retired: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    t.take_releases(retired, &mut out);
    out.into_iter().map(|(id, seq, _)| (id, seq)).collect()
}

#[test]
fn create_validates_format_and_bounds_and_the_table_grows() {
    let mut t = ClientSurfaces::new();
    assert_eq!(t.create(320, 240, (240, 0), 9), Err(SURFACE_STATUS_MALFORMED));
    assert_eq!(t.create(8, 240, (240, 0), FORMAT_BGRA8888), Err(SURFACE_STATUS_MALFORMED));
    assert_eq!(t.create(4096, 240, (240, 0), FORMAT_BGRA8888), Err(SURFACE_STATUS_MALFORMED));
    // No slot count: the table grows with the surfaces (RFC-0114 §7).
    for _ in 0..64 {
        assert!(t.create(320, 240, (240, 0), FORMAT_BGRA8888).is_ok());
    }
}

#[test]
fn ids_are_never_zero_and_never_reused() {
    let mut t = ClientSurfaces::new();
    let a = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("a");
    let b = t.create(200, 100, (100, 0), FORMAT_BGRA8888).expect("b");
    assert!(a != 0 && b != 0 && a != b);
    assert_eq!(t.destroy(a).map(|s| s.id), Ok(a));
    assert!(t.get_by_id(a).is_none() && t.get_by_id(b).is_some());
    let c = t.create(64, 64, (64, 0), FORMAT_BGRA8888).expect("c");
    assert!(c != a && c != b, "a destroyed id is not recycled");
    assert_eq!(t.destroy(a).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
}

/// RFC-0114 §1: the buffer holds the surface — its width and every row of base and store —
/// in one to `SLOTS_MAX` slots; a surface takes one buffer, a retiring one none.
#[test]
fn test_reject_a_buffer_that_does_not_hold_the_surface() {
    let mut t = ClientSurfaces::new();
    let id = t.create(320, 240, (400, 100), FORMAT_BGRA8888).expect("creates");
    let bad = Err(SURFACE_STATUS_MALFORMED);
    assert_eq!(t.bind_buffer(id, buffer(9, 319, 400), None), bad, "narrower");
    assert_eq!(t.bind_buffer(id, buffer(9, 320, 399), None), bad, "fewer rows");
    assert_eq!(t.bind_buffer(id, buffer(0, 320, 400), None), bad, "source 0 is the layout");
    let three = SurfaceBuffer { slots: SLOTS_MAX + 1, ..buffer(9, 320, 400) };
    assert_eq!(t.bind_buffer(id, three, None), bad, "too many slots");
    let thin = SurfaceBuffer { stride: 1000, ..buffer(9, 320, 400) };
    assert_eq!(t.bind_buffer(id, thin, None), bad, "a stride below the width");
    assert_eq!(t.bind_buffer(id, buffer(9, 384, 448), Some(7)), Ok(()), "a larger class fits");
    assert_eq!(t.bind_buffer(id, buffer(10, 384, 448), None), Err(SURFACE_STATUS_BAD_SURFACE));
    assert_eq!(t.owed_caps().collect::<Vec<_>>(), [id], "its client does not hold it yet");
    assert_eq!(t.take_buffer_cap(id), Some(7));
    assert_eq!(t.take_buffer_cap(id), None, "the capability moves once");
    assert_eq!(t.owed_caps().count(), 0);
}

#[test]
fn test_reject_a_present_before_the_buffer() {
    let mut t = ClientSurfaces::new();
    let id = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("creates");
    assert_eq!(t.present(id, (1, 0), &[], 0).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
}

#[test]
fn test_reject_present_slot_out_of_range() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    assert_eq!(t.present(id, (1, 2), &[], 0).unwrap_err(), SURFACE_STATUS_MALFORMED);
    assert!(t.present(id, (1, 1), &[], 0).is_ok());
}

#[test]
fn present_enforces_strict_seq_and_known_surfaces() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    assert_eq!(t.present(id, (2, 0), &[], 0).unwrap_err(), SURFACE_STATUS_BAD_SEQ);
    assert!(t.present(id, (1, 0), &[], 0).is_ok());
    assert_eq!(t.present(id, (1, 1), &[], 0).unwrap_err(), SURFACE_STATUS_BAD_SEQ, "a replay");
    assert!(t.present(id, (2, 1), &[], 0).is_ok());
    assert_eq!(t.present(id + 1, (1, 0), &[], 0).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
}

#[test]
fn damage_is_clamped_to_the_surface_and_store_rows_are_kept() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 100, 50, 400, 100);
    let damage = [
        rect(90, 40, 50, 50),  // overhangs the width → 10 wide
        rect(200, 0, 5, 5),    // fully outside → dropped
        rect(0, 120, 100, 64), // a band's rows below the base (RFC-0109)
        rect(0, 390, 100, 64), // clamped to the last row
    ];
    let (_, rects, count) = t.present(id, (1, 0), &damage, 0).expect("presents");
    assert_eq!(
        &rects[..count],
        &[rect(90, 40, 10, 50), rect(0, 120, 100, 64), rect(0, 390, 100, 10)]
    );
}

#[test]
fn test_reject_a_surface_whose_rows_are_below_its_height_or_past_the_cap() {
    let mut t = ClientSurfaces::new();
    assert_eq!(t.create(100, 50, (49, 0), FORMAT_BGRA8888), Err(SURFACE_STATUS_MALFORMED));
    let cap = nexus_display_proto::surface_layers::MAX_SURFACE_ROWS;
    assert_eq!(t.create(100, 50, (cap + 1, 100), FORMAT_BGRA8888), Err(SURFACE_STATUS_MALFORMED));
    assert!(t.create(100, 50, (cap, 100), FORMAT_BGRA8888).is_ok());
}

/// RFC-0109: the store lies inside the surface's columns and exists exactly when the surface
/// has rows for it — the wire's rule, held again where the surface is kept.
#[test]
fn test_reject_a_store_wider_than_the_surface_or_without_rows() {
    let mut t = ClientSurfaces::new();
    let bad = Err(SURFACE_STATUS_MALFORMED);
    assert_eq!(t.create(100, 50, (400, 101), FORMAT_BGRA8888), bad, "wider");
    assert_eq!(t.create(100, 50, (400, 0), FORMAT_BGRA8888), bad, "rows, no store");
    assert_eq!(t.create(100, 50, (50, 16), FORMAT_BGRA8888), bad, "store, no rows");
}

/// What a frame draws: nothing before the first present, then the newest present's slot with
/// its seq as the content's generation.
#[test]
fn a_surface_shows_its_newest_present() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    assert_eq!(t.get_by_id(id).and_then(ClientSurface::shown), None);
    t.present(id, (1, 0), &[], 0).expect("1");
    t.present(id, (2, 1), &[], 0).expect("2");
    let shown = t.get_by_id(id).and_then(ClientSurface::shown).expect("shown");
    assert_eq!((shown.buffer.source, shown.slot, shown.generation), (id + 100, 1, 2));
}

/// RFC-0114 §5: the first present's ack is due at once — no frame ever read the other slot.
#[test]
fn surface_release_first_present_at_once() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    t.present(id, (1, 0), &[], 0).expect("presents");
    assert_eq!(releases(&mut t, 0), [(id, 1)]);
    assert_eq!(releases(&mut t, 0), [], "owed once");
}

/// The ack of a present into slot 1 waits while a frame in flight still reads slot 0 — the
/// app would draw into it next — and is due the moment that frame retired.
#[test]
fn surface_release_waits_for_the_frames_reading_the_other_slot() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    t.present(id, (1, 0), &[], 0).expect("1");
    let _ = releases(&mut t, 0);
    t.note_read(&[id], 7); // frame 7 reads slot 0
    t.note_read(&[id], 8); // and frame 8
    t.present(id, (2, 1), &[], 0).expect("2");
    assert_eq!(releases(&mut t, 6), [], "frames 7 and 8 in flight");
    assert_eq!(releases(&mut t, 7), [], "frame 8 still reads slot 0");
    assert_eq!(releases(&mut t, 8), [(id, 2)]);
    // The newest slot is never what the app draws next: frames reading it do not hold the ack.
    t.note_read(&[id], 9); // frame 9 reads slot 1
    t.present(id, (3, 0), &[], 0).expect("3");
    assert_eq!(releases(&mut t, 8), [], "frame 9 reads slot 1, the app's next");
    assert_eq!(releases(&mut t, 9), [(id, 3)]);
}

/// A frame reads only the surfaces it named; another surface's frames hold nothing here.
#[test]
fn surface_release_is_per_surface() {
    let mut t = ClientSurfaces::new();
    let a = bound(&mut t, 320, 240, 240, 0);
    let b = bound(&mut t, 320, 240, 240, 0);
    t.present(a, (1, 0), &[], 0).expect("a1");
    t.present(b, (1, 0), &[], 0).expect("b1");
    let _ = releases(&mut t, 0);
    t.note_read(&[a], 3);
    t.present(a, (2, 1), &[], 0).expect("a2");
    t.present(b, (2, 1), &[], 0).expect("b2");
    assert_eq!(releases(&mut t, 2), [(b, 2)], "frame 3 read a, never b");
    assert_eq!(releases(&mut t, 3), [(a, 2)]);
}

/// A destroy takes the owed ack with it (the client re-creates or leaves) and refuses every
/// later present; the surface stays for its last frame until the runtime destroys it.
#[test]
fn test_reject_a_present_to_a_retiring_surface() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    t.present(id, (1, 0), &[], 0).expect("1");
    t.note_read(&[id], 5);
    t.present(id, (2, 1), &[], 0).expect("2");
    assert_eq!(t.retire(id), Ok(()));
    assert_eq!(t.retire(id), Err(SURFACE_STATUS_BAD_SURFACE), "once");
    assert_eq!(releases(&mut t, 5), [], "no ack after the destroy");
    assert_eq!(t.present(id, (3, 0), &[], 0).unwrap_err(), SURFACE_STATUS_BAD_SURFACE);
    let shown = t.get_by_id(id).and_then(ClientSurface::shown).expect("its last frame");
    assert_eq!(shown.slot, 1);
    assert_eq!(t.bind_buffer(id, buffer(9, 320, 240), None), Err(SURFACE_STATUS_BAD_SURFACE));
}

/// The present sequence wraps: a frame just past the wrap is read until it retired.
#[test]
fn surface_release_across_the_sequence_wrap() {
    let mut t = ClientSurfaces::new();
    let id = bound(&mut t, 320, 240, 240, 0);
    t.present(id, (1, 0), &[], 0).expect("1");
    let _ = releases(&mut t, u32::MAX - 1);
    t.note_read(&[id], 1); // the frame after u32::MAX
    t.present(id, (2, 1), &[], 0).expect("2");
    assert_eq!(releases(&mut t, u32::MAX), [], "frame 1 comes after u32::MAX");
    assert_eq!(releases(&mut t, 1), [(id, 2)]);
}

/// RFC-0112: a table seeded past its predecessor's ids hands out none of them (and never 0).
#[test]
fn a_seeded_table_never_hands_out_an_earlier_id() {
    let mut t = ClientSurfaces::starting_at(5000);
    let id = t.create(64, 64, (64, 0), FORMAT_BGRA8888).expect("created");
    assert_eq!(id, 5000);
    let mut zero = ClientSurfaces::starting_at(0);
    assert_eq!(zero.create(64, 64, (64, 0), FORMAT_BGRA8888), Ok(1), "never id 0");
}

/// RFC-0112: a compositor restarted late seeds the table past `u16`; the ids it hands out are
/// found again whole, and the counter never falls back to 1 while it climbs.
#[test]
fn a_table_seeded_past_u16_hands_out_and_finds_wide_ids() {
    let mut t = ClientSurfaces::starting_at(0x1_2D05);
    let a = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("a");
    let b = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("b");
    assert_eq!((a, b), (0x1_2D05, 0x1_2D06));
    assert!(t.get_by_id(a).is_some() && t.get_by_id(b).is_some());
    assert!(t.get_by_id(a & 0xFFFF).is_none(), "the low half alone names no surface");
    t.bind_buffer(a, buffer(0x7_0000, 320, 240), Some(40)).expect("binds a wide source");
    assert_eq!(t.get_by_id(a).and_then(|s| s.shown()), None, "nothing shown before a present");
    let mut t = ClientSurfaces::starting_at(u32::MAX);
    let last = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("last");
    let wrapped = t.create(320, 240, (240, 0), FORMAT_BGRA8888).expect("wrapped");
    assert_eq!((last, wrapped), (u32::MAX, 1), "a wrap skips 0");
}

/// RFC-0112 Phase 3: a ghost shows its adopted slot at once, presents no more and owes no ack;
/// one past its buffer, of no slot, or too small is refused.
#[test]
fn a_ghost_shows_its_adopted_slot_and_takes_no_present() {
    let mut t = ClientSurfaces::starting_at(700);
    let b = buffer(0x1_0007, 1472, 2432);
    let id = t.adopt((1440, 900), b, 1).expect("adopted");
    let s = t.get_by_id(id).expect("live");
    assert!(s.retiring && s.buffer_cap.is_none());
    assert_eq!((s.width, s.height, s.rows, s.store_w), (1440, 900, 900, 0));
    let shown = s.shown().expect("shown at once");
    assert_eq!((shown.buffer.source, shown.slot, shown.generation), (0x1_0007, 1, 1));
    let now = 5;
    assert_eq!(t.present(id, (2, 0), &[], now), Err(SURFACE_STATUS_BAD_SURFACE), "no present");
    let mut due = Vec::new();
    t.take_releases(u32::MAX, &mut due);
    assert!(due.is_empty(), "nothing owed");
    assert!(t.bind_buffer(id, b, None).is_err(), "it has its buffer");
    assert!(t.destroy(id).is_ok());
}

#[test]
fn test_reject_a_ghost_past_its_buffer_or_of_no_slot() {
    let mut t = ClientSurfaces::starting_at(700);
    let b = buffer(0x1_0007, 1472, 2432);
    assert_eq!(t.adopt((1473, 900), b, 1), Err(SURFACE_STATUS_MALFORMED), "wider than the buffer");
    assert_eq!(
        t.adopt((1440, 2433), b, 1),
        Err(SURFACE_STATUS_MALFORMED),
        "more rows than the buffer"
    );
    assert_eq!(t.adopt((1440, 900), b, 2), Err(SURFACE_STATUS_MALFORMED), "slot 2 of two");
    let zero = SurfaceBuffer { source: 0, ..b };
    assert_eq!(t.adopt((1440, 900), zero, 0), Err(SURFACE_STATUS_MALFORMED), "no object");
    assert_eq!(t.adopt((8, 8), b, 0), Err(SURFACE_STATUS_MALFORMED), "below the minimum");
    assert!(t.get_by_id(700).is_none(), "nothing registered");
}
