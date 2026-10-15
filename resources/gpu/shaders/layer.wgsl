// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// The compositor's textured layer (ADR-0072, TASK-0215 5c-2): a surface's texel times the
// layer's tint — opacity and colour in one, premultiplied when the pipeline blends
// premultiplied. The blend itself is the pipeline's epilog, not this program's.

struct Layer {
    tint: vec4<f32>,
}

var<immediate> layer: Layer;

@group(0) @binding(0) var surface: texture_2d<f32>;
@group(0) @binding(1) var surface_sampler: sampler;

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return textureSample(surface, surface_sampler, uv) * layer.tint;
}
