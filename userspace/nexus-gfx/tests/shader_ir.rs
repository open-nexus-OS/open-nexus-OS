// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The IR and its reference interpreter (TASK-0215 5c-2 P2): the compositor's first program —
//! the textured, tinted layer — built through the builder, run by the interpreter over the
//! render bundle's checker texture, and judged by the CPU rasterizer's formulas under each
//! epilog; the validator's rejects; the 16-bit rounding.

use nexus_gfx::backend::gpu::scene::checker_v1;
use nexus_gfx::raster::{blend_over_px, blend_premultiplied_px};
use nexus_gfx::shader::ir::builder::{store_blended, Blend, Builder};
use nexus_gfx::shader::ir::interp::{round_f16, run, Invocation, Rgba8Texture, Texture};
use nexus_gfx::shader::ir::validate::validate;
use nexus_gfx::shader::ir::{
    BinOp, Instr, Interp, IrError, Lod, Module, Op, Stage, TexDim, Type, ValueId, MAX_INSTRS,
    VERSION,
};

const SIZE: u32 = 256;

/// The bundle's checker through the library's reference sampler.
fn checker() -> Rgba8Texture {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    checker_v1(SIZE, SIZE, SIZE * 4, &mut px);
    Rgba8Texture::new(SIZE, SIZE, px).expect("a 256x256 RGBA8 image")
}

/// The textured layer: `texture(tex, uv) * tint`, then the epilog.
fn layer(blend: Blend) -> Module {
    let mut b = Builder::new(Stage::Fragment);
    let uv_in = b.input(0, Type::VEC2, Interp::Perspective);
    let out = b.output(0, Type::VEC4);
    let tex = b.texture(0, TexDim::D2);
    let smp = b.sampler(1);
    b.push_bytes(16);
    let uv = b.load_input(uv_in);
    let texel = b.sample(tex, smp, uv, Lod::Implicit);
    let tint = b.load_push(0, Type::VEC4);
    let color = b.binary(BinOp::Mul, texel, tint);
    store_blended(&mut b, out, color, blend);
    b.finish().expect("the layer validates")
}

fn to_u8(x: f32) -> u8 {
    (x * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn invoke(m: &Module, uv: [f32; 2], tint: [f32; 4], dst: [f32; 4]) -> [u8; 4] {
    let checker = checker();
    let push: Vec<u8> = tint.iter().flat_map(|t| t.to_le_bytes()).collect();
    let textures: [Option<&dyn Texture>; 2] = [Some(&checker), None];
    let outs = run(
        m,
        &Invocation {
            inputs: &[[uv[0], uv[1], 0.0, 0.0]],
            push: &push,
            textures: &textures,
            dst: &[dst],
        },
    )
    .expect("the layer runs");
    outs[0].map(to_u8)
}

/// Texel (136, 136) at its centre: colour (40, 90, 200), alpha 136; the clear (26, 31, 41).
const UV: [f32; 2] = [136.5 / 256.0, 136.5 / 256.0];
const TEXEL: [u8; 4] = [40, 90, 200, 136];
const CLEAR: [u8; 4] = [26, 31, 41, 255];

fn unorm(px: [u8; 4]) -> [f32; 4] {
    px.map(|c| f32::from(c) / 255.0)
}

fn close(a: [u8; 4], b: [u8; 4], channels: usize) -> bool {
    a.iter().zip(&b).take(channels).all(|(x, y)| x.abs_diff(*y) <= 1)
}

#[test]
fn the_layer_replaces_with_texel_times_tint() {
    let px = invoke(&layer(Blend::Replace), UV, [1.0, 0.5, 0.5, 1.0], unorm(CLEAR));
    assert_eq!(px, [40, 45, 100, 136]);
}

#[test]
fn the_straight_over_epilog_is_the_rasterizers_over_on_colour_and_keeps_the_source_alpha() {
    let px = invoke(&layer(Blend::OverStraight), UV, [1.0; 4], unorm(CLEAR));
    let cpu = blend_over_px(CLEAR, TEXEL);
    assert!(close(px, cpu, 3), "colour {px:?} against the rasterizer's {cpu:?}");
    assert_eq!(px[3], TEXEL[3], "the captured pipeline's alpha factors: the source's");
}

#[test]
fn the_premultiplied_over_epilog_is_the_rasterizers_on_colour() {
    // A premultiplied source: the texel's colour already times its alpha.
    let a = f32::from(TEXEL[3]) / 255.0;
    let pre = [TEXEL[0], TEXEL[1], TEXEL[2]].map(|c| to_u8(f32::from(c) / 255.0 * a));
    let src = [pre[0], pre[1], pre[2], TEXEL[3]];
    let tint = [a, a, a, 1.0];
    let px = invoke(&layer(Blend::OverPremultiplied), UV, tint, unorm(CLEAR));
    let cpu = blend_premultiplied_px(CLEAR, src);
    assert!(close(px, cpu, 3), "colour {px:?} against the rasterizer's {cpu:?}");
    assert_eq!(px[3], 255, "over an opaque tile the result is opaque");
}

#[test]
fn a_sample_between_texels_of_one_block_reads_the_block() {
    // 5b's and 5c's board samples sit at 136/256: a 50/50 mix of two texels of the same block.
    let px = invoke(&layer(Blend::Replace), [136.0 / 256.0, 136.0 / 256.0], [1.0; 4], unorm(CLEAR));
    assert!(close(px, TEXEL, 3));
}

fn module(stage: Stage, body: Vec<Instr>) -> Module {
    let mut m = layer(Blend::Replace);
    m.interface.stage = stage;
    m.body = body;
    m
}

#[test]
fn test_reject_an_operand_used_before_its_definition() {
    let body = vec![
        Instr { op: Op::Binary(BinOp::Add, ValueId(1), ValueId(1)), ty: Type::F32 },
        Instr { op: Op::Const([0; 4]), ty: Type::F32 },
    ];
    assert_eq!(
        validate(&module(Stage::Fragment, body)),
        Err(IrError::Invalid { at: 0, what: "operand not defined before its use" })
    );
}

#[test]
fn test_reject_operands_of_different_types() {
    let body = vec![
        Instr { op: Op::Const([0; 4]), ty: Type::F32 },
        Instr { op: Op::Const([0; 4]), ty: Type::VEC2 },
        Instr { op: Op::Binary(BinOp::Mul, ValueId(0), ValueId(1)), ty: Type::F32 },
    ];
    assert_eq!(
        validate(&module(Stage::Fragment, body)),
        Err(IrError::Invalid { at: 2, what: "binary operands" })
    );
}

#[test]
fn test_reject_a_push_load_past_the_block() {
    let body = vec![Instr { op: Op::LoadPush(8), ty: Type::VEC4 }];
    assert_eq!(
        validate(&module(Stage::Fragment, body)),
        Err(IrError::Invalid { at: 0, what: "push load" })
    );
}

#[test]
fn test_reject_an_output_written_twice() {
    let body = vec![
        Instr { op: Op::Const([0; 4]), ty: Type::VEC4 },
        Instr { op: Op::StoreOutput(0, ValueId(0)), ty: Type::NONE },
        Instr { op: Op::StoreOutput(0, ValueId(0)), ty: Type::NONE },
    ];
    assert_eq!(
        validate(&module(Stage::Fragment, body)),
        Err(IrError::Invalid { at: 2, what: "an output written twice" })
    );
}

#[test]
fn test_reject_an_implicit_lod_outside_a_fragment_program() {
    let mut m = layer(Blend::Replace);
    m.interface.stage = Stage::Vertex;
    assert!(matches!(
        validate(&m),
        Err(IrError::Invalid { what: "implicit lod outside a fragment program", .. })
    ));
}

#[test]
fn test_reject_another_version_and_too_many_instructions() {
    let mut m = layer(Blend::Replace);
    m.version = VERSION + 1;
    assert_eq!(validate(&m), Err(IrError::Version(VERSION + 1)));
    let m = module(
        Stage::Fragment,
        vec![Instr { op: Op::Const([0; 4]), ty: Type::F32 }; MAX_INSTRS + 1],
    );
    assert_eq!(validate(&m), Err(IrError::TooLarge { what: "instructions" }));
}

#[test]
fn sixteen_bit_floats_round_to_nearest_even() {
    // Around 1.0 a binary16 step is 2^-10: half a step is a tie, which goes to the even mantissa.
    assert_eq!(round_f16(1.0 + 1.0 / 2048.0), 1.0, "a tie rounds down to the even mantissa");
    assert_eq!(
        round_f16(1.0 + 3.0 / 2048.0),
        1.0 + 2.0 / 1024.0,
        "a tie rounds up to the even mantissa"
    );
    assert_eq!(round_f16(1.0 + 3.0 / 4096.0), 1.0 + 1.0 / 1024.0, "no tie: the nearer value");
    assert_eq!(round_f16(65519.0), 65504.0);
    assert_eq!(round_f16(65520.0), f32::INFINITY);
    assert_eq!(round_f16(1.0e-7), 5.960_464_5e-8 * 2.0, "subnormals in steps of 2^-24");
    assert_eq!(round_f16(-0.333_333_34), -0.333_251_95);
}
