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
| `layer.wgsl` | `main` (fragment) | `@location(0) uv`; immediates `Layer` (tint, size and minus half of it, the corner, the texture's rectangle); `surface` (2D texture, binding 0), `surface_sampler` (binding 1); overrides `tinted = true`, `straight = false`, `rounded = false`, `atlas = false` | a surface's texel times the layer's tint, premultiplied when `straight`, its corners cut by the rounded rectangle's coverage when `rounded`, the texel found in a rectangle of the texture when `atlas` (the QEMU path's atlas); with `tinted=false` the texel as it is |
| `shadow.wgsl` | `main` (fragment) | `@location(0) uv`; immediates `Shadow` (colour, the quad's size and half, the shape with one over the blur, the offset); no texture | a layer's drop shadow: the rounded rectangle's distance falling off over the blur, knocked out under the layer |
| `quad.wgsl` | `main` (vertex) | `@location(0) position` (clip space); outputs `@builtin(position)`, `@location(0) uv` | the QEMU path's quad: the corner as it comes, its 0..1 place across the draw's viewport as `uv` |
| `gradient.wgsl` | `main` (fragment) | `@location(0) uv`; immediates `Gradient` (top and bottom colour, size and half, the corner); no texture | a rounded rectangle filled with a vertical gradient (the QEMU path's pointer before its sprite) |
| `blur.wgsl` | `horizontal`, `vertical` (fragment) | `@location(0) uv`; immediates `Blur` (the output pixel in the source, a tap's step, the saturation); `backdrop` (2D texture, binding 0), `backdrop_sampler` (binding 1); override `radius = 20` | the glass layer's backdrop blur at half resolution: the virgl path's gaussian (σ = R/2, taps out to ±R), the horizontal pass downsampling the full-resolution backdrop, the vertical pass two taps a fetch with the saturation lifted |

Immediates (push constants) are spelled `var<immediate>` in the parser's dialect; the blend never
appears in a source — a pipeline names it. Overrides (WGSL's pipeline-overridable constants) are a
source's variants: a pipeline fixes them (keyed by `@id`, else by name), and what they fix is
evaluated while lowering — arithmetic on constants folded, a `select` or an `if` on a fixed
condition taken, a loop on a fixed counter unrolled — so the variant carries none of the paths it
switched off and a blur's taps are straight-line code with constant weights. The compiled
pipelines and their provenance: `resources/gpu/pipelines`.
