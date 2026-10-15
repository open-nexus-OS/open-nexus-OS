// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// A layer's drop shadow (ADR-0072, TASK-0215 5c-2 P5c): the rounded rectangle blurred — its
// distance outside the shape at the shadow's offset falls off over the blur, squared — and
// knocked out where the layer itself lies, so nothing of it shows under a translucent window
// (the QEMU path's shadow, `virgl_vector.rs`). The colour is straight; the result is
// premultiplied for the premultiplied blend the pipeline links after it. No texture.

struct Shadow {
    // The shadow's straight colour and its alpha.
    colour: vec4<f32>,
    // The quad's size and minus half of it — the quad centred on the shifted shape: uv · size −
    // half is the pixel's offset from the shadow's centre.
    size_half: vec4<f32>,
    // The rounded rectangle: half the shape's size less the radius, the radius, one over the blur.
    shape: vec4<f32>,
    // The shadow's offset from the layer: a pixel's offset from the layer's centre is its offset
    // from the shadow's plus this.
    offset: vec4<f32>,
}

var<immediate> shadow: Shadow;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // The distance outside the shifted shape, falling off over the blur.
    let p = fma(uv, shadow.size_half.xy, shadow.size_half.zw);
    let d = length(max(abs(p) - shadow.shape.xy, vec2<f32>(0.0))) - shadow.shape.z;
    let ramp = saturate(1.0 - d * shadow.shape.w);
    // The knockout: nothing inside the layer's own (unshifted) shape, a pixel's ramp across it.
    let u = p + shadow.offset.xy;
    let inside = length(max(abs(u) - shadow.shape.xy, vec2<f32>(0.0))) - shadow.shape.z;
    let knock = saturate(inside);
    let a = shadow.colour.a * ramp * ramp * knock;
    return vec4<f32>(shadow.colour.rgb * a, a);
}
