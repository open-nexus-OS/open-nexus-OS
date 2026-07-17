# Three-Layer Animation Stack

Date: 2026-05-22
Status: Implemented (TASK-0062 Phase 0-5)
RFC: `docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md`

## Architecture diagram

```
windowd/compositor/runtime.rs
  DisplayServerRuntime::tick(now_ns)
  animation_driver.tick(now_ns) → Vec<SceneUpdate>
       │
       │ Foundation: compositor/ (18 files, TASK-0059 baseline)
       │
       ▼
Animation Engine (userspace/ui/animation)          ← Phase 0
  Timeline  Spring(RK4)  Keyframe  SceneUpdate
  Zero knowledge of rendering — only property values
       │
  ┌────┴────┐
  ▼         ▼
CPU Path    GPU Path (new)
write_rows  NexusGfx SDK (userspace/nexus-gfx)    ← Phase 1
vmo_write   Device  Queue  CommandBuffer
            RenderCommandEncoder  Fence
                │
                ▼
            GfxBackend Trait (userspace/gfx-backend) ← Phase 2
            submit(cmd) → Fence
                │
        ┌───────┴───────┐
        ▼               ▼
    CpuMockBackend   VirtioGpuBackend               ← Phase 3
    (host tests)     (source/drivers/gpud)
    golden ref       MMIO virtio-gpu
                         │
                    Real GPU (future)
                    same trait, no code changes
```

> **Current structure (Gate 1).** The `GfxBackend` trait lives in `userspace/nexus-gfx`
> (`backend::traits`), not a separate `gfx-backend` crate. `CpuMockBackend` (reference) and
> `VirtioGpuBackend`'s CPU/VMO path no longer hand-maintain separate rasterizers — both call the one
> canonical software rasterizer in `userspace/nexus-gfx/src/raster/` (RFC-0067). The windowd↔gpud wire
> is the `nexus-display-proto` SSOT (ADR-0038); the full device-class layering is ADR-0039.

## Layer 1: Animation Engine

Crate: `userspace/ui/animation/` — `src/` + `tests/`

| Module | Responsibility |
|--------|---------------|
| `timeline.rs` | `AnimationDriver::tick()`, active animation lifecycle, reduced motion |
| `spring.rs` | `SpringSim` with fixed-timestep RK4 integration |
| `keyframe.rs` | `KeyframeTrack` with Linear/EaseIn/EaseOut/EaseInOut interpolation |
| `property.rs` | `AnimProp`, `SceneUpdate`, `LayerId`, `Easing`, `SpringConfig` |

Key invariants:
- Fixed-timestep RK4: same dt → same position on x86_64 and riscv64
- Max 16 concurrent animations (Vec capacity)
- Reduced motion caps all durations at 100ms

## Layer 2: NexusGfx SDK

Crate: `userspace/nexus-gfx/` — `src/` + `tests/`

Metal-like API vocabulary. No 3D, no shader compilation (v1 minimal).

| Module | Responsibility |
|--------|---------------|
| `device.rs` | `Device` → `Queue`, `Buffer`, `CommandBuffer` factory |
| `queue.rs` | `Queue::submit(cmd)` → `Fence` |
| `command_buffer.rs` | `CommandBuffer` → `RenderCommandEncoder` → `commit()` → `CommittedBuffer` |
| `render_encoder.rs` | `set_fragment_bytes(offset, data)`, `draw_tiles(tiles)`, `end_encoding()` |
| `buffer.rs` | `Buffer::write(offset, data)`, `as_bytes()` |
| `fence.rs` | `Fence::wait(timeout)`, `signaled()` |

Internal `enum Command { SetFragmentBytes, DrawTiles }` — no heap alloc during recording.

## Layer 3: GPU Backend

### GfxBackend Trait (`userspace/gfx-backend/`)

```rust
pub trait GfxBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError>;
    fn create_resource(&mut self, w, h, fmt) -> Result<ResourceId, GfxError>;
    fn transfer_to_host(&mut self, res, rect) -> Result<(), GfxError>;
    fn set_scanout(&mut self, res) -> Result<(), GfxError>;
    fn move_cursor(&mut self, x, y) -> Result<(), GfxError>;
}
```

### CpuMockBackend

Executes `CommandBuffer` commands directly on a CPU `Vec<u8>` framebuffer.
Golden reference: output must match `write_rows()` for the same `SceneUpdate` stream.

### VirtioGpuBackend (`source/drivers/gpud/`)

Translates `GfxBackend` calls to virtio-gpu MMIO protocol:
- `create_resource` → `CREATE_RESOURCE_2D` + `ATTACH_BACKING`
- `transfer_to_host` → `TRANSFER_TO_HOST_2D`
- `set_scanout` → `SET_SCANOUT` (buffer flip, zero CPU copy)
- `move_cursor` → `MOVE_CURSOR` (hardware cursor, zero CPU per frame)

MMIO probe: scans virtio-mmio registers at `0x10008000 + N*0x200` for device ID 16.

## RISC-V Optimizations

| File | Change | Impact |
|------|--------|--------|
| `fixed_sdf.rs` | `circle_sd()` fixed-point (integer only) | 5–10× faster circle SDF |
| `primitives.rs` | `div255(x) = ((x*257)+32768)>>16` | 3–5× faster alpha blend |
| `.cargo/config.toml` | `target-feature=+zbb` | 10–20% bitmanip ops |

## Integration chain

```
input event → apply_input_state() → implicit transitions
    → AnimationDriver::spring_to()
    → tick() → AnimationDriver::tick(now_ns) → SceneUpdate
    → TileMap::mark_rect() → re-render affected tiles
    → vmo_write() → visible frame
```

Hop markers along the chain:
```
uiruntime: on → uianim: timeline on → windowd: implicit transitions on
→ windowd: live transition ok → SELFTEST: ui v5 transition ok
```

Selftest-client: observer-only — reads UART markers, verifies chain, never writes.

## File structure (no monoliths)

```
userspace/ui/animation/     src/ + tests/   12 tests
userspace/nexus-gfx/         src/ + tests/   4 tests
userspace/gfx-backend/       src/ + tests/   4 tests
source/drivers/gpud/         src/ + tests/   9 tests
source/services/windowd/src/compositor/      18 files + markers/
```
