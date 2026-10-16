// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// A filled rounded rectangle with a vertical gradient (ADR-0072, TASK-0215 5c-2 P6): the QEMU
// path's pointer drawn before its sprite arrives. The colour runs from the top's to the bottom's
// across the rectangle, the corners are cut by the rounded rectangle's coverage (its exact
// distance, interior included), and the result is premultiplied for the premultiplied blend the
// pipeline names. No texture.

struct Gradient {
    // The straight colours at the top and at the bottom.
    top: vec4<f32>,
    bottom: vec4<f32>,
    // The rectangle's size and minus half of it: uv · size − half is the pixel's offset from its
    // centre.
    size_half: vec4<f32>,
    // The rounded rectangle: half the size less the radius, then the radius.
    corner: vec4<f32>,
}

var<immediate> gradient: Gradient;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let c = mix(gradient.top, gradient.bottom, uv.y);
    let p = fma(uv, gradient.size_half.xy, gradient.size_half.zw);
    let q = abs(p) - gradient.corner.xy;
    let sd = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - gradient.corner.z;
    let a = c.a * saturate(0.5 - sd);
    return vec4<f32>(c.rgb * a, a);
}
