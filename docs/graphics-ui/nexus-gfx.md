---
schema: 1
title: "nexus-gfx crate"
role: reference
subsystem: ui
sources: [userspace/nexus-gfx/src, userspace/nexus-gfx/tests, tools/nxgpu]
contracts: [RFC-0059, RFC-0067, RFC-0101, ADR-0072]
verified: 2026-11-04
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# nexus-gfx crate

> **Purpose** — what `userspace/nexus-gfx` really contains, module by module, and which parts are placeholders.
> **Owns** — `userspace/nexus-gfx/`
> **Contracts** — [RFC-0059](../rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md) (the SDK and driver contract), [ADR-0072](../adr/0072-gpu-programs-wgsl-our-ir-our-backends-pipeline-library.md) (GPU programs: WGSL → our IR → per-target pipeline library), [RFC-0101](../rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md) (v2 device and scanout contract), [RFC-0067](../rfcs/RFC-0067-windowd-compositor-service-boundary-rasterizer-app-ui-extraction.md) (one rasterizer)
> **Invariants** — `raster/` is the only CPU implementation of the command semantics. Every encode and decode of a present goes through `command/validation.rs`. The planner allocates nothing per frame.
> **Read next** — [display-chain.md](display-chain.md), [animation-stack.md](animation-stack.md), [README.md](README.md)

`no_std` on the OS target (`#![cfg_attr(target_os = "none", no_std)]`). Users: windowd, gpud, `nexus-display-proto` (the present's payload) and `tools/nxgpu/` (builds pipeline libraries).

## Live modules

| path | content |
|---|---|
| `core/` | `Device`, `Queue` (`submit` consumes a `CommandBuffer`), `Fence` (unsignaled → signaled once), error and types |
| `resource/buffer.rs` | `Buffer` (write, `as_bytes`) |
| `command/buffer.rs` | `CommandBuffer` → `CommittedBuffer`; `enum Command` and its wire encode/decode — the payload of `OP_PRESENT_DAMAGE` |
| `command/render_encoder.rs` | `RenderCommandEncoder`: tiles, blits, SDF rects and gradients, drop shadow, backdrop blur, cursor blend, `composite_layer` / `composite_layer_full` |
| `command/layer.rs`, `command/layer_wire.rs` | the compositor layer SSOT: `Layer` with opacity, radius, shadow, backdrop; the `CompositeLayer` record |
| `command/validation.rs` | bounded checks of extents, counts, sizes and rects |
| `frame/` | the frame planner: the retained layer set (`layers.rs`), damage per plane (`damage.rs`), occlusion (`occlude.rs`: the planner's single-occluder cut and the compositor's exact culling, `cross` + `covered`), the glass cache and its re-blur geometry (`glass.rs`, `chain.rs`, `merge.rs`), the plan (`plan.rs`), and the plan as the board GPU's passes (`board.rs`, `pieces.rs`) |
| `raster/` | the canonical software rasterizer: `fill`, `blit`, `blur`, `blend` over a borrowed `Surface` |
| `shader/ir/` | the GPU program IR: builder, validator, reference interpreter |
| `shader/rogue/`, `shader/tgsi/` | code generators: the board GPU's encoding, and shader text for QEMU's host renderer |
| `shader/library.rs` | the pipeline library archive: one per code target, every size bounded, a wrong target or version refused whole |
| `sync/timeline.rs` | host model of the kernel's timeline fence (a monotonic `u64` point) |
| `backend/traits.rs` | `GfxBackend` (submit, create resource, transfer, set scanout, move cursor) |
| `backend/cpu_mock.rs` | `CpuMockBackend`, the host golden reference over `raster/` |
| `backend/v2/` | `GfxDevice`, the v2 contract: contexts with their own address space, buffers, batches on one timeline; `scanout.rs` is the plane ring and flip model |
| `backend/gpu/` | the board GPU's host half: firmware image and boot, firmware interface structs, command rings, MMU page tables, quads, the scene bundle |
| `backend/dc/` | the board display controller's host half: register model, EDID, cursor, frame stats |
| `perf/timer.rs` | `PipelineTimer`: per-stage durations against a budget |

`VirtioGpuBackend`, the second `GfxBackend`, lives in gpud (`source/drivers/gpud/src/backend/mod.rs`).

## Placeholders

These files hold only a CONTEXT header (`STATUS: Placeholder`) — no code:

- `command/compute_encoder.rs`, `command/blit_encoder.rs`, `command/pass.rs`
- `sync/event.rs`, `sync/barrier.rs`
- `pipeline/` (all), `cache/` (all), `transfer/` (all)
- `resource/image.rs`, `resource/sampler.rs`, `resource/heap.rs`, `resource/descriptor.rs`
- `perf/budget.rs`, `perf/counters.rs`, `perf/trace.rs`

There is no compute dispatch, no barrier or event object and no image/sampler resource yet. Don't document them as features. The planned SDK shape (compute, resources, sync, artifacts, text) is tracked in `tasks/TRACK-NEXUSGFX-SDK.md`.

## Tests

`userspace/nexus-gfx/tests/`: command buffer and render encoder, raster, frame planner (`frame_*.rs`), the board GPU model and goldens (`gpu_*.rs`), display controller goldens (`dc_goldens.rs`), shader IR and library, code generator (`rogue_*.rs`). Run `cargo test -p nexus-gfx`.
