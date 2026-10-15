// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The WGSL frontend (TASK-0215 5c-2 P2b, P3c): the compositor's layer program lowered into the
//! IR and run by the reference interpreter over the render bundle's checker — the same pixels as
//! the CPU rasterizer's formulas under each blend, each override variant; the refusals name what
//! the subset lacks.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::backend::gpu::scene::checker_v1;
use nexus_gfx::raster::blend_over_px;
use nexus_gfx::shader::ir::interp::{run, Invocation, Rgba8Texture, Texture};
use nexus_gfx::shader::ir::{Interp, Op, Scalar, Stage, Type};
use nxgpu::{lower_fragment, Blend, FrontendError};

const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");
const SIZE: u32 = 256;

fn checker() -> Rgba8Texture {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    checker_v1(SIZE, SIZE, SIZE * 4, &mut px);
    Rgba8Texture::new(SIZE, SIZE, px).expect("the checker")
}

/// The layer's push block (`Quad::layer`, the one place its layout is written down).
fn layer_push(w: f32, h: f32, tint: [f32; 4], radius: f32) -> Vec<u8> {
    let q = Quad::layer(0.0, 0.0, w, h, tint, radius);
    q.push[..12].iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn to_u8(x: f32) -> u8 {
    (x * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn invoke(
    overrides: &[(&str, f64)],
    blend: Blend,
    uv: [f32; 2],
    tint: [f32; 4],
    dst: [u8; 4],
) -> [u8; 4] {
    let m = lower_fragment(LAYER, "main", overrides, blend).expect("the layer lowers");
    let tex = checker();
    let push = layer_push(SIZE as f32, SIZE as f32, tint, 0.0);
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
    let m = lower_fragment(LAYER, "main", &[], Blend::Replace).expect("the layer lowers");
    let i = &m.interface;
    assert_eq!(i.stage, Stage::Fragment);
    assert_eq!(
        (i.inputs[0].location, i.inputs[0].ty, i.inputs[0].interp),
        (0, Type::VEC2, Interp::Perspective)
    );
    assert_eq!((i.outputs[0].location, i.outputs[0].ty), (0, Type::VEC4));
    assert_eq!((i.push_bytes, i.resources.len()), (48, 2));
}

#[test]
fn the_lowered_layer_replaces_with_texel_times_tint() {
    assert_eq!(invoke(&[], Blend::Replace, UV, [1.0, 0.5, 0.5, 1.0], CLEAR), [40, 45, 100, 136]);
}

#[test]
fn the_lowered_layer_blends_as_the_rasterizer_does() {
    let px = invoke(&[], Blend::OverStraight, UV, [1.0; 4], CLEAR);
    let cpu = blend_over_px(CLEAR, TEXEL);
    assert!(
        px.iter().zip(&cpu).take(3).all(|(a, b)| a.abs_diff(*b) <= 1),
        "{px:?} against {cpu:?}"
    );
}

#[test]
fn the_untinted_variant_draws_the_texel_without_a_multiply() {
    // The tint is ignored: the texel as it is; no multiply, no select, no read of the tint.
    assert_eq!(invoke(&[("tinted", 0.0)], Blend::Replace, UV, [1.0, 0.5, 0.5, 1.0], CLEAR), TEXEL);
    let m = lower_fragment(LAYER, "main", &[("tinted", 0.0)], Blend::Replace).expect("lowers");
    assert!(!m
        .body
        .iter()
        .any(|i| matches!(i.op, Op::Binary(..) | Op::Select(..) | Op::LoadPush(_))));
    // The default and an explicit true are the tinted layer, instruction for instruction.
    let tinted = lower_fragment(LAYER, "main", &[], Blend::OverPremultiplied).expect("lowers");
    let explicit = lower_fragment(LAYER, "main", &[("tinted", 1.0)], Blend::OverPremultiplied)
        .expect("lowers");
    assert_eq!(tinted.body, explicit.body);
    assert!(!tinted.body.iter().any(|i| matches!(i.op, Op::Select(..))));
}

#[test]
fn an_override_is_keyed_by_its_id_else_its_name() {
    let src = "@id(7) override level: f32 = 0.5;\noverride gain: f32;\n@fragment fn main() -> @location(0) vec4<f32> {\n    return vec4<f32>(level, gain, 0.0, 1.0);\n}\n";
    let m = lower_fragment(src, "main", &[("7", 0.25), ("gain", 0.75)], Blend::Replace)
        .expect("lowers");
    let outs = run(&m, &Invocation { inputs: &[], push: &[], textures: &[], dst: &[[0.0; 4]] })
        .expect("runs");
    assert_eq!(outs[0], [0.25, 0.75, 0.0, 1.0]);
}

#[test]
fn test_reject_override_values() {
    let src = "@id(7) override level: f32 = 0.5;\noverride count: i32 = 1;\noverride gain: f32;\n@fragment fn main() -> @location(0) vec4<f32> {\n    return vec4<f32>(level, f32(count), gain, 1.0);\n}\n";
    let refused = |given: &[(&str, f64)]| {
        matches!(
            lower_fragment(src, "main", given, Blend::Replace),
            Err(FrontendError::Override(_))
        )
    };
    // gain has no default: it must be given.
    assert!(refused(&[]));
    // A key naming no override; level by its name although it has an id; a key given twice.
    assert!(refused(&[("gain", 1.0), ("tinted", 0.0)]));
    assert!(refused(&[("gain", 1.0), ("level", 0.25)]));
    assert!(refused(&[("gain", 1.0), ("gain", 0.5)]));
    // Values their types cannot hold.
    assert!(refused(&[("gain", 1.0), ("count", 1.5)]));
    assert!(refused(&[("gain", 1.0), ("count", 3e9)]));
    assert!(refused(&[("gain", f64::NAN)]));
    assert!(refused(&[("gain", 1e39)]));
    assert!(lower_fragment(src, "main", &[("gain", 1.0)], Blend::Replace).is_ok());
}

#[test]
fn test_reject_what_the_subset_lacks() {
    let discard = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n    if uv.x < 0.5 { discard; }\n    return vec4<f32>(uv, 0.0, 1.0);\n}\n";
    assert_eq!(
        lower_fragment(discard, "main", &[], Blend::Replace),
        Err(FrontendError::Unsupported("control flow"))
    );
    assert!(matches!(
        lower_fragment("fn broken(", "main", &[], Blend::Replace),
        Err(FrontendError::Parse(_))
    ));
    assert_eq!(
        lower_fragment(LAYER, "other", &[], Blend::Replace),
        Err(FrontendError::NoEntryPoint("other".into()))
    );
}

#[test]
fn scalar_vector_arithmetic_is_widened() {
    let src = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {\n    return vec4<f32>(uv * 2.0, 0.25, 1.0);\n}\n";
    let m = lower_fragment(src, "main", &[], Blend::Replace).expect("lowers");
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

#[test]
fn dot_length_and_the_unit_clamp_lower_to_the_float_units_ops() {
    let src = "struct P { v: vec4<f32>, }\nvar<immediate> p: P;\n@fragment fn main() -> @location(0) vec4<f32> {\n    let v = p.v.xy;\n    return vec4<f32>(dot(v, p.v.zw), length(v), clamp(p.v.x, 0.0, 1.0), length(p.v.z));\n}\n";
    let m = lower_fragment(src, "main", &[], Blend::Replace).expect("lowers");
    let push: Vec<u8> = [3.0f32, 4.0, 0.5, -2.0].iter().flat_map(|x| x.to_le_bytes()).collect();
    let outs = run(&m, &Invocation { inputs: &[], push: &push, textures: &[], dst: &[[0.0; 4]] })
        .expect("runs");
    assert_eq!(outs[0], [-6.5, 5.0, 1.0, 0.5]);
    // The unit clamp is the free saturation, not a min and a max; the lengths a square root
    // and an absolute value.
    use nexus_gfx::shader::ir::{BinOp, UnOp};
    let has = |f: &dyn Fn(&Op) -> bool| m.body.iter().any(|i| f(&i.op));
    assert!(has(&|o| matches!(o, Op::Unary(UnOp::Sat, _))));
    assert!(!has(&|o| matches!(o, Op::Binary(BinOp::Min | BinOp::Max, ..))));
    assert!(has(&|o| matches!(o, Op::Unary(UnOp::Sqrt, _))));
    assert!(has(&|o| matches!(o, Op::Unary(UnOp::Abs, _))));
}

/// A texture of one straight colour.
fn solid(c: [u8; 4]) -> Rgba8Texture {
    Rgba8Texture::new(4, 4, c.repeat(16)).expect("the solid texture")
}

fn run_layer(overrides: &[(&str, f64)], tex: &Rgba8Texture, uv: [f32; 2], push: &[u8]) -> [f32; 4] {
    let m = lower_fragment(LAYER, "main", overrides, Blend::Replace).expect("the layer lowers");
    let textures: [Option<&dyn Texture>; 2] = [Some(tex), None];
    let outs = run(
        &m,
        &Invocation {
            inputs: &[[uv[0], uv[1], 0.0, 0.0]],
            push,
            textures: &textures,
            dst: &[[0.0; 4]],
        },
    )
    .expect("the layer runs");
    outs[0]
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
}

#[test]
fn the_straight_variant_premultiplies_the_texel_at_the_layers_opacity() {
    let tint = [1.0, 0.5, 0.5, 0.5];
    let c = [200u8, 100, 50, 128];
    let out =
        run_layer(&[("straight", 1.0)], &solid(c), [0.5, 0.5], &layer_push(64.0, 64.0, tint, 0.0));
    let s = c.map(|v| f32::from(v) / 255.0);
    let a = s[3] * tint[3];
    let want = [s[0] * tint[0] * a, s[1] * tint[1] * a, s[2] * tint[2] * a, a];
    assert!(close(out, want), "{out:?} against {want:?}");
}

/// The virgl layer's coverage (`virgl_composite.rs` FS_LAYER): the rounded rectangle's exact
/// signed distance at the pixel centre, a one-pixel band.
fn coverage(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let (qx, qy) = ((px - w / 2.0).abs() - (w / 2.0 - r), (py - h / 2.0).abs() - (h / 2.0 - r));
    let len = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let sd = len + qx.max(qy).min(0.0) - r;
    (0.5 - sd).clamp(0.0, 1.0)
}

#[test]
fn the_rounded_variant_cuts_its_corners_as_the_virgl_layer_does() {
    let (w, h, r) = (64.0, 64.0, 12.0);
    let c = [200u8, 100, 50, 255];
    let push = layer_push(w, h, [1.0; 4], r);
    let tex = solid(c);
    let s = c.map(|v| f32::from(v) / 255.0);
    // The far corner (outside), a pixel on the arc (partial), the centre and an edge's middle.
    for (x, y, expect) in
        [(0.0, 0.0, Some(0.0)), (3.0, 3.0, None), (32.0, 32.0, Some(1.0)), (0.0, 32.0, Some(1.0))]
    {
        let (px, py) = (x + 0.5f32, y + 0.5f32);
        let cov = coverage(px, py, w, h, r);
        if let Some(e) = expect {
            assert_eq!(cov, e, "the reference at ({x}, {y})");
        } else {
            assert!(cov > 0.0 && cov < 1.0, "a partial pixel on the arc: {cov}");
        }
        let out = run_layer(&[("straight", 1.0), ("rounded", 1.0)], &tex, [px / w, py / h], &push);
        let a = s[3] * cov;
        let want = [s[0] * a, s[1] * a, s[2] * a, a];
        assert!(close(out, want), "({x}, {y}): {out:?} against {want:?}");
    }
}

const SHADOW: &str = include_str!("../../../resources/gpu/shaders/shadow.wgsl");

/// The virgl shadow (`virgl_vector.rs` FS_SHADOW): the distance outside the shifted rounded
/// rectangle falling off over the blur, squared, knocked out across the unshifted one.
fn shadow_alpha(
    p: (f32, f32),
    inner: (f32, f32),
    r: f32,
    blur: f32,
    offset: (f32, f32),
    alpha: f32,
) -> f32 {
    let dist = |x: f32, y: f32| {
        let (qx, qy) = (x.abs() - inner.0, y.abs() - inner.1);
        (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - r
    };
    let ramp = (1.0 - dist(p.0, p.1) / blur).clamp(0.0, 1.0);
    let knock = dist(p.0 + offset.0, p.1 + offset.1).clamp(0.0, 1.0);
    alpha * ramp * ramp * knock
}

#[test]
fn the_shadow_falls_off_and_is_knocked_out_as_the_virgl_shadow() {
    // A 64x48 layer, corner radius 8, the window's shadow: blur 18, offset (0, 5), alpha 90/255.
    let (w, h, r, blur, off) = (64.0f32, 48.0f32, 8.0f32, 18.0f32, [0.0f32, 5.0]);
    let colour = [0.2, 0.3, 0.4, 90.0 / 255.0];
    let q = Quad::shadow(100.0, 100.0, w, h, r, blur, off, colour);
    let push: Vec<u8> = q.push.iter().flat_map(|x| x.to_le_bytes()).collect();
    let m = lower_fragment(SHADOW, "main", &[], Blend::Replace).expect("the shadow lowers");
    assert_eq!((m.interface.resources.len(), m.interface.push_bytes), (0, 64));
    // Quad pixels: under the window, below it, mid-ramp left, past the blur, at the window's edge.
    for (px, py) in [(50.0, 42.0), (50.0, 76.0), (10.0, 42.0), (0.0, 0.0), (17.6, 42.0)] {
        let (cx, cy) = (px + 0.5f32, py + 0.5f32);
        let p = (cx - q.w / 2.0, cy - q.h / 2.0);
        let a = shadow_alpha(p, (w / 2.0 - r, h / 2.0 - r), r, blur, (off[0], off[1]), colour[3]);
        let outs = run(
            &m,
            &Invocation {
                inputs: &[[cx / q.w, cy / q.h, 0.0, 0.0]],
                push: &push,
                textures: &[],
                dst: &[[0.0; 4]],
            },
        )
        .expect("the shadow runs");
        let want = [colour[0] * a, colour[1] * a, colour[2] * a, a];
        assert!(close(outs[0], want), "({px}, {py}): {:?} against {want:?}", outs[0]);
    }
    // Under the window nothing; below it the shadow shows.
    let at = |px: f32, py: f32| {
        let p = (px + 0.5 - q.w / 2.0, py + 0.5 - q.h / 2.0);
        shadow_alpha(p, (w / 2.0 - r, h / 2.0 - r), r, blur, (off[0], off[1]), colour[3])
    };
    assert_eq!(at(50.0, 42.0), 0.0);
    assert!(at(50.0, 76.0) > 0.05);
}
