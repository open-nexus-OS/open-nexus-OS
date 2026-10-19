// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The glass layer's backdrop blur (TASK-0215 5c-2 P5d-4): `blur.wgsl`'s two passes run by the
//! reference interpreter at half resolution — each written to eight bits, as a plane holds it —
//! and sampled back to full resolution by the layer, against the separable gaussian the virgl
//! path drew at full resolution until P6 (σ = R/2, taps out to ±R, weights 2^(k·i²) normalised,
//! clamped at the edges, eight bits between the passes) and the CPU path's saturation
//! (`raster::saturate`). The loops are unrolled into their taps, the weights constants.

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::shader::ir::interp::{run, Invocation, Rgba8Texture, Texture};
use nexus_gfx::shader::ir::{Module, Op};
use nxgpu::{compile_fragment, lower_fragment, Blend};

const BLUR: &str = include_str!("../../../resources/gpu/shaders/blur.wgsl");
const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");
/// The backdrop: wide enough for the 41-tap kernel to meet both its edges and its middle.
const W: u32 = 96;
const H: u32 = 64;

/// A backdrop with edges and colour: red squares on dark, a green ramp across, a blue ramp down.
fn backdrop() -> Vec<u8> {
    let mut px = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let red = if (x / 8 + y / 8) % 2 == 0 { 230 } else { 20 };
            px.extend_from_slice(&[red, (x * 255 / (W - 1)) as u8, (y * 255 / (H - 1)) as u8, 255]);
        }
    }
    px
}

fn to_u8(x: f32) -> u8 {
    (x * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A blur pass's push block (`Quad::blur`, the one place its layout is written down).
fn push(source: [f32; 4], step: [f32; 2], saturation: f32) -> Vec<u8> {
    let q = Quad::blur(0.0, 0.0, 1.0, 1.0, source, step, saturation);
    q.push[..8].iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// `m` run once a pixel of a `w` × `h` target (uv at the pixel's centre) over `tex`.
fn pass(m: &Module, w: u32, h: u32, tex: &Rgba8Texture, push: &[u8]) -> Vec<u8> {
    let textures: [Option<&dyn Texture>; 2] = [Some(tex), None];
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32, 0.0, 0.0];
            let inv = Invocation { inputs: &[uv], push, textures: &textures, dst: &[[0.0; 4]] };
            let o = run(m, &inv).expect("the pass runs");
            out.extend(o[0].map(to_u8));
        }
    }
    out
}

/// The chain: horizontal (full resolution in, half out), vertical (half in and out, the
/// saturation), the layer sampling it back at full resolution.
fn ours(radius: i32, saturation: f32) -> Vec<u8> {
    ours_at(radius, scale_of(radius), saturation)
}

/// The resolution the library blurs `radius` at: half below 20, quarter from 20 on (the board's
/// and the QEMU path's rule, `nexus_gfx::frame::blur_scale`).
fn scale_of(radius: i32) -> i32 {
    if radius < 20 {
        2
    } else {
        4
    }
}

fn ours_at(radius: i32, scale: i32, saturation: f32) -> Vec<u8> {
    let r = [("radius", f64::from(radius)), ("scale", f64::from(scale))];
    let h = lower_fragment(BLUR, "horizontal", &r, Blend::Replace).expect("horizontal lowers");
    let v = lower_fragment(BLUR, "vertical", &r, Blend::Replace).expect("vertical lowers");
    let layer = lower_fragment(LAYER, "main", &[("tinted", 0.0)], Blend::Replace).expect("layer");
    let (hw, hh) = (W / scale as u32, H / scale as u32);
    let src = Rgba8Texture::new(W, H, backdrop()).expect("the backdrop");
    // The horizontal pass: a reduced pixel's centre is the corner of the four texels it
    // averages; a tap `scale` texels across.
    let step = scale as f32 / W as f32;
    let across = pass(&h, hw, hh, &src, &push([1.0, 1.0, 0.0, 0.0], [step, 0.0], 1.0));
    let across = Rgba8Texture::new(hw, hh, across).expect("the horizontal result");
    let down = push([1.0, 1.0, 0.0, 0.0], [0.0, 1.0 / hh as f32], saturation);
    let blurred = pass(&v, hw, hh, &across, &down);
    let blurred = Rgba8Texture::new(hw, hh, blurred).expect("the vertical result");
    pass(&layer, W, H, &blurred, &[0; 48])
}

/// The virgl path's kernel: 2R + 1 taps, normalised.
fn kernel(radius: i32) -> Vec<f32> {
    let sigma = (radius as f32 / 2.0).max(0.5);
    let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
    let w: Vec<f32> = (-radius..=radius).map(|i| libm::exp2f(k * (i * i) as f32)).collect();
    let total: f32 = w.iter().sum();
    w.iter().map(|x| x / total).collect()
}

/// One separable pass along (dx, dy), clamped at the edges, to eight bits.
fn convolve(src: &[u8], kernel: &[f32], dx: i32, dy: i32) -> Vec<u8> {
    let r = (kernel.len() / 2) as i32;
    let mut out = vec![0u8; src.len()];
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let mut acc = [0.0f32; 4];
            for (t, w) in kernel.iter().enumerate() {
                let i = t as i32 - r;
                let sx = (x + i * dx).clamp(0, W as i32 - 1);
                let sy = (y + i * dy).clamp(0, H as i32 - 1);
                let at = ((sy as u32 * W + sx as u32) * 4) as usize;
                for (a, s) in acc.iter_mut().zip(&src[at..at + 4]) {
                    *a += w * f32::from(*s);
                }
            }
            let at = ((y as u32 * W + x as u32) * 4) as usize;
            for (o, a) in out[at..at + 4].iter_mut().zip(acc) {
                *o = a.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// The virgl blur at full resolution, then the CPU path's saturation.
fn reference(radius: i32, saturation: f32) -> Vec<u8> {
    let k = kernel(radius);
    let mut px = convolve(&convolve(&backdrop(), &k, 1, 0), &k, 0, 1);
    for p in px.chunks_exact_mut(4) {
        let [r, g, b] = [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])];
        let grey = 0.299 * r + 0.587 * g + 0.114 * b;
        for c in &mut p[..3] {
            *c = (grey + (f32::from(*c) - grey) * saturation).round().clamp(0.0, 255.0) as u8;
        }
    }
    px
}

/// The largest and the mean difference of two images, over every channel.
fn difference(a: &[u8], b: &[u8]) -> (u8, f32) {
    let d: Vec<u8> = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).collect();
    (*d.iter().max().unwrap_or(&0), d.iter().map(|x| f32::from(*x)).sum::<f32>() / d.len() as f32)
}

#[test]
fn the_half_resolution_blur_is_the_virgl_blur_within_three_levels() {
    // windowd's glass: radius 20, plain and at 140 % saturation. The half-resolution grid widens
    // the kernel by the downsample's box and the upsample's tent (a twentieth of a pixel against
    // σ = 10), and each pass rounds to eight bits as the planes do: three levels at most, about a
    // quarter of one on average. (A small radius deviates more — seven levels at radius 8 on the
    // red squares' edges — so the library builds the half-resolution blur for radius 20.)
    for saturation in [1.0, 1.4] {
        let (max, mean) = difference(&ours_at(20, 2, saturation), &reference(20, saturation));
        assert!(max <= 3 && mean < 0.35, "saturation {saturation}: max {max}, mean {mean}");
    }
}

#[test]
fn the_loops_unroll_into_their_taps_and_compile_for_the_board() {
    // Radius 20: the horizontal pass a tap every two pixels out to ±20 — 21 fetches; the vertical
    // pass the centre and five pairs a side — 11; straight-line float code, the weights the
    // pipeline's constant block.
    let r = [("radius", 20.0), ("scale", 2.0)];
    for (entry, taps) in [("horizontal", 21), ("vertical", 11)] {
        let m = lower_fragment(BLUR, entry, &r, Blend::Replace).expect("lowers");
        let samples = m.body.iter().filter(|i| matches!(i.op, Op::Sample { .. })).count();
        assert_eq!(samples, taps, "{entry}");
        assert!(!m.body.iter().any(|i| matches!(i.op, Op::Compare(..) | Op::Select(..))));
        let c = compile_fragment(BLUR, entry, &r, Blend::Replace, true).expect("compiles");
        assert!(!c.program.constants.is_empty(), "{entry}: the weights are the constant block");
    }
}

#[test]
fn the_other_radii_windowd_asks_follow_the_gaussian_and_compile_for_the_board() {
    // TASK-0215 5c-3 P4b: the board's chain at 8, 30 and 40 as well (measured on the host: radius
    // 8 up to seven levels on the red squares' edges, as on the virtual GPU's path; 30 and 40 four
    // at most). The wider passes carry more weights than the constant block's runs hold: their
    // compile is the codegen's other way — those taps a group a lane, the weights one register.
    // Since P6 radius 30 and 40 run at quarter resolution: ten levels at most, under one on
    // average (measured on the host: 7/10 levels, means 0.43–0.81).
    for (radius, max_levels, mean_levels) in [(8, 7, 0.6), (30, 10, 0.6), (40, 10, 0.9)] {
        for saturation in [1.0, 1.4] {
            let (max, mean) = difference(&ours(radius, saturation), &reference(radius, saturation));
            assert!(
                max <= max_levels && mean < mean_levels,
                "radius {radius} saturation {saturation}: max {max}, mean {mean}"
            );
        }
        let r = [("radius", f64::from(radius)), ("scale", f64::from(scale_of(radius)))];
        for entry in ["horizontal", "vertical"] {
            let c = compile_fragment(BLUR, entry, &r, Blend::Replace, true).expect("compiles");
            assert!(!c.program.constants.is_empty(), "radius {radius} {entry}");
        }
    }
}

/// TASK-0215 5c-3 P6: from radius 20 on the chain runs at quarter resolution (the stock stack's
/// `glass-quarter`) — a quarter of the pixels, a quarter of the taps. Against the
/// full-resolution gaussian it deviates more than the half-resolution chain did; the bound here
/// is what the host measures, the board's rung holds its reads to it.
#[test]
fn the_quarter_resolution_blur_follows_the_gaussian_within_its_bound() {
    for (radius, max_levels, mean_levels) in [(20, 10, 0.5), (30, 10, 0.6), (40, 10, 0.9)] {
        for saturation in [1.0, 1.4] {
            let (max, mean) =
                difference(&ours_at(radius, 4, saturation), &reference(radius, saturation));
            assert!(
                max <= max_levels && mean < mean_levels,
                "radius {radius} saturation {saturation}: max {max}, mean {mean}"
            );
        }
    }
    // The taps: radius 20 at quarter is five a side — eleven fetches across, seven down (the centre and three pairs).
    let r = [("radius", 20.0), ("scale", 4.0)];
    for (entry, taps) in [("horizontal", 11), ("vertical", 7)] {
        let m = lower_fragment(BLUR, entry, &r, Blend::Replace).expect("lowers");
        let samples = m.body.iter().filter(|i| matches!(i.op, Op::Sample { .. })).count();
        assert_eq!(samples, taps, "{entry}");
    }
}
