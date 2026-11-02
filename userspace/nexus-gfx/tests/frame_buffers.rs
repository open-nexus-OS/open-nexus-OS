// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A layer that samples a surface buffer the app draws into (RFC-0114 §6, TASK-0348): its source
//! and slot cross the scene wire, a flip of the buffer's slots is new content — never a new
//! backdrop for the glass above it nor a moved layer — the layer uploads exactly when its
//! buffer's generation, buffer or slot changed, and the board samples the buffer, not the layout.

use nexus_gfx::command::buffer::{Command, CommandBuffer, CommittedBuffer};
use nexus_gfx::command::layer::Layer;
use nexus_gfx::command::layer_wire::{layer_slot, with_slot, LAYER_SLOT_MASK};
use nexus_gfx::core::types::{RenderPassDesc, TileRect};
use nexus_gfx::frame::{Board, Frame, FrameLayer, Planned, Tex};

const DISPLAY: (u32, u32) = (1920, 1080);
const LAYOUT_ROWS: u32 = 14040;
const HALF_ROWS: u32 = 600;
/// The buffer's id gpud gave windowd (never 0).
const BUFFER: u32 = 7;

fn cmd(l: FrameLayer) -> Command {
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
        flags: with_slot(0, l.slot),
        key: l.key,
        source: l.source,
    }
}

/// A window's body drawn from slot `slot` of the buffer at present `generation`.
fn body(slot: u8, generation: u32) -> FrameLayer {
    FrameLayer {
        src_row_abs: 0,
        width: 600,
        height: 400,
        dst_x: 300,
        dst_y: 200,
        opacity: 255,
        layer_id: 1,
        key: 0x0001_0100,
        source: BUFFER,
        slot,
        content_epoch: generation,
        ..FrameLayer::default()
    }
}

/// A glass panel (the desktop's) lying over the window.
fn glass_over() -> FrameLayer {
    FrameLayer {
        src_row_abs: 5400,
        width: 200,
        height: 100,
        dst_x: 400,
        dst_y: 300,
        opacity: 255,
        backdrop_blur: 20,
        key: 0xffff_0300,
        ..FrameLayer::default()
    }
}

/// A present of `layers` naming no damage of its own (what moved alone is the planner's).
fn plan(frame: &mut Frame, layers: &[FrameLayer]) -> Planned {
    let mut cmds: Vec<Command> = layers.iter().copied().map(cmd).collect();
    cmds.push(Command::Damage { rect: TileRect::NONE, origin: 0 });
    frame.layers.take_frame(&cmds);
    frame.plan(DISPLAY, &|_| true)
}

#[test]
fn a_buffer_layer_crosses_the_wire_with_its_source_and_slot() {
    // The encoder windowd composes with: a layer of slot 2 of the buffer.
    let mut layer = Layer::opaque(0, 0, 600, 400, 300, 200);
    (layer.source, layer.slot, layer.key) = (BUFFER, 2, 0x0001_0100);
    let mut cb = CommandBuffer::new();
    {
        let desc = RenderPassDesc { color_attachments: Vec::new(), width: 1920, height: 1080 };
        let mut enc = cb.begin_render_pass(desc);
        enc.composite_layer_full(&layer, DISPLAY).expect("emit");
    }
    let committed = cb.commit();
    let mut wire = [0u8; 256];
    let n = committed.serialize_into(&mut wire).expect("serialize");
    let (back, used) = CommittedBuffer::deserialize_from(&wire[..n]).expect("read");
    assert_eq!(used, n);
    assert_eq!(back.commands(), committed.commands());
    let [Command::CompositeLayer { flags, source, .. }] = back.commands() else {
        panic!("one layer: {:?}", back.commands());
    };
    assert_eq!((*source, layer_slot(*flags)), (BUFFER, 2));
    assert_eq!(*flags & !LAYER_SLOT_MASK, 0, "the slot takes no other flag's bit");
}

#[test]
fn a_slot_flip_is_new_content_never_a_new_backdrop_or_a_moved_layer() {
    let mut f = Frame::new(1024);
    f.glass.ready = true;
    plan(&mut f, &[body(0, 1), glass_over()]);
    // The app presents into the other slot: the glass above keeps its blur (its key is the
    // window's geometry, not the slot) and the window is not a moved layer — what the app drew
    // reaches the glass as the present's damage (none named here), never through the flip.
    let flipped = plan(&mut f, &[body(1, 2), glass_over()]);
    assert_eq!(flipped.writes.total(), 0, "no glass blurred again for a slot flip");
    assert!(
        !flipped.damage.touches(nexus_gfx::frame::Area::rect(300, 200, 600, 400)),
        "the flip names no damage of its own: the present's damage does"
    );
    assert!(f.step(0).expect("the body").upload, "the flipped slot's rows are new");
}

#[test]
fn a_buffer_layer_uploads_exactly_when_its_generation_buffer_or_slot_changed() {
    let mut f = Frame::new(1024);
    plan(&mut f, &[body(0, 1)]);
    assert!(f.step(0).expect("new").upload, "a new layer uploads");
    plan(&mut f, &[body(0, 1)]);
    assert!(!f.step(0).expect("same").upload, "the same rows are not uploaded again");
    plan(&mut f, &[body(0, 2)]);
    assert!(f.step(0).expect("presented").upload, "a present's rows are");
    plan(&mut f, &[FrameLayer { source: BUFFER + 1, ..body(0, 2) }]);
    assert!(f.step(0).expect("re-created").upload, "another buffer's rows are");
    f.layers.invalidate_upload();
    plan(&mut f, &[FrameLayer { source: BUFFER + 1, ..body(0, 2) }]);
    assert!(f.step(0).expect("lost").upload, "an executor that lost its uploads gets them all");
    // A layout layer is the frame's upload, never the step's.
    plan(&mut f, &[glass_over()]);
    assert!(!f.step(0).expect("layout").upload);
}

#[test]
fn the_board_samples_the_buffer_slot_not_the_layout() {
    let mut f = Frame::new(1024);
    let p = plan(&mut f, &[body(1, 4)]);
    let b = Board::new(&f, p, DISPLAY, LAYOUT_ROWS, HALF_ROWS);
    let pass = b.pass(b.pass_count() - 1).expect("the slot's pass");
    let texes: Vec<Tex> =
        (0..b.draws(&pass)).filter_map(|i| b.draw(&pass, i)).map(|d| d.tex).collect();
    assert!(
        texes.contains(&Tex::Buffer { source: BUFFER, slot: 1, x: 0, row: 0, w: 600, h: 400 }),
        "{texes:?}"
    );
}
