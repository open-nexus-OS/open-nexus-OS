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
places the programs in the client's context; it holds no program of its own. windowd carries both
archives (`compositor.nxgp` for the board's GPU, `compositor-virgl.nxgp` for QEMU's host
renderer) and moves the one whose header names the device's code target to gpud when it opens
its context (TASK-0215 5c-3 P3, RFC-0101 §7b).

| Id | Name | Source, overrides, blend | Groups / budget | Temporaries (fours) | Pass type | Program bytes |
|---|---|---|---|---|---|---|
| 1 | `layer-opaque` | `layer.wgsl` `main`, `tinted=false`, replace | 3 / 3 | 1 | opaque | 32 |
| 2 | `layer-premultiplied` | `layer.wgsl` `main`, defaults (`tinted=true`), premultiplied over | 11 / 11 | 3 | translucent | 120 |
| 3 | `layer-straight` | `layer.wgsl` `main`, `straight=true` (a client's straight-alpha texel premultiplied), premultiplied over | 15 / 15 | 3 | translucent | 160 |
| 4 | `layer-rounded` | `layer.wgsl` `main`, `straight=true`, `rounded=true` (the corners cut: the rounded rectangle's exact distance, a one-pixel band), premultiplied over | 34 / 34 | 3 | translucent | 360 |
| 5 | `shadow` | `shadow.wgsl` `main`: the blurred rounded rectangle, knocked out under the layer; no texture, premultiplied over | 43 / 43 | 3 | translucent | 472 |
| 6 | `blur-horizontal-20` | `blur.wgsl` `horizontal`, `radius=20`, `scale=4`: the backdrop into quarter resolution, a tap every four pixels, each fetch averaging four texels (11 fetches); replace | 49 / 49 | 3 | opaque | 512 |
| 7 | `blur-vertical-20` | `blur.wgsl` `vertical`, `radius=20`, `scale=4`: the quarter-resolution result down, two taps a fetch (7 fetches), the saturation lifted; replace | 50 / 50 | 3 | opaque | 520 |
| 8 | `blur-horizontal-8` | the horizontal pass at `radius=8`, `scale=2` (a subtle glass, half resolution); replace | 41 / 41 | 3 | opaque | 432 |
| 9 | `blur-vertical-8` | the vertical pass at `radius=8`, `scale=2`; replace | 40 / 40 | 3 | opaque | 416 |
| 10 | `blur-horizontal-30` | the horizontal pass at `radius=30`, `scale=4` (windowd's default glass); replace | 65 / 65 | 3 | opaque | 680 |
| 11 | `blur-vertical-30` | the vertical pass at `radius=30`, `scale=4`; replace | 60 / 60 | 3 | opaque | 624 |
| 12 | `blur-horizontal-40` | the horizontal pass at `radius=40`, `scale=4` (panels and overlays); replace | 89 / 89 | 3 | opaque | 936 |
| 13 | `blur-vertical-40` | the vertical pass at `radius=40`, `scale=4`; replace | 70 / 70 | 3 | opaque | 728 |

The archive: 10 012 bytes, FNV-1a 64 `0x853e5fe7d5cff61f`, code target `0x0024001d003400b6` (the
board's core). Ids 8–13 (TASK-0215 5c-3 P4b) are the blur at the other radii windowd's glass asks;
the wider ones carry more weights than the constant block's runs of four hold (a block holds 128
values), so the codegen keeps their taps a group a lane and each weight one register. On the host
the chain follows the full-resolution gaussian within seven levels at radius 8 (half resolution,
as on the virtual GPU) and ten at 20, 30 and 40 — quarter resolution since TASK-0215 5c-3 P6, the
stock stack's `glass-quarter`: a quarter of the pixels and of the taps (`tools/nxgpu/tests/blur.rs`). The layers need iterate-and-sample of the device, the shadow and the blur passes
nothing; a layer's push block is 64 bytes (`Layer`: tint, size and minus half of it, the corner,
the texture's rectangle an atlas variant reads — `Quad::layer` and `with_texture` write it), the
shadow's 64 (`Shadow`: colour, the quad's size and half, the shape
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

## The QEMU path's library — `compositor-virgl.nxgp`

The same WGSL sources built for the QEMU path's virtual GPU (TASK-0215 5c-2 P6) from
`compositor-virgl.toml`: `nxgpu library` lowers each into our IR and compiles it with the
`tgsi` backend (`nexus_gfx::shader::tgsi`) into the shader text the host renderer parses. A
fragment pipeline's main part is the program — its blend is the pipeline's state, which the
virtual GPU's blend unit applies (`tgsi::State`); a budget counts instructions; the vertex
program every draw runs is an entry of its own. Code target `TGSI` v1 (`tgsi::CODE_TARGET`).

| Id | Name | Source, overrides | Blend | Instructions |
|---|---|---|---|---|
| 1 | `quad` | `quad.wgsl` (vertex): the corner as it comes, its 0..1 place as `uv` | — | 3 |
| 2 | `layer-opaque` | `layer.wgsl`, `tinted=false` (the wallpaper) | replace | 2 |
| 3 | `layer-tinted` | `layer.wgsl` (the splash's pulse) | replace | 3 |
| 4 | `layer-atlas` | `layer.wgsl`, `straight`, `rounded`, `atlas` (a client's surface from the atlas; the blurred backdrop drawn back; the blur cache's hits) | premultiplied | 21 |
| 5 | `layer-premultiplied` | `layer.wgsl` (the pointer's sprite) | premultiplied | 3 |
| 6 | `shadow` | `shadow.wgsl` | premultiplied | 27 |
| 7–14 | `blur-horizontal-R`, `blur-vertical-R` | `blur.wgsl`, `radius` = 8, 20, 30, 40 (windowd's radii) | replace | 29/23, 65/41, 95/59, 125/71 |
| 15 | `gradient` | `gradient.wgsl` (the pointer before its sprite) | premultiplied | 20 |

The archive: 24 660 bytes, FNV-1a 64 `0xa747afcaf7d3cd00`. Rebuilding:

```text
cargo run -p nxgpu -- library resources/gpu/pipelines/compositor-virgl.toml --out resources/gpu/pipelines/compositor-virgl.nxgp
```

`tools/nxgpu/tests/tgsi.rs` holds it to its manifest byte for byte, every entry to its stage's
text and its blend.

gpud takes it in (`source/drivers/gpud/src/virgl_pipelines.rs`): the archive parsed for this target
and every program's text checked before a byte reaches the device; each program one shader object
of the virgl context (a long one sent in chunks of 4 000 bytes, each its own command, as the host
renderer's long-shader protocol takes it); the pipelines it draws with resolved by name — the blur
pair nearest the radius windowd asks. A draw binds the quad's vertex program, the pipeline's
program and blend, its push block as the fragment stage's constant buffer and its texture through
the one bilinear sampler, over a viewport that is the quad's rectangle. Every draw of the virgl
path is one of these; the probe's readback proofs draw `gradient` (`gpud: virgl draw ok`,
`gpud: virgl gradient ok` — red at the top, blue at the bottom) and `layer-atlas`
(`gpud: layer composite ok`).
