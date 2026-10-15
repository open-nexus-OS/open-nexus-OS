<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# GPU program sources — the compositor's pipelines in WGSL

The sources of the system's GPU programs (ADR-0072). `tools/nxgpu` parses and validates each with
naga (host only), lowers it into the IR of `nexus_gfx::shader::ir`, and links the epilog the
pipeline's blend names (replace, straight-alpha over, premultiplied over) — the GPU has no blend
unit, so blending is program code. The backends compile the IR per code target into the pipeline
library (RFC-0101 §7b).

| Source | Entry | Interface | What it draws |
|---|---|---|---|
| `layer.wgsl` | `main` (fragment) | `@location(0) uv`; immediates `tint: vec4`; `surface` (2D texture, binding 0), `surface_sampler` (binding 1) | a surface's texel times the layer's tint |

Immediates (push constants) are spelled `var<immediate>` in the parser's dialect; the blend never
appears in a source — a pipeline names it.
