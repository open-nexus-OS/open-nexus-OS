# ADR-0031: Three-Layer Animation Architecture (Animation Engine + NexusGfx SDK + GPU Driver)

- Status: Accepted
- Created: 2026-05-22
- RFC: `docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md`
- Task: `tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md`

## Context

TASK-0059 delivered a production-grade CPU compositor with ShadowArena, TileMap, LayerCache,
and the `compositor/` module structure. Property changes were immediately visible — no animation
timeline, no spring physics, no keyframe interpolation.

To reach OHOS/Fuchsia-level animation quality on RISC-V without a GPU, we needed three layers:

1. **Animation engine** that interpolates layer properties per vsync tick, producing minimal dirty regions
2. **NexusGfx SDK** — a Metal-like API vocabulary (Device, Queue, CommandBuffer) to express rendering as commands
3. **GPU driver** for QEMU's virtio-gpu MMIO device — hardware cursor, scanout flip, blit offload

## Decision

We designed and implemented all three layers together as a 5-phase program:

```
Animation Engine → SceneUpdate → CPU Path (write_rows) / GPU Path (CommandBuffer → GfxBackend)
```

### Key architectural choices

1. **No separate animation thread**: Cooperative scheduling in `tick()`. Pre-allocated buffers.
2. **Fixed-timestep RK4**: Deterministic spring physics across x86_64 and riscv64.
3. **GfxBackend trait**: CpuMockBackend (host golden reference) + VirtioGpuBackend (QEMU).
   Same trait accepts a real GPU driver later without API changes.
4. **v1 minimal**: No 3D, no shader compilation, no pipeline state objects.
5. **RISC-V optimizations inline**: Fixed-point `circle_sd`, `div255` multiply+shift, `+zbb`.
6. **Every crate: `src/` + `tests/`**: No monoliths. Each file ≤500 lines.

## Consequences

- Animation engine is host-testable (no QEMU needed for Spring, Keyframe, Timeline).
- NexusGfx SDK decouples compositor from GPU model.
- GPU driver can be swapped (CpuMock ↔ VirtioGpu ↔ Real GPU) without changing windowd.
- Implicit transitions trigger on property changes — no explicit animation API needed for simple fades.
- Selftest-client is observer-only: reads markers, verifies chain, never writes.

## Proof

```bash
cargo test -p animation                    # 12/12
cargo test -p nexus-gfx                    # 4/4
cargo test -p gfx-backend                  # 4/4
cargo test -p gpud                         # 9/9
cargo check -p windowd --features os-lite  # 0 errors
cargo test -p windowd                      # 9/9
```

QEMU markers (gated):
```
uiruntime: on
uianim: timeline on
windowd: implicit transitions on
SELFTEST: ui v5 transition ok
```
