<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# GPU pipelines — the compositor's programs, compiled for the board GPU

The board GPU's programs for the compositor's pipelines, compiled from `resources/gpu/shaders`
by our own toolchain (ADR-0072): naga parses the WGSL, `tools/nxgpu` lowers it into our IR with
the pipeline's overrides fixed (a variant of one source), the backend
(`nexus_gfx::shader::rogue::codegen`) compiles it over the render bundle's setup programs'
register layout (W at cf0, the first input at cf4, texture state at sh4, sampler state at sh12,
the immediates at sh24). A `.usc` file is the program's bytes; until the pipeline library
(TASK-0215 5c-2 P4, RFC-0101 §7b) carries them, gpud's table names the temporaries and pass type
beside each.

| File | Source, overrides, blend | Groups | Temporaries (units of four) | Pass type | Bytes | Digest (FNV-1a 64) |
|---|---|---|---|---|---|---|
| `layer-opaque.usc` | `layer.wgsl` `main`, `tinted=false`, replace | 3 | 1 | opaque | 32 | `0xfc3a43a9c89899e6` |
| `layer-premultiplied.usc` | `layer.wgsl` `main`, defaults (`tinted=true`), premultiplied over | 11 | 3 | translucent | 120 | `0x20e4e05b2d8fff4d` |

The opaque layer is a surface at full opacity in its own colours — the compositor's commonest
draw — so its variant carries no tint multiply (on the board a third of a tinted opaque layer's
cost a pixel, cycle 14: 1.97 ns against 1.34 ns). Both files are byte for byte the
hand-assembled references `opaque-itrsmp` (board cycle 14: 1.30 ns a pixel) and
`premultiplied-itrsmp` of `nexus_gfx::shader::rogue::reference`; `tools/nxgpu/tests/pipelines.rs`
checks both directions. Rebuilding:

```text
cargo run -p nxgpu -- build resources/gpu/shaders/layer.wgsl --entry main --override tinted=false --blend replace --out resources/gpu/pipelines/layer-opaque.usc
cargo run -p nxgpu -- build resources/gpu/shaders/layer.wgsl --entry main --blend premultiplied --out resources/gpu/pipelines/layer-premultiplied.usc
```

A change to the source, the frontend or the backend that changes a program fails the test until
the file and this table are rebuilt — and then wants a board cycle (`gpud: gpu pipeline ok (`).
