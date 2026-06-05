# RFC-0059: Deterministic Animation Runtime + NexusGfx 2D Pipeline + GPU Driver Contract

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-05-22
- Last Updated: 2026-06-05 (Phase 6c-7 closure criteria hardened: deterministic pacing + measurable smoothness gates)
- Links:
  - Tasks: `tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md` (execution + proof)
  - Depends on: `docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md`
  - Gfx architecture: `docs/architecture/nexusgfx-command-and-pass-model.md`, `docs/architecture/nexusgfx-resource-model.md`, `docs/architecture/nexusgfx-tile-aware-design.md`
  - Gfx track: `tasks/TRACK-NEXUSGFX-SDK.md`, Driver track: `tasks/TRACK-DRIVERS-ACCELERATORS.md`
  - Performance target matrix: `docs/dev/perf/PLATFORM-CLASS-UI-PERFORMANCE-OPTIMIZATIONS-QEMU-MATRIX.md`

## Status at a Glance

- Phase 0 (Animation Engine): ✅
- Phase 1 (NexusGfx SDK — types, CommandBuffer, Queue, Fence): ✅
- Phase 2 (GfxBackend trait + CpuMockBackend): ✅
- Phase 3 (gpud + virtio-gpu MMIO): ✅
- Phase 4 (windowd integration + animation proof): ✅
- Phase 5 (NexusGfx module structure): ✅
- Phase 6a (CommandBuffer serialization + IPC wire format): ✅
- Phase 6b (Reactive gpud IPC loop — Wait::Blocking): ✅
- Phase 6c (GPU-first rendering pipeline — single CB per frame): 🟡 in progress
- Phase 6d (Async Fence + double-buffer pipelining): ⬜
- Phase 6e (RISC-V fixed-point rendering in backend): ⬜
- Phase 7 (Golden tests + perf regression gates): ⬜

## Scope boundaries

- **This RFC owns**: Animation engine, NexusGfx 2D pipeline, GPU backend trait, gpud service, RISC-V optimizations, frame-budget discipline, reduced-motion, single-IPC frame submission
- **This RFC does NOT own**: Full 3D pipeline, real GPU shader backends, kernel MMIO policy, declarative scene graph (TASK-0073), DSL (TASK-0075), WM/scene transitions (TASK-0064)

## Context

TASK-0059 delivered a CPU compositor. TASK-0062 adds animation + a GPU command path. The initial Phase 4 integration kept the CPU compositor as the primary rendering path, with the GPU path as a parallel metadata submission that did not produce pixels. Phase 6a-6c replaces the dual-path architecture with a single GPU-first pipeline: windowd builds one CommandBuffer per frame, sends it via one IPC to gpud, and gpud renders the complete frame.

### Design targets

| Property | Target |
|---|---|
| Animation API | Declarative (Spring RK4, deterministic) |
| Physics | Fixed-timestep RK4, explicit dt |
| Frame budget | 8.3 ms @ 120 Hz (CPU backend with bounded tiles) |
| Quality degradation | Explicit (GlassQuality → Low → Opaque) |
| Reduced motion | System flag, first-class |
| Rendering path | GPU-first: single CommandBuffer per frame → gpud |
| CPU reference | CpuMockBackend golden output == VirtioGpuBackend output |
| Determinism | Guaranteed (fixed-timestep, explicit dt, stable rasterization) |
| Heap during animation | Zero (all buffers pre-allocated) |
| IPC per frame | One (CommittedBuffer, not per damage-rect) |

## Architecture (Phase 6c target)

```text
┌──────────────────────────────────────────────────────────────┐
│ windowd (Producer — window management + input)                │
│                                                              │
│  recv(Wait::Blocking) → apply_input_state()                  │
│  tick(now_ns) → animation_driver.tick() → SceneUpdates       │
│  build_frame_commands() → CommandBuffer                      │
│    ├─ BlitSurface (wallpaper)                                │
│    ├─ FillSdfRoundedRect (panels, sidebar)                   │
│    ├─ BlurBackdrop (glass effects)                           │
│    ├─ DrawText (labels)                                      │
│    └─ BlendCursor (pointer)                                  │
│  commit() → CommittedBuffer                                  │
│  IPC[single CB] → gpud                                       │
│  NO vmo_write(), NO CPU rendering in windowd                 │
└──────────────────────┬───────────────────────────────────────┘
                       │ cap-gated IPC (single message per frame)
                       ▼
┌──────────────────────────────────────────────────────────────┐
│ gpud (Consumer — rendering + scanout)                        │
│                                                              │
│  recv(Wait::Blocking) → CommittedBuffer::deserialize()       │
│  backend.submit(cb) → Fence                                  │
│    ├─ CpuMockBackend (host): SW rasterizer with SIMD         │
│    └─ VirtioGpuBackend (OS): CPU rendering in gpud space     │
│  Writes directly into VMO (ATTACH_BACKING zero-copy)         │
│  TRANSFER_TO_HOST_2D + RESOURCE_FLUSH (once per frame)       │
│  fence.signal() → windowd can build next frame               │
└──────────────────────────────────────────────────────────────┘
```

### Key architectural properties

1. **Single owner, single path.** windowd produces commands; gpud consumes and renders. No dual CPU/GPU path.
2. **Zero-copy VMO.** windowd creates the framebuffer VMO and hands it to gpud via cap transfer. gpud maps it with ATTACH_BACKING. All rendering happens directly in GPU-visible memory.
3. **Reactive IPC.** gpud blocks on recv. windowd blocks on recv (with future timer for animation ticks). No polling, no busy-wait.
4. **Async pipeline.** Fence separates submission from completion. windowd builds frame N+1 while gpud renders frame N.
5. **CPU backend golden.** CpuMockBackend produces bit-identical output to VirtioGpuBackend for the same CommandBuffer input.

## Phases

### Phase 0 — Animation Engine (`userspace/ui/animation`) ✅
Timeline, Spring RK4, Keyframe, Easing, SceneUpdate, ReducedMotion.

### Phase 1 — NexusGfx SDK Core (`userspace/nexus-gfx`) ✅
Device, Queue, CommandBuffer, RenderCommandEncoder, CommittedBuffer, Fence, GfxError, TileRect, RenderPassDesc.

### Phase 2 — GfxBackend Trait + CpuMockBackend ✅
GfxBackend trait (submit, create_resource, transfer_to_host, set_scanout, move_cursor). CpuMockBackend as deterministic CPU reference.

### Phase 3 — gpud Service + virtio-gpu MMIO ✅
MMIO probe, virtqueue setup, resource creation, ATTACH_BACKING, SET_SCANOUT, TRANSFER_TO_HOST_2D, RESOURCE_FLUSH, hardware cursor.

### Phase 4 — windowd Integration + Animation Proof ✅
AnimationDriver in DisplayServerRuntime, implicit transitions on paint flag changes, sidebar spring animation, QEMU proof markers.

### Phase 5 — NexusGfx Module Structure ✅
10-module tree under `userspace/nexus-gfx/src/`: core, resource, command, pipeline, shader, backend, sync, transfer, perf, cache.

### Phase 6a — CommandBuffer Wire Format ✅
Binary serialization/deserialization for CommittedBuffer. Compact LE encoding: command_count (u16) + per-command tag + payload. Round-trip tests.

### Phase 6b — Reactive gpud IPC ✅
gpud: `Wait::NonBlocking` → `Wait::Blocking`. No polling, no busy-wait. Kernel wakes gpud on message arrival.

### Phase 6c — GPU-first Rendering Pipeline (this phase)
- New Command types: `BlitSurface`, `FillSdfRoundedRect`, `BlurBackdrop`, `DrawText` (bitmap), `BlendCursor`
- CpuMockBackend implements all commands as a real software rasterizer
- windowd `flush_pending_damage()` replaced by `build_frame_commands()` + single IPC
- gpud receives full CommittedBuffer, renders, presents once per frame
- No per-damage-rect IPC, no `vmo_write()` from windowd, no CPU compositing in windowd
- Binary format extended with new command tags (2-6)

### Phase 6d — Async Fence + Double-Buffer Pipelining
- `Fence` with `wait()` and `signal()` — no longer always-signaled
- Two framebuffer VMOs (ping-pong): windowd builds into one while gpud renders the other
- Cap-based ownership transfer: windowd sends VMO cap with CommandBuffer, gpud returns it on fence signal

### Phase 6e — RISC-V Fixed-Point Rendering
- Port `fixed_sdf.rs` fixed-point algorithms into CpuMockBackend/VirtioGpuBackend
- `(a*b*257)>>16` blend operations
- `+zbb` target-feature for bit-manipulation acceleration
- Loop unrolling for `blur_backdrop` horizontal pass

### Phase 7 — Golden Tests + Perf Regression Gates
- Pixel-golden comparison: CpuMockBackend output == VirtioGpuBackend output
- Frame-time histogram (p50, p95, p99) from host perf traces
- Blur cost as function of radius × area
- IPC latency per frame (must be constant, independent of damage-rect count)
- Zero heap growth across consecutive frames

## Normative Phase Closure (6c -> 7)

This section is normative. A phase is complete only when all "required proofs" pass and no "fail condition" is true.

### Cross-track dependency (mandatory for Phase 7)

Phase 7 closure depends on the kernel timer capability package defined in
`docs/dev/perf/KERNEL-TIMER-CAPABILITY-ANALYSIS.md` (Phase 2, estimated 6-8 engineer-days).
This dependency is not optional because pacing closure requires timer + present completion.

### Phase 6c closure

#### Required implementation state
- `VirtioGpuBackend::submit()` executes command semantics (no no-op submit path).
- `windowd` frame path sends one committed command stream per frame.
- Per-frame `vmo_write()` CPU compositing path is removed from the primary render path.

#### Required proofs
- Host tests prove command execution semantics for all phase-6c command tags.
- QEMU proof shows one-command-stream submit path (`windowd` submit marker + `gpud` render marker).
- End-to-end frame result is visible (scanout marker ladder green).

#### Fail conditions (phase remains open)
- `submit()` validates but does not execute render commands.
- Frame path still depends on CPU row-compositing as primary renderer.
- Command stream is split into per-damage IPCs in steady-state rendering.

### Phase 6d closure

#### Required implementation state
- Fence lifecycle is asynchronous (`submitted -> pending -> signaled`) and not permanently pre-signaled.
- At least double-buffer pipeline with bounded in-flight frames (target: max 2).
- Completion correlation exists (`present_id`/sequence equivalent).

#### Required proofs
- Host tests assert fence wait/signal ordering and timeout behavior.
- QEMU proof asserts no deadlock under delayed completion and preserves forward progress.
- Backpressure behavior is deterministic (bounded queue/in-flight counters).

#### Fail conditions
- Fence is always signaled immediately regardless of render progress.
- Unlimited in-flight growth or unbounded retries under load.
- Completion events cannot be correlated to submitted frames.

### Phase 6e closure

#### Required implementation state
- Fixed-point math path is active in backend hot loops (blend/SDF/blur critical sections).
- Optimization path is architecture-aware and deterministic.
- Quality degradation policy is explicit and bounded for overload cases.

#### Required proofs
- Host parity tests: fixed-point output remains within bounded tolerance vs reference path.
- Microbench confirms improvement in targeted hot loops.
- No new unbounded allocation in frame hot path.

#### Fail conditions
- Optimization only exists in docs/comments without active execution path.
- Determinism regresses between runs with same input.
- Performance gains rely on nondeterministic shortcuts.

### Phase 7 closure

#### Required implementation state
- Golden image suite covers blur, shadow, rounded geometry, text, cursor composition.
- Regression gates enforce p50/p95/p99 budgets per test profile.
- Memory stability gate enforces no unbounded heap growth across long runs.
- Kernel timer capability is integrated as the primary frame-tick source for paced rendering.
- Present completion feedback path is integrated and correlated (`present_id`/sequence equivalent).
- Final frame pacing uses timer + present completion closure (not timer-only and not completion-blind).
- Kernel timer package dependency (6-8d scope) is complete and integrated.

#### Required proofs
- Golden tests pass on host pipeline and OS pipeline comparator path.
- QEMU profiles report stable marker ladders and pass defined pacing budgets.
- Performance artifacts are archived and comparable across runs.

#### Fail conditions
- "Looks smoother" without passing budget gates.
- Marker-only success without image/metric assertions.
- Improvements that pass only in one ad-hoc environment and fail standard profiles.
- Phase-7 closure claimed without kernel timer capability in the active pacing path.
- Phase-7 closure claimed without present completion correlation in the active pacing path.
- Phase-7 closure claimed while kernel timer dependency package is still open.

## Command vocabulary (Phase 6c)

| Tag | Command | Payload | Purpose |
|-----|---------|---------|---------|
| 0 | `SetFragmentBytes` | offset(u16) + len(u16) + data | Shader uniform parameters |
| 1 | `DrawTiles` | count(u16) + [x(u32)+y(u32)+w(u32)+h(u32)]* | Solid-color tile fill |
| 2 | `BlitSurface` | src_x(u32)+src_y(u32)+dst_x(u32)+dst_y(u32)+w(u32)+h(u32) | Copy from source surface |
| 3 | `FillSdfRoundedRect` | x(u32)+y(u32)+w(u32)+h(u32)+radius(u32)+rgba(u32) | SDF rounded rectangle fill |
| 4 | `BlurBackdrop` | x(u32)+y(u32)+w(u32)+h(u32)+radius(u32)+sat(u32) | Box blur + saturation |
| 5 | `DrawText` | x(u32)+y(u32)+scale(u32)+rgba(u32)+len(u16)+utf8 | Bitmap text rendering |
| 6 | `BlendCursor` | x(u32)+y(u32)+w(u32)+h(u32) | Alpha-blend cursor bitmap |

All multi-byte fields are little-endian. All sizes validated before execution.

## Zero-copy IPC design

The framebuffer VMO follows the cap transfer protocol:

```text
1. windowd: vmo_create(1280*800*4) → fb_handle
2. windowd: cap_clone(fb_handle) → clone                   // create sendable copy
3. windowd: client.send_with_cap_move_wait(&[opcode], clone, Wait::Blocking)
4. kernel:   transfers cap ownership windowd → gpud          // zero-copy: same physical pages
5. gpud:     recv → moved_cap.take() → cap_query(slot) → phys_addr
6. gpud:     ATTACH_BACKING(resource_id, phys_addr, len)    // GPU can now access VMO
7. gpud:     SET_SCANOUT(resource_id, width, height)
8. gpud:     renders directly into VMO pages (CPU or GPU backend)
9. gpud:     TRANSFER_TO_HOST_2D + RESOURCE_FLUSH (once per frame)
```

On real GPU hardware (future): step 9 is replaced by a cache flush or is unnecessary if the GPU has coherent access to system memory.

## Rust idioms and safety

- `TileRect`, `FenceId`, `ResourceId` are `Copy` newtypes over `u32`/`u64`
- `CommittedBuffer` is `Send` (no shared ownership, transfers between threads/processes)
- `Fence` uses `#[must_use]` — ignoring a fence is a logic error
- `GfxBackend::submit` takes ownership of `CommittedBuffer` (move semantics)
- All rendering functions return `Result<(), GfxError>` — no panics in backend code
- `CpuMockBackend` owns its framebuffer `Vec<u8>` — clear ownership, no shared mutable state

## RISC-V specific optimizations

| Technique | Where | Rationale |
|---|---|---|
| Fixed-point SDF | `FillSdfRoundedRect` in backend | Avoids float→int conversion in inner loops |
| `(a*b*257)>>16` | Alpha blending in backend | Replaces `/255` with shift, 3-5× faster |
| `+zbb` target-feature | `.cargo/config.toml` | Enables `andn`, `orn`, `maxu`, `minu` for bit ops |
| Loop unroll ×4 | `BlurBackdrop` horizontal pass | Reduces branch overhead in box-blur sliding window |
| Tile-local scratch | `CpuMockBackend` internal buffers | Avoids heap allocation per tile; reuses pre-allocated scratch |

## Failure model

- `DeviceNotFound` → `gpud: no device`, clean exit
- `CommandRejected` → discard, log, no partial execution
- `ResourceExhausted` → bounded retry; fail → `Unsupported` → marker emitted
- `InvalidArgument` → malformed command, reject whole buffer, log diagnostic
- `MmioFault` → `gpud: mmio fault`, exit, restartable
- No silent fallback. CPU backend is the golden reference, not a degradation path.

## Proof strategy

### Host
```bash
cargo test -p nexus-gfx -p gpud -p windowd -p animation
```

### Host golden tests (Phase 7)
```bash
cargo test -p nexus-gfx --test golden_tests
```
Verifies: CpuMockBackend output matches reference frames for known CommandBuffer inputs.

### QEMU
```bash
RUN_UNTIL_MARKER=1 just test-os
```

### Markers
`gpud: ready` `gpud: cursor on` `gpud: scanout ok`
`uiruntime: on` `uianim: timeline on` `uianim: spring converge ok`
`windowd: implicit transitions on` `windowd: live transition ok`
`windowd: cb submit ok` `gpud: cb render ok`
`SELFTEST: ui v5 transition ok` `SELFTEST: gpu cursor move ok`

### Perf gates (Phase 7)
- Host: frame compositing < 8.3 ms (p95)
- Host: IPC serialization + deserialization < 100 µs
- Host: zero heap growth across 1000 consecutive frames
- QEMU: no missing markers, no timeout

### Profile-class performance targets (Phase 7)

These targets avoid vendor naming and define platform-class smoothness in measurable terms.

| Profile class | Target |
|---|---|
| Lightweight interaction | p95 frame interval <= 16.7 ms, p99 <= 22 ms |
| Blur/glass medium load | p95 <= 20 ms, no sustained sawtooth pacing |
| Heavy transition burst | bounded degrade allowed, but no multi-second stalls |
| Input latency under load | no persistent input starvation while animation is active |

Passing requires metric evidence, not marker-only evidence.

## Open questions

- Timer profile tuning: final interval/deadline policy and fallback thresholds per display profile
- Double-buffer cap handoff: does kernel support returning a cap to sender? → investigate
- Reduced-motion config propagation: `set_reduced_motion(bool)` in Phase 0 is sufficient
- Shader binary format: SPIR-V or custom IR? → defer to CAND-GFX-002

## Implementation Checklist

- [x] Phase 0: Animation Engine — `cargo test -p animation`
- [x] Phase 1: NexusGfx SDK Core — `cargo test -p nexus-gfx`
- [x] Phase 2: GfxBackend + CpuMockBackend
- [x] Phase 3: gpud + virtio-gpu — QEMU markers
- [x] Phase 4: windowd integration — animation proof markers
- [x] Phase 5: NexusGfx module structure — 10-module tree
- [x] Phase 6a: CommandBuffer wire format — serialize/deserialize + round-trip tests
- [x] Phase 6b: Reactive gpud IPC — Wait::Blocking
- [ ] Phase 6c: GPU-first rendering pipeline — new commands, backend rasterizer, single-IPC frame
- [ ] Phase 6d: Async Fence + double-buffer pipelining
- [ ] Phase 6e: RISC-V fixed-point rendering in backend
- [ ] Phase 7: Golden tests + perf regression gates + timer/present pacing closure
