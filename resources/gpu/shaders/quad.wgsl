// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// The QEMU path's quad (ADR-0072, TASK-0215 5c-2 P6): the vertex program every draw of the
// virtual GPU runs. A draw sets its viewport to the rectangle it covers and sends the quad's
// corners in clip space; the program passes them on and gives the fragment programs `uv`, the
// pixel's place across the rectangle (0 at its top left, 1 at its bottom right).

struct Corner {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn main(@location(0) position: vec4<f32>) -> Corner {
    return Corner(position, fma(position.xy, vec2<f32>(0.5), vec2<f32>(0.5)));
}
