// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// The glass layer's backdrop blur (ADR-0072, TASK-0215 5c-2 P5d): the separable gaussian of the
// kernel table (`nexus_gfx::frame::kernels`, ADR-0094 — the table alone says how wide it is and
// what each tap weighs; nothing of that is written here) run at a reduced resolution, on both
// paths (half for the smallest library entry, quarter from the second on — TASK-0215 5c-3 P6,
// the stock stack's `glass-quarter`). The horizontal pass reads the full-resolution
// backdrop, a tap every `scale` pixels, each one bilinear fetch at the corner of four texels:
// the fetch averages them, which is the downsample. The vertical pass reads the horizontal one's
// reduced result two taps a fetch — placed between them by their weights — and lifts the
// saturation (the CPU path's `raster::saturate`; windowd's glass asks 140 %). The glass layer
// samples the result with its corners cut (`layer.wgsl`). The taps and their weights are
// overrides `nxgpu` emits from the table for each library entry at its resolution: a pipeline
// per entry, its loops unrolled and its weights constants.

struct Blur {
    // uv · source.xy + source.zw: the output pixel's centre in the source, normalised (for the
    // horizontal pass the corner of the four texels it averages).
    source: vec4<f32>,
    // .xy: a tap's step in the source, normalised (`scale` texels across for the horizontal
    // pass, one down for the vertical); .z: the saturation (1 leaves the colour, 1.4 is 140 %).
    step: vec4<f32>,
}

var<immediate> blur: Blur;

// The kernel's taps a side at this pipeline's resolution, and its normalised weights from the
// centre out (`w0` the centre, `w10` the tenth tap: the table's `MAX_TAPS`). A weight past the
// taps is never read.
override taps: i32 = 5;
override w0: f32 = 1.0;
override w1: f32 = 0.0;
override w2: f32 = 0.0;
override w3: f32 = 0.0;
override w4: f32 = 0.0;
override w5: f32 = 0.0;
override w6: f32 = 0.0;
override w7: f32 = 0.0;
override w8: f32 = 0.0;
override w9: f32 = 0.0;
override w10: f32 = 0.0;

@group(0) @binding(0) var backdrop: texture_2d<f32>;
@group(0) @binding(1) var backdrop_sampler: sampler;

// The saturation's luma weights (`raster::saturate`).
const LUMA: vec3<f32> = vec3<f32>(0.299, 0.587, 0.114);

@fragment
fn horizontal(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let centre = fma(uv, blur.source.xy, blur.source.zw);
    var at = fma(blur.step.xy, vec2<f32>(f32(-taps)), centre);
    var sum = vec4<f32>(0.0);
    for (var j = -taps; j <= taps; j++) {
        // The tap's weight: the table's at its distance (fixed by the pipeline, taken while
        // lowering — no branch in the program).
        var k = j;
        if k < 0 { k = -k; }
        var w = w0;
        if k == 1 { w = w1; }
        if k == 2 { w = w2; }
        if k == 3 { w = w3; }
        if k == 4 { w = w4; }
        if k == 5 { w = w5; }
        if k == 6 { w = w6; }
        if k == 7 { w = w7; }
        if k == 8 { w = w8; }
        if k == 9 { w = w9; }
        if k == 10 { w = w10; }
        sum = fma(vec4<f32>(w), textureSample(backdrop, backdrop_sampler, at), sum);
        if j < taps {
            at += blur.step.xy;
        }
    }
    return sum;
}

@fragment
fn vertical(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let centre = fma(uv, blur.source.xy, blur.source.zw);
    // The centre tap alone, then the taps in pairs (j, j + 1) on each side: one fetch between
    // them, placed by their weights.
    var sum = textureSample(backdrop, backdrop_sampler, centre) * w0;
    var first = 0.0;
    for (var j = 1; j <= taps; j++) {
        var w = w0;
        if j == 1 { w = w1; }
        if j == 2 { w = w2; }
        if j == 3 { w = w3; }
        if j == 4 { w = w4; }
        if j == 5 { w = w5; }
        if j == 6 { w = w6; }
        if j == 7 { w = w7; }
        if j == 8 { w = w8; }
        if j == 9 { w = w9; }
        if j == 10 { w = w10; }
        var pair = 0.0;
        var d = 0.0;
        if j - (j / 2) * 2 == 1 {
            // An odd tap opens a pair; the last one alone when the taps are odd.
            first = w;
            if j == taps {
                pair = w;
                d = f32(j);
            }
        } else {
            pair = first + w;
            d = f32(j - 1) + w / pair;
        }
        if pair > 0.0 {
            let below = fma(blur.step.xy, vec2<f32>(d), centre);
            let above = fma(blur.step.xy, vec2<f32>(-d), centre);
            sum = fma(vec4<f32>(pair), textureSample(backdrop, backdrop_sampler, below), sum);
            sum = fma(vec4<f32>(pair), textureSample(backdrop, backdrop_sampler, above), sum);
        }
    }
    let grey = dot(sum.rgb, LUMA);
    return vec4<f32>(mix(vec3<f32>(grey), sum.rgb, blur.step.z), sum.a);
}
