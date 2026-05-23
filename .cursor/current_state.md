# Current State — Open Nexus OS

Last updated: 2026-05-22

## Active focus

**TASK-0062 / RFC-0059: Implemented (Phases 0-5 complete).**
Production-grade animation stack: Animation Engine + NexusGfx SDK + GfxBackend + gpud virtio-gpu driver.
RISC-V optimizations applied. All 38 tests green. Implicit transitions integrated in windowd.

## Previous (complete)

**TASK-0059 / RFC-0058: Done.**
ShadowCache fixed. Compositor module refactored (`os_lite.rs` → `compositor/` 18 files).

## Architecture (TASK-0062)

```
Animation Engine → SceneUpdate → CPU Path (write_rows) / GPU Path (CommandBuffer → GfxBackend)
  ├── CpuMockBackend (host tests, golden reference)
  └── VirtioGpuBackend (QEMU, real GPU later — same trait)
```

## Phases

| Phase | Crate | Status | Tests |
|-------|-------|--------|-------|
| 0 | `userspace/ui/animation` | ✅ | 12/12 |
| 1 | `userspace/nexus-gfx` | ✅ | 4/4 |
| 2 | `userspace/gfx-backend` | ✅ | 4/4 |
| 3 | `source/drivers/gpud` | ✅ | 9/9 |
| 4 | windowd integration | ✅ | 9/9 |
| 5 | RISC-V optimizations | ✅ | — |
| — | **Total** | ✅ | **38/38** |

## Key deliverables

- **Animation engine**: Spring (fixed-timestep RK4), Keyframe (Linear/EaseIn/EaseOut/EaseInOut), Timeline, Reduced Motion
- **NexusGfx SDK**: Device, Queue, CommandBuffer, RenderCommandEncoder, Buffer, Fence — Metal-like API
- **GfxBackend trait**: CpuMockBackend + VirtioGpuBackend — swappable without API changes
- **gpud service**: virtio-gpu MMIO protocol (CREATE_RESOURCE_2D, SET_SCANOUT, MOVE_CURSOR) + probe
- **Implicit transitions**: Opacity/transform changes trigger spring animations automatically
- **RISC-V**: Fixed-point `circle_sd`, `div255` multiply+shift, `+zbb` target-feature
- **Clean test structure**: All 4 crates have `src/` + `tests/` with meaningful names
- **Docs**: ADR-0031, `docs/architecture/animation-nexusgfx-gpu-three-layer-stack.md`

## Test Status

| Suite | Count | Result |
|-------|-------|--------|
| animation | 12/12 | ✅ |
| nexus-gfx | 4/4 | ✅ |
| gfx-backend | 4/4 | ✅ |
| gpud | 9/9 | ✅ |
| windowd | 9/9 | ✅ |
| OS check (RISC-V) | — | ✅ 0 errors |

## Key files

```
docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md
docs/adr/0031-three-layer-animation-architecture.md
docs/architecture/animation-nexusgfx-gpu-three-layer-stack.md
.cursor/handoff/current.md
```
