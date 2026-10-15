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
counts), and builds the archive. The device validates it entry by entry and places the programs
in the client's context; it holds no program of its own.

| Id | Name | Source, overrides, blend | Groups / budget | Temporaries (fours) | Pass type | Program bytes |
|---|---|---|---|---|---|---|
| 1 | `layer-opaque` | `layer.wgsl` `main`, `tinted=false`, replace | 3 / 3 | 1 | opaque | 32 |
| 2 | `layer-premultiplied` | `layer.wgsl` `main`, defaults (`tinted=true`), premultiplied over | 11 / 11 | 3 | translucent | 120 |

The archive: 624 bytes, FNV-1a 64 `0x1084b1990109bdfc`, code target `0x0024001d003400b6` (the
board's core). Both programs are byte for byte the hand-assembled references `opaque-itrsmp`
(board cycle 14: 1.30 ns a pixel) and `premultiplied-itrsmp` (cycle 15: 4.04 ns) of
`nexus_gfx::shader::rogue::reference`; both need iterate-and-sample of the device. Rebuilding:

```text
cargo run -p nxgpu -- library resources/gpu/pipelines/compositor.toml --out resources/gpu/pipelines/compositor.nxgp
```

A change to a source, the manifest, the frontend, the backend or the setup generator that changes
the archive fails `tools/nxgpu/tests/pipelines.rs` until the file and this table are rebuilt — and
then wants a board cycle (`gpud: gpu library ok (`, `gpud: gpu pipeline ok (`).
