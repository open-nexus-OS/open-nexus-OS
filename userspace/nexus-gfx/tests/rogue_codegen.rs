// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board GPU's backend against the hand-assembled references (TASK-0215 5c-2 P3c): the
//! compositor's layer — texel, tinted, blended premultiplied — compiled from our IR gives the
//! reference programs byte for byte, fused (iterate-and-sample) and unfused (iterate, sample),
//! with the temporaries and pass type the kick and the draw state need; what the subset lacks
//! is refused by name.

use nexus_gfx::shader::ir::builder::{store_blended, Blend, Builder};
use nexus_gfx::shader::ir::{BinOp, Interp, Lod, Module, Op, Stage, TexDim, Type};
use nexus_gfx::shader::rogue::codegen::{compile, CodegenError, Layout, Options, Program};
use nexus_gfx::shader::rogue::encode_program;
use nexus_gfx::shader::rogue::reference::{self, Reference};

/// The layer: `texture(surface, uv)`, times the tint when `tinted`, through the epilog.
fn layer(tinted: bool, blend: Blend) -> Module {
    let mut b = Builder::new(Stage::Fragment);
    let uv_in = b.input(0, Type::VEC2, Interp::Perspective);
    let out = b.output(0, Type::VEC4);
    let tex = b.texture(0, TexDim::D2);
    let smp = b.sampler(1);
    b.push_bytes(16);
    let uv = b.load_input(uv_in);
    let texel = b.sample(tex, smp, uv, Lod::Implicit);
    let color = if tinted {
        let tint = b.load_push(0, Type::VEC4);
        b.binary(BinOp::Mul, texel, tint)
    } else {
        texel
    };
    store_blended(&mut b, out, color, blend);
    b.finish().expect("the layer validates")
}

fn compiled(m: &Module, fused: bool) -> Program {
    compile(m, Layout::BUNDLE, Options { fused_sample: fused }).expect("the layer compiles")
}

fn same(p: &Program, r: &Reference) {
    assert_eq!(
        encode_program(&p.groups).expect("encodes"),
        r.encode().expect("encodes"),
        "{}: the compiled program is the reference",
        r.name
    );
    assert_eq!((p.temps, p.pass_type), (r.temps, r.pass_type), "{}: kick and pass type", r.name);
}

#[test]
fn the_texel_compiles_to_the_opaque_references() {
    let m = layer(false, Blend::Replace);
    same(&compiled(&m, false), &reference::opaque_ditr().unwrap());
    same(&compiled(&m, true), &reference::opaque_itrsmp().unwrap());
}

#[test]
fn the_tinted_layer_compiles_to_the_tinted_references() {
    let m = layer(true, Blend::Replace);
    same(&compiled(&m, false), &reference::tinted().unwrap());
    same(&compiled(&m, true), &reference::tinted_itrsmp().unwrap());
}

#[test]
fn the_premultiplied_layer_compiles_to_the_premultiplied_references() {
    let m = layer(true, Blend::OverPremultiplied);
    same(&compiled(&m, false), &reference::premultiplied().unwrap());
    same(&compiled(&m, true), &reference::premultiplied_itrsmp().unwrap());
}

#[test]
fn test_reject_what_the_backend_lacks() {
    let unsupported =
        |r: Result<Program, CodegenError>| matches!(r, Err(CodegenError::Unsupported { .. }));
    // The straight-alpha epilog builds its result from parts in other registers.
    assert!(unsupported(compile(
        &layer(true, Blend::OverStraight),
        Layout::BUNDLE,
        Options { fused_sample: true }
    )));
    // Another stage.
    let mut m = layer(false, Blend::Replace);
    m.interface.stage = Stage::Compute;
    m.body.retain(|i| !matches!(i.op, Op::Sample { .. } | Op::StoreOutput(..)));
    assert!(compile(&m, Layout::BUNDLE, Options { fused_sample: true }).is_err());
    // A constant other than 0 and 1, and -0.0: the constant registers hold +0.0 and 1.0.
    for x in [0.5, -0.0] {
        let mut b = Builder::new(Stage::Fragment);
        let out = b.output(0, Type::VEC4);
        let c = b.f32s(&[x]);
        let v = b.splat(c, 4);
        b.store_output(out, v);
        let m = b.finish().unwrap();
        assert!(unsupported(compile(&m, Layout::BUNDLE, Options { fused_sample: true })), "{x}");
    }
}
