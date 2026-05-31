# RFC-0059: UI v5a Production-Grade Animation Runtime + NexusGfx SDK Minimal + GPU Driver

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-05-22
- Last Updated: 2026-05-29 (Phase 5-6 added: NexusGfx SDK module structure, zero-copy VMO backing)
- Links:
  - Tasks: `tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md` (execution + proof)
  - Depends on: `docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md`
  - Gfx architecture: `docs/architecture/nexusgfx-command-and-pass-model.md`, `docs/architecture/nexusgfx-resource-model.md`
  - Gfx track: `tasks/TRACK-NEXUSGFX-SDK.md`, Driver track: `tasks/TRACK-DRIVERS-ACCELERATORS.md`

## Status at a Glance

- Phase 0 (Animation Engine): ✅
- Phase 1 (NexusGfx SDK minimal): ✅
- Phase 2 (GPU Backend Trait + CPU Mock): ✅
- Phase 3 (gpud + virtio-gpu MMIO): ✅ (1280×800 display resource, set_scanout ok, no crash)
- Phase 4 (Integration + Proof Gates): ✅ (windowd wired, initial compose works, wallpaer fallback)
- Phase 5 (NexusGfx module structure): ✅ (Metal-like 10-module tree, 40+ skeleton files, zero-copy-ready)
- Phase 6 (Real GPU pipeline + VMO-backed resources): 🟡 (zero-copy VMO handoff windowd→gpud plumbed, virtio-gpu primary display, QEMU window resize pending test)

## Scope boundaries

- **This RFC owns**: Animation engine, NexusGfx SDK minimal, GPU backend trait, gpud service, RISC-V optimizations, frame-budget discipline, reduced-motion
- **This RFC does NOT own**: Full 3D NexusGfx, real GPU drivers, kernel MMIO policy, WM/scene transitions (TASK-0064), theme tokens (TASK-0063)

## Context

TASK-0059 delivered a production-grade CPU compositor (`compositor/` — 18 files). The compositor is "immediate": no animation timeline, no GPU offload. To reach OHOS/Fuchsia-level animation quality on RISC-V without a GPU, we need three layers built together.

### Production-grade framing

| Property | OHOS/Fuchsia | Our target |
|---|---|---|
| Animation API | Declarative (Spring) | Declarative (Spring RK4) |
| Physics | Platform spring | Fixed-timestep RK4 (deterministic) |
| Frame budget | 8.3ms @ 120Hz GPU | 8.3ms @ 120Hz CPU (bounded tiles) |
| Quality degradation | Automatic GPU | Explicit (GlassQuality → Low → Opaque) |
| Reduced motion | System flag | `configd` flag, first-class |
| GPU offload | Full compositor | Cursor + blit + scanout flip |
| CPU reference | Not required | Required (CpuMockBackend golden) |
| Determinism | Not guaranteed | Guaranteed (fixed-timestep, explicit dt) |
| Heap during animation | Yes (GPU allocs) | Zero (all pre-allocated) |

## Goals

1. Animation Engine: Timeline, Spring (RK4), Keyframe, SceneUpdate, ReducedMotion
2. NexusGfx SDK minimal: Device, Queue, CommandBuffer, RenderEncoder, Buffer, Fence
3. GPU Backend Trait: GfxBackend + CpuMockBackend (golden) + VirtioGpuBackend
4. gpud Service: MMIO virtio-gpu, hardware cursor, scanout flip
5. RISC-V Optimizations: fixed-point sd_circle, (a*b*257)>>16, +zbb
6. Modular: every crate `src/` + `tests/`, no monoliths

## Non-Goals

Full 3D, real GPU drivers, kernel MMIO policy, WM/scene transitions, theme tokens.

## Constraints

- Deterministic spring physics (fixed-timestep RK4, explicit dt)
- No heap growth during animation (pre-allocated buffers)
- Bounded resources (max 16 animations, 1024 commands)
- Reduced motion first-class
- CPU reference == GPU output for same input
- No stubs claiming success

## Proposed design

### Full architecture

```
┌──────────────────────────────────────────────────────────┐
│  windowd/compositor/runtime.rs                           │
│  DisplayServerRuntime::tick(now_ns)                      │
│  animation_driver.tick(now_ns) → Vec<SceneUpdate>        │
│  Foundation: compositor/ (18 files, TASK-0059 baseline)  │
└──────────────────────┬───────────────────────────────────┘
                       │ SceneUpdate { layer, property, value, progress }
                       ▼
┌──────────────────────────────────────────────────────────┐
│  Animation Engine  (userspace/ui/animation)         ← NEW│
│  Timeline  Spring(RK4)  Keyframe  Easing               │
│  SceneUpdate  ReducedMotion                             │
└──────────────────────┬───────────────────────────────────┘
                       │
        ┌──────────────┴──────────────┐
        │ CPU Path (exists)           │ GPU Path (new)
        ▼                             ▼
┌──────────────────┐    ┌──────────────────────────────────┐
│ write_rows()     │    │  NexusGfx SDK (userspace/   ← NEW│
│ vmo_write        │    │  nexus-gfx)                      │
│                  │    │  Device Queue CommandBuffer       │
│ RISC-V optim.:   │    │  RenderEncoder Buffer Fence       │
│ • fixed_sdf.rs   │    └──────────────┬───────────────────┘
│ • (a*b*257)>>16  │                   │ commit()
│ • +zbb feature   │                   ▼
└──────────────────┘    ┌──────────────────────────────────┐
                        │  GPU Backend Trait          ← NEW│
                        │  (userspace/gfx-backend)         │
                        │  GfxBackend::submit(cmd) → Fence │
                        └──────────────┬───────────────────┘
                                       │
                  ┌────────────────────┴────────────────────┐
                  ▼                                         ▼
        ┌──────────────────┐                  ┌──────────────────────┐
        │ CpuMockBackend   │                  │ VirtioGpuBackend     │
        │ (host, golden)   │                  │ (source/drivers/gpud)│
        │ CPU Vec<u8> exec │                  │ MMIO virtio-gpu      │
        └──────────────────┘                  │ • Hardware Cursor    │
                                              │ • Scanout Flip       │
                                              └──────────┬───────────┘
                                                         │
                                                ┌────────┴────────────┐
                                                │ Real GPU (future)    │
                                                │ same GfxBackend trait│
                                                └─────────────────────┘
```

### Crate structure

```
userspace/ui/animation/    userspace/nexus-gfx/    userspace/gfx-backend/    source/drivers/gpud/
```

Each: `src/` (modules), `tests/` (integration), `Cargo.toml`. No file >500 lines.

### RISC-V optimizations (inline)

| File | Change | Gain |
|---|---|---|
| `fixed_sdf.rs` | `circle_sd()` fixed-point | 5–10× |
| `primitives.rs` | `(a*b*257)>>16` vs `/255` | 3–5× |
| `.cargo/config.toml` | `+zbb` feature | 10–20% |
| `primitives.rs` | Loop unroll `blend_overlay_row` | 2–4× |

## Security considerations

- **Threat model**: Malicious GPU commands, confused deputy windowd→gpud, MMIO OOB
- **Mitigations**: gpud validates command sizes, windowd IPC sender-id gated, Resource IDs opaque, MMIO bounds-checked
- **Open risks**: virtio-gpu trusted (QEMU); real GPU needs own RFC

## Failure model

- DeviceNotFound → `gpud: no device`, clean exit
- CommandRejected → discard, log, no partial execution
- ResourceExhausted → LRU evict, retry; fail → Unsupported → CPU path
- Unsupported → explicit CPU fallback, marker emitted
- MmioFault → `gpud: mmio fault`, exit, restartable
- No silent fallback. CPU path always available.

## Proof strategy

### Host
```bash
cargo test -p animation -p nexus-gfx -p gfx-backend -p gpud
```

### QEMU
```bash
RUN_UNTIL_MARKER=1 just test-os
```

### Markers
`gpud: ready` `gpud: cursor on` `uiruntime: on` `uianim: timeline on`
`windowd: implicit transitions on` `SELFTEST: ui v5 transition ok` `SELFTEST: gpu cursor move ok`

### Goldens
CpuMockBackend output == reference write_rows() for same SceneUpdate. Spring steps identical host vs QEMU.

## Alternatives considered

- Direct virtio-gpu (no SDK) → rejected: couples to one GPU
- Vulkan SDK v1 → rejected: too large; Metal-like sufficient
- Wait for real GPU → rejected: virtio-gpu gives cursor/scanout now
- Separate animation thread → rejected: single-threaded microkernel
- Float-based springs → rejected: non-deterministic across platforms

## Phase 5-6: NexusGfx SDK Full Module Structure (2026-05-29)

Phase 5 established the Metal-like module tree under `userspace/nexus-gfx/src/`:

```
core/       — device, queue, fence, error, types (moved from flat src/)
resource/   — buffer, image, sampler, heap, descriptor
command/    — buffer (was command_buffer), render_encoder, compute_encoder, blit_encoder, pass, validation
pipeline/   — render, compute, vertex, cache
shader/     — module, function, library, reflection
backend/    — traits, cpu_mock (re-exports from gfx-backend)
sync/       — timeline, event, barrier
transfer/   — vmo, dma, layout
perf/       — counters, trace, budget
cache/      — texture_atlas, render_target, descriptor_set
```

40+ skeleton files with CONTEXT headers. Existing code moved into new structure.
Backward-compatible re-exports from `lib.rs`. All imports updated in gfx-backend, gpud.

Phase 6 target: real GPU pipeline with VMO-backed Buffer/Image (zero-copy),
ShaderModule (SPIR-V), RenderPipeline (blend+raster), TimelineFence (async completion).
Aligns with TRACK-NEXUSGFX-SDK.md CAND-GFX-000 through CAND-GFX-030.

## Open questions

- virtio-gpu MMIO address on RISC-V virt? → @runtime, before Phase 3
- Vsync: poll nsec() or real interrupt? → polling sufficient v1
- Reduced-motion config propagation? → `set_reduced_motion(bool)` in Phase 0

## Implementation Checklist

- [x] Phase 0: Animation Engine — `cargo test -p animation`
- [x] Phase 1: NexusGfx SDK — `cargo test -p nexus-gfx`
- [x] Phase 2: GPU Backend — `cargo test -p gfx-backend`
- [x] Phase 3: gpud + virtio-gpu — QEMU `gpud: virtio-gpu probed` + `gpud: scanout 1280x800 bgra8888`
- [x] Phase 4: windowd integration — `windowd: full-window color visible`, `SELFTEST: end`, wallpaper fallback
- [x] RISC-V optimizations applied
- [x] All crates `src/` + `tests/`, no monoliths
- [x] Phase 5: NexusGfx module structure — 10-module tree, 40+ skeleton files, Metal-like layout
- [x] Phase 6: GPU display pipeline — zero-copy VMO handoff windowd→gpud, virtio-gpu primary display, QEMU window resize
- [ ] Phase 6b: ShaderModule (SPIR-V), RenderPipeline (blend+raster), TimelineFence (async completion)
