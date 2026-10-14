<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# GPU fragment programs — the open compiler's code for one core, as measurement probes

A `.usc` file is the raw code of one fragment program for the core's shader cores, exactly as
the open userspace driver's compiler emitted it: the bytes its kick word points the core at,
nothing around them. gpud's cost rung (`source/drivers/gpud/src/backend/gpu/render_cost.rs`,
TASK-0215 5c-1) writes a program into the free tail of the render bundle's code arena, points a
copy of the bundle's fragment kick word at it and sets the pass type the program needs in a copy
of the draw's state block — everything else is the bundle's pass. The programs answer one
question: what a pixel costs as a function of the program (measurement README of
`docs/board/measurements/2026-10-13-gpu-render-queue/`, 5c-1 second cycle).

All four share the bundle's interface: one sampled texture at binding 0, a `vec4` tint in the
push constants, one `vec2` varying, one colour output. The PDS programs that load the texture
state, the push constants and the varyings are byte-identical across the four captures (only
the end-of-tile program moves with the code arena's layout), so the bundle's serve each. All
four need at most 20 temporaries — the five units of four the bundle's kick word allocates.

## Provenance

The open userspace driver for BVNC 36.29.52.182 (its Vulkan implementation, MIT licence; tree
`26.3.0-devel`, commit `768b18d90b`) on the host over its kernel-stand-in shim, driven by
`docs/board/measurements/2026-10-13-gpu-render-queue/capture/render_quad.c` with one quad
(`RQ_FS` names the fragment shader, `RQ_BLEND=0` turns blending off). Captured 2026-10-14; the
compiler's own statistics (`PCO_DEBUG_PRINT=stats`) give the instruction groups and temporaries.

| File | Shader (capture folder) | Blending | Groups | Temps | Pass type | Bytes | Digest (FNV-1a 64) |
|---|---|---|---|---|---|---|---|
| `frag-base.usc` | `quad.frag`: `texture(tex, uv) * tint` | source-over | 68 | 17 | punch-through (2) | 736 | `0x0fa65ca15b937497` |
| `frag-chain.usc` | `quad_chain.frag`: the same, then twelve times `c = c * tint + tint` | source-over | 116 | 17 | punch-through (2) | 1312 | `0x82b8ca6ffbb43157` |
| `frag-eft.usc` | `quad_eft.frag`: `quad.frag` with `layout(early_fragment_tests) in` | source-over | 67 | 17 | translucent (1) | 728 | `0x0c4742c89ca4bf25` |
| `frag-opaque-eft.usc` | `quad_eft.frag` | off | 55 | 18 | translucent (1) | 608 | `0x8476199eb9c2813d` |

`frag-base.usc` is byte-identical to the fragment program inside `resources/gpu/scenes/render-quad.scene`
(the bundle's code arena at offset `0x6b80`, kick word `0x0000002800007b80`).

What the compiler does that the programs show (its listing, `PCO_DEBUG_PRINT=fs`):

- Every fragment shader of an application gets an ISP feedback instruction that discards the
  helper lanes, unless the shader declares early fragment tests — and with the feedback the
  pass type is punch-through. `frag-eft` is `frag-base` without it: one group shorter, the pass
  type translucent.
- Every fragment shader gets the dynamic alpha-to-coverage path (a state bit tested at run
  time) and a sample check that reads the tile's previous colour back — so even the opaque
  program is translucent: no shader of this compiler lets the tile hardware remove a hidden
  pixel before shading.
- A 2D sample costs fourteen moves that zero the unused part of the sampler's coordinate block.

## Reproducing

```text
RQ_FS=quad_eft.frag.spv RQ_BLEND=0 PCO_DEBUG_PRINT=fs,stats PVR_SHIM_DUMP_DIR=<dir> ./render_quad 1
```

(the shim environment of the measurement README's capture recipe). The kick word at the PDS
arena's heap offset `0x2900` names the program: bits 2–31 its offset from the code heap's base
`0xe0_0000_0000`, bits 35–40 its temporaries in units of four; the bytes at that offset for the
compiler's code size are the file. The pass type is bits 24–26 of the third word of the draw's
state block (general arena `0x80_0004_b000` + `0x8208`). A new capture changes a digest: update
it here and in `userspace/nexus-gfx/tests/gpu_scene_goldens.rs` (`the_programs_are_the_captures_of_2026_10_14`).
