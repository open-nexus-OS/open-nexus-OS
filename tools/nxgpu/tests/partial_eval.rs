// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the pipeline fixes, evaluated while lowering (TASK-0215 5c-2 P5d-2): a loop on a fixed
//! counter unrolled into its taps, function variables holding what was last stored, an `if` on a
//! fixed condition, arithmetic of literals and overrides folded into one constant — the program
//! the reference interpreter runs is the straight line, and nothing the backend refuses (an
//! integer or boolean constant, a comparison) reaches it; the refusals name what stays outside.

use nexus_gfx::shader::ir::interp::{run, Invocation, Rgba8Texture, Texture};
use nexus_gfx::shader::ir::{BinOp, Module, Op, Scalar, Type};
use nxgpu::{lower_fragment, Blend, FrontendError};

/// Eight texels in a row, texel i at i/8 in every channel.
fn ramp() -> Rgba8Texture {
    let px: Vec<u8> = (0..8u8).flat_map(|i| [i * 32; 4]).collect();
    Rgba8Texture::new(8, 1, px).expect("the ramp")
}

fn eval(m: &Module, uv: [f32; 2]) -> [f32; 4] {
    let tex = ramp();
    let textures: [Option<&dyn Texture>; 2] = [Some(&tex), None];
    let outs = run(
        m,
        &Invocation {
            inputs: &[[uv[0], uv[1], 0.0, 0.0]],
            push: &[],
            textures: &textures,
            dst: &[[0.0; 4]],
        },
    )
    .expect("runs");
    outs[0]
}

fn count(m: &Module, f: impl Fn(&Op) -> bool) -> usize {
    m.body.iter().filter(|i| f(&i.op)).count()
}

/// The program carries only what the float unit runs: no comparison, no select, no integer or
/// boolean value.
fn straight_float(m: &Module) -> bool {
    m.body.iter().all(|i| {
        !matches!(i.op, Op::Compare(..) | Op::Select(..))
            && (i.ty.lanes == 0 || i.ty.scalar == Scalar::F32)
    })
}

const BOX: &str = "@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
override radius: i32 = 1;
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var acc = vec4<f32>(0.0);
    for (var i = -radius; i <= radius; i++) {
        acc += textureSample(t, s, uv + vec2<f32>(f32(i) * 0.125, 0.0));
    }
    return acc / f32(2 * radius + 1);
}
";

#[test]
fn a_loop_on_a_fixed_counter_unrolls_into_its_taps() {
    for radius in [1, 2, 3] {
        let m = lower_fragment(BOX, "main", &[("radius", f64::from(radius))], Blend::Replace)
            .expect("the box filter lowers");
        let taps = 2 * radius + 1;
        assert_eq!(count(&m, |o| matches!(o, Op::Sample { .. })), taps as usize);
        assert!(straight_float(&m), "radius {radius}: {:?}", m.body);
        // At texel 4's centre the taps are texels 4−r..4+r: their mean is texel 4's value.
        let out = eval(&m, [4.5 / 8.0, 0.5]);
        let want = 128.0 / 255.0;
        assert!(out.iter().all(|c| (c - want).abs() < 1e-5), "radius {radius}: {out:?}");
        // At the left edge the clamp repeats texel 0: (r+1)·0 + 1 + … + r texels, over the taps.
        let out = eval(&m, [0.5 / 8.0, 0.5]);
        let sum: i32 = (1..=radius).map(|i| i * 32).sum();
        let want = sum as f32 / 255.0 / taps as f32;
        assert!((out[0] - want).abs() < 1e-5, "radius {radius}: {out:?} against {want}");
    }
}

#[test]
fn a_loop_with_a_continuing_block_breaks_where_its_fixed_condition_says() {
    let src = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var sum = 0.0;
    var i = 0;
    loop {
        sum += uv.x;
        continuing {
            i += 1;
            break if i >= 5;
        }
    }
    return vec4<f32>(sum, 0.0, 0.0, 1.0);
}
";
    let m = lower_fragment(src, "main", &[], Blend::Replace).expect("lowers");
    assert!(straight_float(&m));
    // 0 + x is x: five terms, four additions.
    assert_eq!(count(&m, |o| matches!(o, Op::Binary(BinOp::Add, ..))), 4);
    assert_eq!(eval(&m, [0.25, 0.0]), [1.25, 0.0, 0.0, 1.0]);
}

#[test]
fn a_variable_holds_what_was_last_stored_and_a_load_reads_it_where_it_stands() {
    let src = "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var x = 1.0;
    let a = x * uv.x;
    x = 3.0;
    var c = vec4<f32>(0.0);
    c.y = a;
    c.z = x;
    c.w = c.z - 2.0;
    return c;
}
";
    let m = lower_fragment(src, "main", &[], Blend::Replace).expect("lowers");
    assert!(straight_float(&m));
    assert_eq!(eval(&m, [0.5, 0.0]), [0.0, 0.5, 3.0, 1.0]);
}

#[test]
fn an_if_on_a_fixed_condition_takes_its_branch_and_lowers_only_that() {
    let src = "@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
override dim: bool = false;
override strong: bool = false;
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var c = textureSample(t, s, uv);
    if dim && !strong {
        c *= 0.5;
    } else if dim {
        c *= 0.25;
    }
    return c;
}
";
    let lower = |dim: f64, strong: f64| {
        lower_fragment(src, "main", &[("dim", dim), ("strong", strong)], Blend::Replace)
            .expect("lowers")
    };
    let texel = 128.0 / 255.0;
    for (dim, strong, factor, muls) in
        [(0.0, 0.0, 1.0, 0), (0.0, 1.0, 1.0, 0), (1.0, 0.0, 0.5, 1), (1.0, 1.0, 0.25, 1)]
    {
        let m = lower(dim, strong);
        assert!(straight_float(&m));
        assert_eq!(count(&m, |o| matches!(o, Op::Binary(BinOp::Mul, ..))), muls);
        let out = eval(&m, [4.5 / 8.0, 0.5]);
        assert!((out[0] - texel * factor).abs() < 1e-6, "dim {dim} strong {strong}: {out:?}");
    }
}

#[test]
fn arithmetic_the_pipeline_fixes_is_one_constant_made_once() {
    let src = "override k: f32 = 3.0;
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let gain = exp2(k) - 2.0 * k;
    return vec4<f32>(uv.x * gain, uv.y * gain, 0.5 * gain, 1.0);
}
";
    let m = lower_fragment(src, "main", &[], Blend::Replace).expect("lowers");
    // exp2(3) − 6 = 2: two multiplies by it, the constant made once, 0.5·2 folded into the 1.0
    // the alpha already made.
    assert_eq!(count(&m, |o| matches!(o, Op::Binary(..))), 2);
    let consts: Vec<f32> = m
        .body
        .iter()
        .filter_map(|i| match i.op {
            Op::Const(bits) if i.ty == Type::F32 => Some(f32::from_bits(bits[0])),
            _ => None,
        })
        .collect();
    assert_eq!(consts, [2.0, 1.0]);
    assert_eq!(eval(&m, [0.25, 0.125]), [0.5, 0.25, 1.0, 1.0]);
    // Another value of the override is another constant, folded the same way.
    let m = lower_fragment(src, "main", &[("k", 2.0)], Blend::Replace).expect("lowers");
    assert_eq!(eval(&m, [0.25, 0.125]), [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn test_reject_what_the_pipeline_cannot_fix() {
    let refused = |src: &str| match lower_fragment(src, "main", &[], Blend::Replace) {
        Err(FrontendError::Unsupported(what)) => what,
        other => panic!("{other:?}"),
    };
    let body = |b: &str| {
        format!(
            "@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {{\n{b}\n}}\n"
        )
    };
    // A loop past the iterations a pipeline unrolls.
    assert_eq!(
        refused(&body(
            "var i = 0;\nloop { i += 1; if i > 5000 { break; } }\nreturn vec4<f32>(f32(i));"
        )),
        "a loop past the iterations a pipeline unrolls"
    );
    // A loop on a computed counter, a branch on a computed value.
    assert_eq!(
        refused(&body("var x = 0.0;\nwhile x < uv.x { x += 0.25; }\nreturn vec4<f32>(x);")),
        "a branch on a value the program computes"
    );
    // An index the program computes.
    assert_eq!(
        refused(&body(
            "var v = vec4<f32>(uv, 0.0, 1.0);\nlet i = i32(uv.x * 4.0);\nreturn vec4<f32>(v[i]);"
        )),
        "an index the pipeline does not fix"
    );
    // Arrays: as variables, as constants — refused, not flattened into a vector.
    assert_eq!(
        refused(&body("var w = array<f32, 2>(1.0, 2.0);\nreturn vec4<f32>(w[1]);")),
        "a value type outside scalars and vectors"
    );
    let constant = "const W = array<vec2<f32>, 2>(vec2<f32>(1.0, 2.0), vec2<f32>(3.0, 4.0));
@fragment fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var i = 1;
    return vec4<f32>(W[i], uv);
}
";
    assert_eq!(refused(constant), "a value type outside scalars and vectors");
    // A switch, even on a fixed selector.
    assert_eq!(
        refused(&body(
            "var x = 0.0;\nswitch 1 { case 1: { x = 1.0; } default: {} }\nreturn vec4<f32>(x);"
        )),
        "a switch"
    );
}
