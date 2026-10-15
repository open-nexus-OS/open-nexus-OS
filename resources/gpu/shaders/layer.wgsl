// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// The compositor's textured layer (ADR-0072, TASK-0215 5c-2): a surface's texel times the
// layer's tint — opacity and colour in one, premultiplied when the pipeline blends
// premultiplied. The blend itself is the pipeline's epilog, not this program's.

struct Layer {
    tint: vec4<f32>,
}

var<immediate> layer: Layer;

// Whether the layer is tinted. A layer at full opacity in its own colours (a tint of all ones)
// draws its texel as it is: the library's untinted variants fix this false, and their programs
// carry no multiply — on the board GPU a third of a tinted opaque layer's cost a pixel.
override tinted: bool = true;

@group(0) @binding(0) var surface: texture_2d<f32>;
@group(0) @binding(1) var surface_sampler: sampler;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let texel = textureSample(surface, surface_sampler, uv);
    return select(texel, texel * layer.tint, tinted);
}
