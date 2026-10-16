<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# GPU pipelines — the compositor's pipeline library for the board GPU

`compositor.nxgp` is the compositor's pipeline library for the board GPU (RFC-0101 §7b, format v1
in `nexus_gfx::shader::library`), built by our own toolchain (ADR-0072) from `compositor.toml`:
each variant declared once — a source in `resources/gpu/shaders`, its entry point, its overrides,
its blend, its instruction-group budget. `nxgpu library` lowers each into our IR, compiles it with
the backend (`nexus_gfx::shader::rogue::codegen`) over the render bundle's register layout (W at
cf0, the first input at cf4, a texture's descriptor at sh4 with its sampler state at sh12, the
immediates at sh24), holds it to its budget, generates its setup programs (`rogue::setup`: the
kick, the varying iteration, the state load) and its state (temporaries, pass type, register
counts, the constant block), and builds the archive. The device validates it entry by entry and
places the programs in the client's context; it holds no program of its own.

| Id | Name | Source, overrides, blend | Groups / budget | Temporaries (fours) | Pass type | Program bytes |
|---|---|---|---|---|---|---|
| 1 | `layer-opaque` | `layer.wgsl` `main`, `tinted=false`, replace | 3 / 3 | 1 | opaque | 32 |
| 2 | `layer-premultiplied` | `layer.wgsl` `main`, defaults (`tinted=true`), premultiplied over | 11 / 11 | 3 | translucent | 120 |
| 3 | `layer-straight` | `layer.wgsl` `main`, `straight=true` (a client's straight-alpha texel premultiplied), premultiplied over | 15 / 15 | 3 | translucent | 160 |
| 4 | `layer-rounded` | `layer.wgsl` `main`, `straight=true`, `rounded=true` (the corners cut: the rounded rectangle's exact distance, a one-pixel band), premultiplied over | 34 / 34 | 3 | translucent | 360 |
| 5 | `shadow` | `shadow.wgsl` `main`: the blurred rounded rectangle, knocked out under the layer; no texture, premultiplied over | 43 / 43 | 3 | translucent | 472 |
| 6 | `blur-horizontal-20` | `blur.wgsl` `horizontal`, `radius=20`: the backdrop into half resolution, a tap every two pixels, each fetch averaging four texels (21 fetches); replace | 89 / 89 | 3 | opaque | 936 |
| 7 | `blur-vertical-20` | `blur.wgsl` `vertical`, `radius=20`: the half-resolution result down, two taps a fetch (11 fetches), the saturation lifted; replace | 70 / 70 | 3 | opaque | 728 |

The archive: 4748 bytes, FNV-1a 64 `0xaa0d92e858e9f2dc`, code target `0x0024001d003400b6` (the
board's core). The layers need iterate-and-sample of the device, the shadow and the blur passes
nothing; a layer's push block is 48 bytes (`Layer`: tint, size and minus half of it, the corner —
`Quad::layer` writes it), the shadow's 64 (`Shadow`: colour, the quad's size and half, the shape
with one over the blur, the offset — `Quad::shadow`), a blur pass's 32 (`Blur`: where the output
pixel lies in the source, a tap's step, the saturation — `Quad::blur`). The blur passes carry
their weights as the pipeline's constant block (46 and 47 values: a weight four times over, so
its multiply-add is one repeated group), which the state load DMAs into shared registers after
the push constants. Against the virgl path's full-resolution blur the chain (horizontal,
vertical, the layer sampling the result back) differs by three levels at most
(`tools/nxgpu/tests/blur.rs`).
The first two programs are byte for byte the hand-assembled references `opaque-itrsmp` (board
cycle 14: 1.30 ns a pixel) and `premultiplied-itrsmp` (cycle 15: 4.04 ns) of
`nexus_gfx::shader::rogue::reference`. `layer-rounded` is for a layer's corner squares only — the
compositor draws the rest of a rounded layer with `layer-straight`. Rebuilding:

```text
cargo run -p nxgpu -- library resources/gpu/pipelines/compositor.toml --out resources/gpu/pipelines/compositor.nxgp
```

A change to a source, the manifest, the frontend, the backend or the setup generator that changes
the archive fails `tools/nxgpu/tests/pipelines.rs` until the file and this table are rebuilt — and
then wants a board cycle (`gpud: gpu library ok (`, `gpud: gpu pipeline ok (`).
