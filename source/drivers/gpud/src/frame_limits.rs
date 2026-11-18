// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What a frame may ask of the executor, and the proof that a frame at the
//! compositor's bounds goes the whole way (TASK-0351 P6). Four fixed numbers along one path —
//! the planner's 64 layers, the pass's 640 draws, the cache's 1024 rows, the serializer's 8000
//! bytes — were found one board cycle at a time (`board--2026-11-18T11-44-54`, `…T13-29-53`),
//! each behind the last. The bounds are tied by constant asserts now, and the test below sends
//! a scene of sixty-four windows down the real path on the host: the scene's encoder, the
//! serializer, the sender's parts, this driver's gathering, the decoder and its validation,
//! the planner, the board's passes — and counts the draws the largest pass keeps.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests below (host)
//! INVARIANTS: a present's body is the serializer's bound; a pass of `MAX_DRAWS` draws and the
//!   glass passes beside it fit every ring of the frame's pass memory; a pass of more draws
//!   keeps its topmost — a frame is never refused for its size.

/// The frame's pass memory (the context's buffer at `FRAME_VA`): the general arena (a draw's
/// push constants, PPP block and texture descriptor, about 290 bytes), the control streams,
/// the vertices (64 bytes a quad).
pub(crate) const GENERAL_BYTES: u32 = 1024 * 1024;
pub(crate) const STREAM_BYTES: u32 = 128 * 1024;
pub(crate) const VERTEX_BYTES: u32 = 384 * 1024;
/// The PDS heap's lines a frame's draws take their state-load data from (heap offsets): after
/// the targets' pixel event data, to the bundle's PDS arena's end. One line of 64 bytes a
/// draw — 1807 of them: the ring that bounds a frame, not the three above.
pub(crate) const PDS_LINES: (u32, u32) = (0x4c00, 0x2_1000);
/// The most draws a pass emits — those that touch its area: what the PDS lines hold beside
/// the frame's glass passes (`a_pass_of_the_most_draws_fits_every_ring`). Twenty windows drew
/// 606 a frame, about 33 a window. A pass of more keeps its topmost draws and says so
/// (`desktop_pass`); it was "2560" by the three rings that were grown, never by this one.
pub(crate) const MAX_DRAWS: usize = 1536;

/// How many of a pass's lowest draws are left out when `in_area` of them touch its area: the
/// topmost `MAX_DRAWS` are drawn — what lies lowest is what the layers above cover most.
pub(crate) const fn left_out(in_area: usize) -> usize {
    in_area.saturating_sub(MAX_DRAWS)
}

// One bound of a frame's bytes: what a present carries is what the scene serializes to.
const _: () =
    assert!(nexus_display_proto::PRESENT_BODY_MAX == nexus_gfx::command::buffer::MAX_SERIALIZED);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::present_parts::Parts;
    use alloc::vec::Vec;
    use nexus_display_proto::{
        present_parts, OP_PRESENT_DAMAGE, PRESENT_BODY_MAX, PRESENT_HEADER_LEN,
        PRESENT_PART_HEADER_LEN,
    };
    use nexus_gfx::backend::gpu::quads::{stream_bytes, Arena, Need, Placer, Rings};
    use nexus_gfx::frame::{clip_draw, Board, Frame};
    use nexus_gfx::{
        BackdropCache, CommandBuffer, CommittedBuffer, Layer, LayerBackdrop, LayerShadow,
        RenderPassDesc,
    };

    const DISPLAY: (u32, u32) = (1920, 1080);
    const WINDOWD: u64 = 0x77;
    /// One IPC frame (`nexus_abi::IPC_PAYLOAD_MAX`).
    const FRAME_MAX: usize = 8 * 1024;

    /// `windows` windows cascaded as the launcher opens them: a glass body with its shadow,
    /// a title bar, a sidebar and an opaque content pane — serialized as a present's body.
    fn scene(windows: u32, body: &mut [u8]) -> usize {
        let mut cb = CommandBuffer::new();
        let mut enc = cb
            .try_begin_render_pass(RenderPassDesc {
                color_attachments: alloc::vec![],
                width: DISPLAY.0,
                height: DISPLAY.1,
            })
            .expect("a pass");
        enc.try_damage_none().expect("damage");
        for w in 0..windows {
            enc.try_layer_transform(w + 1, (0, 0), 255, 10_000, (0, 0)).expect("transform");
        }
        for w in 0..windows {
            let (x, y) = (40 + 20 * w, 40 + 10 * w);
            let part = |x, y, width, height, n: u32| {
                let mut l = Layer::opaque(0, 0, width, height, x, y);
                (l.layer_id, l.key, l.corner_radius) = (w + 1, (w + 1) << 16 | n, 12);
                l
            };
            let mut glass = part(x, y, 600, 400, 0x100);
            glass.shadow = Some(LayerShadow { blur: 24, offset_y: 8, alpha: 90 });
            glass.backdrop = Some(LayerBackdrop {
                blur_radius: 40,
                saturation_percent: 120,
                restore_halo_pad: 0,
                retained_src_y_offset: 0,
                cache: BackdropCache::None,
            });
            let mut pane = part(x + 150, y + 40, 450, 360, 0x200);
            pane.opaque = true;
            for l in [glass, part(x, y, 600, 40, 0x300), part(x, y + 40, 150, 360, 0x400), pane] {
                enc.composite_layer_full(&l, DISPLAY).expect("a layer");
            }
        }
        enc.end_encoding();
        cb.serialize_into(body).expect("the scene is a present's body")
    }

    /// The rings as the desktop makes them (`desktop.rs`).
    fn rings() -> Rings {
        Rings::new(
            Arena { base: 0, len: GENERAL_BYTES },
            Arena { base: PDS_LINES.0, len: PDS_LINES.1 - PDS_LINES.0 },
            Arena { base: GENERAL_BYTES, len: STREAM_BYTES },
            Arena { base: GENERAL_BYTES + STREAM_BYTES, len: VERTEX_BYTES },
        )
    }

    /// What a pass of `n` draws takes of the rings (`emit_draws` and the kept draws'
    /// descriptors), each draw at its most: the widest push constants, the longest state data.
    fn pass(rings: &mut Rings, n: usize) -> bool {
        rings.begin();
        let mut ok = rings.place(0, Need::Stream(stream_bytes(n) as u32)).is_some()
            && rings.place(0, Need::Vertices(64 * n as u32)).is_some()
            && rings.place(0, Need::VsPdsData).is_some();
        for i in 0..n {
            ok = ok
                && rings.place(i, Need::Descriptor).is_some()
                && rings.place(i, Need::Push(64)).is_some()
                && rings.place(i, Need::StateData(48)).is_some()
                && rings.place(i, if i == 0 { Need::Ppp26 } else { Need::Ppp }).is_some();
        }
        ok && rings.submitted(1).is_ok()
    }

    #[test]
    fn a_pass_of_the_most_draws_fits_every_ring() {
        // A frame in flight: fifteen glass passes of a draw each, then the scene's pass.
        let mut r = rings();
        assert!((0..15).all(|_| pass(&mut r, 1)));
        assert!(pass(&mut r, MAX_DRAWS), "the scene pass at the bound");
        // The rings hold no second one beside it: the bound is the rings', not a guess.
        assert!(!pass(&mut r, MAX_DRAWS));
        assert_eq!((left_out(MAX_DRAWS), left_out(MAX_DRAWS + 7), left_out(3)), (0, 7, 0));
    }

    #[test]
    fn a_frame_of_sixty_four_windows_goes_the_whole_way() {
        // The compositor: the scene's bytes behind a part header's room, sent in parts.
        let mut wire = alloc::vec![0u8; PRESENT_PART_HEADER_LEN + PRESENT_BODY_MAX];
        let n = scene(64, &mut wire[PRESENT_PART_HEADER_LEN..]);
        assert!(n > 2 * FRAME_MAX, "{n}: the scene is past one frame");
        let sent = wire[PRESENT_PART_HEADER_LEN..][..n].to_vec();
        // The driver: the parts gathered, the present served whole.
        let mut parts = Parts::default();
        let mut whole = Vec::new();
        let wire = &mut wire[..PRESENT_PART_HEADER_LEN + n];
        present_parts::send(wire, 7, FRAME_MAX, |part| {
            assert!(whole.is_empty(), "nothing follows the last part");
            if let Some(present) = parts.take(part, WINDOWD) {
                whole = present.to_vec();
            }
            Ok::<(), ()>(())
        })
        .expect("sent");
        assert_eq!(whole[..PRESENT_HEADER_LEN], [OP_PRESENT_DAMAGE, 7, 0, 0, 0]);
        assert_eq!(whole[PRESENT_HEADER_LEN..], sent[..]);
        // Decoded and validated as the service loop does.
        let mut cb = CommittedBuffer::with_capacity(32);
        assert_eq!(cb.reload_from(&whole[PRESENT_HEADER_LEN..]), Ok(n));
        // Planned: no layer dropped, every glass given rows as the cache grows.
        let mut frame = Frame::new(1024);
        frame.glass.ready = true;
        assert_eq!(frame.layers.take_frame(cb.commands()).dropped, 0);
        let planned = frame.plan(DISPLAY, &|_| true);
        assert_eq!(planned.layers, 64 * 4);
        let mut planes = 1;
        while frame.glass.short() != 0 {
            frame.place_short();
            planes += 1;
            assert!(planes <= crate::surface_va::CACHE_PLANES, "the cache's planes hold it");
        }
        // The board's passes: this cascade's first frame asks more draws of the scene's pass
        // than a pass holds (about 44 a window) — the pass draws its topmost, never nothing.
        let board = Board::new(&frame, planned, DISPLAY, 14_040, 600);
        let most = (0..board.pass_count())
            .filter_map(|p| board.pass(p))
            .map(|spec| board.walk(&spec).filter(|d| clip_draw(d, spec.area).is_some()).count())
            .max()
            .unwrap_or(0);
        assert!((2048..4096).contains(&most), "{most} draws in the largest pass");
        assert_eq!(most - left_out(most), MAX_DRAWS);
    }
}
