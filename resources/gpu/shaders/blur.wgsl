// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// The glass layer's backdrop blur (ADR-0072, TASK-0215 5c-2 P5d): the virgl path's separable
// gaussian — σ = R/2, taps out to ±R pixels, weights 2^(k·d²) normalised (`FS_BLUR`) — run at
// half resolution. The horizontal pass reads the full-resolution backdrop, a tap every two pixels,
// each one bilinear fetch at the corner of four texels: the fetch averages them, which is the
// downsample. The vertical pass reads the horizontal one's half-resolution result two taps a
// fetch — placed between them by their weights — and lifts the saturation (the CPU path's
// `raster::saturate`; windowd's glass asks 140 %). The glass layer samples the result with its
// corners cut (`layer.wgsl`). The radius is an override: a pipeline per radius, its loops
// unrolled and its weights constants.

struct Blur {
    // uv · source.xy + source.zw: the output pixel's centre in the source, normalised (for the
    // horizontal pass the corner of the four texels it averages).
    source: vec4<f32>,
    // .xy: a tap's step in the source, normalised (two texels across for the horizontal pass,
    // one down for the vertical); .z: the saturation (1 leaves the colour, 1.4 is 140 %).
    step: vec4<f32>,
}

var<immediate> blur: Blur;

// The blur's radius in full-resolution pixels (the virgl path's).
override radius: i32 = 20;

@group(0) @binding(0) var backdrop: texture_2d<f32>;
@group(0) @binding(1) var backdrop_sampler: sampler;

// ln 2: the weights as powers of two, as the virgl shader computes them.
const LN2: f32 = 0.6931471805599453;
// The saturation's luma weights (`raster::saturate`).
const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

@fragment
fn horizontal(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // A tap d full-resolution pixels out weighs 2^(k·d²).
    let sigma = max(f32(radius) / 2.0, 0.5);
    let k = -1.0 / (2.0 * sigma * sigma * LN2);
    let taps = radius / 2;
    var total = 0.0;
    for (var j = -taps; j <= taps; j++) {
        total += exp2(k * f32(4 * j * j));
    }
    let centre = fma(uv, blur.source.xy, blur.source.zw);
    var at = fma(blur.step.xy, vec2<f32>(f32(-taps)), centre);
    var sum = vec4<f32>(0.0);
    for (var j = -taps; j <= taps; j++) {
        let w = exp2(k * f32(4 * j * j)) / total;
        sum = fma(vec4<f32>(w), textureSample(backdrop, backdrop_sampler, at), sum);
        if j < taps {
            at += blur.step.xy;
        }
    }
    return sum;
}

@fragment
fn vertical(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let sigma = max(f32(radius) / 2.0, 0.5);
    let k = -1.0 / (2.0 * sigma * sigma * LN2);
    let taps = radius / 2;
    var total = 0.0;
    for (var j = -taps; j <= taps; j++) {
        total += exp2(k * f32(4 * j * j));
    }
    let centre = fma(uv, blur.source.xy, blur.source.zw);
    // The centre tap alone, then the taps in pairs (j, j + 1) on each side.
    var sum = textureSample(backdrop, backdrop_sampler, centre) * (1.0 / total);
    for (var j = 1; j <= taps; j += 2) {
        let w0 = exp2(k * f32(4 * j * j)) / total;
        var w1 = 0.0;
        if j < taps {
            w1 = exp2(k * f32(4 * (j + 1) * (j + 1))) / total;
        }
        let w = w0 + w1;
        let d = f32(j) + w1 / w;
        let below = fma(blur.step.xy, vec2<f32>(d), centre);
        let above = fma(blur.step.xy, vec2<f32>(-d), centre);
        sum = fma(vec4<f32>(w), textureSample(backdrop, backdrop_sampler, below), sum);
        sum = fma(vec4<f32>(w), textureSample(backdrop, backdrop_sampler, above), sum);
    }
    let grey = dot(sum.rgb, LUMA);
    return vec4<f32>(mix(vec3<f32>(grey), sum.rgb, blur.step.z), sum.a);
}
