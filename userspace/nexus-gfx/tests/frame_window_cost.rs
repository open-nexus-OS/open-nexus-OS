// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The pixels a window costs in the slot's pass (TASK-0351 P14): a glass window whose
//! content pane is an opaque region costs its body once, outside the pane, and nothing of the
//! wallpaper or the glass back under the pane is drawn.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{Board, Frame, FrameLayer, PassSpec, PROGRAM_KINDS};

const DISPLAY: (u32, u32) = (1920, 1080);
const LAYOUT_ROWS: u32 = 14_040;
const HALF_ROWS: u32 = 600;
const ATLAS: u32 = 5400;

fn layer(f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer { src_row_abs: ATLAS, opacity: 255, ..FrameLayer::default() };
    f(&mut l);
    Command::CompositeLayer {
        src_row_abs: l.src_row_abs,
        src_x: l.src_x,
        width: l.width,
        height: l.height,
        dst_x: l.dst_x,
        dst_y: l.dst_y,
        opacity: l.opacity,
        corner_radius: l.corner_radius,
        shadow_blur: l.shadow_blur,
        shadow_offset_y: l.shadow_offset_y,
        shadow_alpha: l.shadow_alpha,
        backdrop_blur: l.backdrop_blur,
        content_w: l.content_w,
        content_h: l.content_h,
        scroll_band_top_abs: l.scroll_band_top_abs,
        scroll_band_h: l.scroll_band_h,
        layer_id: l.layer_id,
        content_epoch: l.content_epoch,
        flags: nexus_gfx::command::layer_wire::with_slot(
            (if l.opaque { nexus_gfx::command::buffer::LAYER_OPAQUE } else { 0 })
                | (if l.solid { nexus_gfx::command::buffer::LAYER_SOLID } else { 0 }),
            l.slot,
        ),
        key: l.key,
        source: l.source,
    }
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> impl FnOnce(&mut FrameLayer) {
    move |l| {
        (l.dst_x, l.dst_y, l.width, l.height) = (x, y, w, h);
    }
}

fn ready() -> Frame {
    let mut f = Frame::new(2048);
    f.glass.ready = true;
    f
}

fn slot(b: &Board<'_>) -> PassSpec {
    b.pass(b.pass_count() - 1).expect("the slot's pass")
}

/// A settled glass window at (200, 100) 800×600 with its shadow, its opaque content pane
/// 600×480 at (300, 200) and a damage of `dmg`.
fn window(pane_opaque: bool, spread: bool, dmg: TileRect) -> Vec<Command> {
    let body = layer(|l| {
        rect(200, 100, 800, 600)(l);
        (l.backdrop_blur, l.corner_radius, l.layer_id, l.key) = (40, 24, 1, 1 << 16);
        (l.shadow_blur, l.shadow_offset_y, l.shadow_alpha) = (24, 8, 80);
        if spread {
            (l.content_w, l.content_h) = (780, 580);
        }
    });
    let pane = layer(|l| {
        rect(300, 200, 600, 480)(l);
        (l.corner_radius, l.layer_id, l.key, l.opaque) = (16, 1, (1 << 16) | (1 << 8), pane_opaque);
    });
    vec![body, pane, Command::Damage { rect: dmg, origin: 0 }]
}

fn px(b: &Board<'_>, p: &PassSpec) -> [u64; PROGRAM_KINDS] {
    b.work_by_program(p)
}

#[test]
fn an_opaque_pane_hides_the_body_the_back_and_the_wallpaper_under_it() {
    let mut f = ready();
    let whole = TileRect { x: 0, y: 0, width: 1920, height: 1080 };
    f.layers.take_frame(&window(true, false, whole));
    let p = f.plan(DISPLAY, &|_| true);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let s = slot(&b);
    let [opaque, straight, rounded, _shadow, _blur] = px(&b, &s);
    // The pane's 600×480 is drawn once, opaque; the body's blended pixels are the window minus
    // the pane's occluder (inset by its radius); the wallpaper is cut around the glass back.
    assert!(straight < 800 * 600 - (600 - 32) * (480 - 32) + 4 * 24 * 24, "straight={straight}");
    // The corner squares alone: the body's four of 24, the pane's four of 16 and the shadow's.
    assert!(rounded < 12 * 24 * 24, "rounded={rounded}");
    assert!(opaque < 1920 * 1080 + 800 * 600 - 600 * 480 / 2, "opaque={opaque}");
}

/// The blended pixels of the settled window: the body outside the pane and the rounded parts.
fn blended(cmds: &[Command]) -> (u64, [u64; PROGRAM_KINDS]) {
    let mut f = ready();
    f.layers.take_frame(cmds);
    let p = f.plan(DISPLAY, &|_| true);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let s = slot(&b);
    let w = px(&b, &s);
    (w[1] + w[2], w)
}

#[test]
fn an_opaque_slice_on_the_pane_adds_no_blended_pixel_and_cuts_the_pane() {
    let whole = TileRect { x: 0, y: 0, width: 1920, height: 1080 };
    let mut with = window(true, false, whole);
    // The list's slice over the pane (TASK-0351 P14: `BAND_OPAQUE` → an opaque layer).
    let slice = |opaque: bool| {
        layer(|l| {
            rect(320, 240, 560, 400)(l);
            (l.layer_id, l.key, l.opaque) = (1, (1 << 16) | (3 << 8), opaque);
        })
    };
    with.insert(2, slice(true));
    let (blended_without, w0) = blended(&window(true, false, whole));
    let (blended_with, w1) = blended(&with);
    assert_eq!(blended_with, blended_without, "an opaque slice blends nothing: {w1:?} vs {w0:?}");
    // The pane under the slice is cut: the slice's pixels replace the pane's, not add to them.
    assert!(w1[0] <= w0[0] + 560 * 400 / 10, "opaque {} vs {}", w1[0], w0[0]);
    // The same slice transparent (today's bands): its whole area blended over the pane.
    let mut translucent = window(true, false, whole);
    translucent.insert(2, slice(false));
    let (blended_translucent, _) = blended(&translucent);
    assert!(blended_translucent >= blended_without + 560 * 400, "blended={blended_translucent}");
    // By kind (the board's `gpu blend kinds` line): the slice blends nothing when opaque, its
    // whole area when not; the body blends outside the pane either way.
    let kinds = |cmds: &[Command]| {
        let mut f = ready();
        f.layers.take_frame(cmds);
        let p = f.plan(DISPLAY, &|_| true);
        let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
        b.blended_by_kind(&slot(&b))
    };
    let (k_opaque, k_translucent) = (kinds(&with), kinds(&translucent));
    assert_eq!(k_opaque[3], 0, "{k_opaque:?}");
    assert_eq!(k_translucent[3], 560 * 400, "{k_translucent:?}");
    assert_eq!(k_opaque[1], k_translucent[1], "the body's blend does not depend on the slice");
    assert!(k_opaque[1] > 0 && k_opaque[2] < 8 * 16 * 16, "{k_opaque:?}");
}

#[test]
fn a_spread_body_is_one_rounded_draw_that_no_pane_cuts() {
    // A live resize's frame (the base spread over the frame): the planner cannot cut it — the
    // whole window runs the rounded program. Named here so the cost is known, not a surprise.
    let whole = TileRect { x: 0, y: 0, width: 1920, height: 1080 };
    let (_, w) = blended(&window(true, true, whole));
    assert_eq!(w[1], 0, "no straight draw");
    assert!(w[2] >= 800 * 600, "rounded={}", w[2]);
}

#[test]
fn a_settled_window_with_a_pointer_damage_costs_the_damage() {
    let mut f = ready();
    let whole = TileRect { x: 0, y: 0, width: 1920, height: 1080 };
    f.layers.take_frame(&window(true, false, whole));
    let _ = f.plan(DISPLAY, &|_| true);
    let point = TileRect { x: 500, y: 400, width: 16, height: 16 };
    f.layers.take_frame(&window(true, false, point));
    let p = f.plan(DISPLAY, &|_| true);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS).with_area((500, 400, 515, 415));
    let s = slot(&b);
    let w = px(&b, &s);
    assert_eq!(w.iter().sum::<u64>(), 16 * 16, "{w:?}");
}
