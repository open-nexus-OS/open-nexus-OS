// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// The compositor's textured layer (ADR-0072, TASK-0215 5c-2): a surface's texel times the
// layer's tint (colour and opacity), its corners cut by the rounded rectangle's coverage, the
// result premultiplied for the premultiplied blend the pipeline links after it. The variants
// are overrides: a pipeline fixes them, and the paths it switches off are never compiled.

struct Layer {
    // A multiplier on the texel: the colour in rgb, the opacity in a.
    tint: vec4<f32>,
    // The layer's size in pixels and minus half of it: uv · size − half is the pixel's offset
    // from the layer's centre.
    size_half: vec4<f32>,
    // The rounded rectangle: half the size less the radius, then the radius.
    corner: vec4<f32>,
}

var<immediate> layer: Layer;

// Whether the layer is tinted. A layer at full opacity in its own colours (a tint of all ones)
// draws its texel as it is, and carries no multiply — on the board GPU a third of a tinted
// opaque layer's cost a pixel.
override tinted: bool = true;
// Whether the surface's texels are straight alpha (a client's surface): premultiplied here.
override straight: bool = false;
// Whether the corners are cut: the coverage of the rounded rectangle at the pixel's centre, a
// one-pixel band across its edge. The compositor draws only a layer's corner squares this way.
override rounded: bool = false;

@group(0) @binding(0) var surface: texture_2d<f32>;
@group(0) @binding(1) var surface_sampler: sampler;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let texel = textureSample(surface, surface_sampler, uv);
    let c = select(texel, texel * layer.tint, tinted);
    // The rounded rectangle's signed distance (zero on its edge, negative inside).
    let p = fma(uv, layer.size_half.xy, layer.size_half.zw);
    let q = abs(p) - layer.corner.xy;
    let sd = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - layer.corner.z;
    let coverage = saturate(0.5 - sd);
    let a = select(c.a, c.a * coverage, rounded);
    let rgb = select(select(c.rgb, c.rgb * coverage, rounded), c.rgb * a, straight);
    return select(c, vec4<f32>(rgb, a), straight | rounded);
}
