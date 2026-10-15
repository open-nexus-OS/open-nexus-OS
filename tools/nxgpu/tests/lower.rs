// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The WGSL frontend (TASK-0215 5c-2 P2b): the compositor's layer program lowered into the IR
//! and run by the reference interpreter over the render bundle's checker — the same pixels as the
//! CPU rasterizer's formulas under each blend; the refusals name what the subset lacks.

use nexus_gfx::backend::gpu::scene::checker_v1;
use nexus_gfx::raster::blend_over_px;
use nexus_gfx::shader::ir::interp::{run, Invocation, Rgba8Texture, Texture};
use nexus_gfx::shader::ir::{Interp, Scalar, Stage, Type};
use nxgpu::{lower_fragment, Blend, FrontendError};

const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");
const SIZE: u32 = 256;

fn checker() -> Rgba8Texture {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    checker_v1(SIZE, SIZE, SIZE * 4, &mut px);
    Rgba8Texture::new(SIZE, SIZE, px).expect("the checker")
}

fn to_u8(x: f32) -> u8 {
    (x * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn invoke(blend: Blend, uv: [f32; 2], tint: [f32; 4], dst: [u8; 4]) -> [u8; 4] {
    let m = lower_fragment(LAYER, "main", blend).expect("the layer lowers");
    let tex = checker();
    let push: Vec<u8> = tint.iter().flat_map(|t| t.to_le_bytes()).collect();
    let textures: [Option<&dyn Texture>; 2] = [Some(&tex), None];
    let dst = dst.map(|c| f32::from(c) / 255.0);
    let outs = run(
        &m,
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

const UV: [f32; 2] = [136.5 / 256.0, 136.5 / 256.0];
const TEXEL: [u8; 4] = [40, 90, 200, 136];
const CLEAR: [u8; 4] = [26, 31, 41, 255];

#[test]
fn the_layer_source_lowers_to_the_interface_the_pipeline_binds() {
    let m = lower_fragment(LAYER, "main", Blend::Replace).expect("the layer lowers");
    let i = &m.interface;
    assert_eq!(i.stage, Stage::Fragment);
    assert_eq!(
        (i.inputs[0].location, i.inputs[0].ty, i.inputs[0].interp),
        (0, Type::VEC2, Interp::Perspective)
    );
    assert_eq!((i.outputs[0].location, i.outputs[0].ty), (0, Type::VEC4));
    assert_eq!((i.push_bytes, i.resources.len()), (16, 2));
}

#[test]
fn the_lowered_layer_replaces_with_texel_times_tint() {
    assert_eq!(invoke(Blend::Replace, UV, [1.0, 0.5, 0.5, 1.0], CLEAR), [40, 45, 100, 136]);
}

#[test]
fn the_lowered_layer_blends_as_the_rasterizer_does() {
    let px = invoke(Blend::OverStraight, UV, [1.0; 4], CLEAR);
    let cpu = blend_over_px(CLEAR, TEXEL);
    assert!(
        px.iter().zip(&cpu).take(3).all(|(a, b)| a.abs_diff(*b) <= 1),
        "{px:?} against {cpu:?}"
    );
}

#[test]
fn test_reject_what_the_subset_lacks() {
    let discard = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n    if uv.x < 0.5 { discard; }\n    return vec4<f32>(uv, 0.0, 1.0);\n}\n";
    assert_eq!(
        lower_fragment(discard, "main", Blend::Replace),
        Err(FrontendError::Unsupported("control flow"))
    );
    assert!(matches!(
        lower_fragment("fn broken(", "main", Blend::Replace),
        Err(FrontendError::Parse(_))
    ));
    assert_eq!(
        lower_fragment(LAYER, "other", Blend::Replace),
        Err(FrontendError::NoEntryPoint("other".into()))
    );
}

#[test]
fn scalar_vector_arithmetic_is_widened() {
    let src = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n    return vec4<f32>(uv * 2.0, 0.25, 1.0);\n}\n";
    let m = lower_fragment(src, "main", Blend::Replace).expect("lowers");
    let outs = run(
        &m,
        &Invocation {
            inputs: &[[0.25, 0.125, 0.0, 0.0]],
            push: &[],
            textures: &[],
            dst: &[[0.0; 4]],
        },
    )
    .expect("runs");
    assert_eq!(outs[0], [0.5, 0.25, 0.25, 1.0]);
    assert!(m.body.iter().any(|i| i.ty == Type::new(Scalar::F32, 2)));
}
