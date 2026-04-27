# RFC-0046: UI v1a host CPU renderer + deterministic snapshots contract seed

- Status: Done
- Owners: @ui @runtime
- Created: 2026-04-27
- Last Updated: 2026-04-27
- Links:
  - Tasks: `tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md` (execution + proof SSOT)
  - Related tasks: `tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md`, `tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md`, `tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md`, `tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md`, `tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md`
  - Related RFCs: `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`, `docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`, `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
  - Architecture: `docs/architecture/nexusgfx-compute-and-executor-model.md`, `docs/architecture/nexusgfx-text-pipeline.md`
  - Production gate policy: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate E: Windowing, UI & Graphics, production-floor; kernel production-grade follow-ups stay explicit)

## Status at a Glance

- **Phase 0 (bounded renderer core contract)**: [x]
- **Phase 1 (deterministic host snapshots + goldens)**: [x]
- **Phase 2 (production-grade local hardening + escalation checks)**: [x]

Definition:

- "Done" means this RFC's host renderer contract is implemented by `TASK-0054` with deterministic host proofs green and no OS/QEMU/kernel marker claims.
- This RFC may require production-grade local hardening for bounds, ownership, input rejection, and proof quality. It does not claim Gate A kernel/core production-grade closure.
- Completion evidence (2026-04-27):
  - `cargo test -p ui_renderer -- --nocapture`
  - `cargo test -p ui_host_snap -- --nocapture` — 24 tests
  - `cargo test -p ui_host_snap reject -- --nocapture` — 14 reject-filtered tests
  - `just diag-host`
  - no OS/QEMU marker proof was run or claimed.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the host-first BGRA8888 CPU framebuffer contract for TASK-0054,
  - deterministic primitive rendering and damage tracking semantics,
  - host snapshot/golden comparison rules,
  - local Rust type-safety, ownership, `Send`/`Sync`, `#[must_use]`, and safe-Rust requirements for the renderer slice,
  - production-grade local rejection behavior for malformed or oversized renderer inputs.
- **This RFC does NOT own**:
  - kernel scheduling, IPC, MM, VMO rights/sealing, or zero-copy production closure,
  - OS `windowd`, compositor, input routing, vsync, present markers, or surface IPC,
  - GPU, `wgpu`, display-driver, MMIO, IRQ, or device-service paths,
  - the long-term Scene-IR + Backend trait contract if `TASK-0169` is selected as the implementation vehicle.

### Relationship to tasks (single execution truth)

- `TASK-0054` is the execution SSOT for this RFC.
- Task stop conditions and proof commands are authoritative for closure.
- If `TASK-0169` becomes the implementation vehicle, `TASK-0054` must be marked as implemented-by that work rather than creating a second renderer architecture.
- If implementation discovers that scheduler, memory management, IPC, VMO, or kernel ownership semantics are too weak or too simplistic for the renderer contract, stop and report the gap. Do not paper over it in TASK-0054. Route the gap to `TASK-0054B`, `TASK-0054C`, `TASK-0054D`, `TASK-0288`, `TASK-0290`, or a new task/RFC as appropriate.

## Context

The UI stack needs a deterministic first renderer slice before real display drivers and compositor wiring exist. TASK-0054 deliberately starts with a host-only CPU renderer that draws into BGRA8888 buffers, tracks dirty rectangles, and proves output through headless snapshots.

That small scope is useful only if it is honest:

- host snapshot tests must validate the desired behavior, not implementation quirks,
- goldens must not be silently rewritten,
- text and image inputs must be deterministic across machines,
- renderer types must prevent width/height/stride/rect confusion,
- and host-only code must not emit OS success markers.

The production-gate boundary is also important. TASK-0054 contributes to Gate E (`Windowing, UI & Graphics`, production-floor). It can require strict local bounds and proof quality, but it cannot close Gate A kernel/core production-grade behavior. Kernel/UI performance, IPC fastpath, MM reuse, SMP/timer stress, and VMO sealing/reuse truth remain explicit follow-ups.

## Goals

- Define a small host CPU renderer contract with BGRA8888 pixels and 64-byte aligned rows.
- Define deterministic primitives: clear, rect, simple rounded-rect coverage, blit, and fixture-font text.
- Define bounded damage tracking with stable coalescing/overflow behavior.
- Define snapshot/golden behavior that is deterministic, opt-in for updates, and resistant to fixture path escape.
- Define local production-grade hardening for bounds, type safety, ownership, and negative tests.
- Define escalation rules for kernel/scheduler/MM/IPC complexity discovered during implementation.

## Non-Goals

- Kernel changes.
- Scheduler, SMP, timer, IPC, MM, VMO rights, or zero-copy production closure.
- A compositor, present scheduler, input routing, or `windowd` integration.
- GPU acceleration or `wgpu` integration.
- Host font discovery or locale-dependent text fallback.
- OS/QEMU success markers.
- A second long-term renderer architecture parallel to `TASK-0169`.

## Constraints / invariants (hard requirements)

- **Determinism**:
  - fixed BGRA8888 byte order,
  - fixed row stride calculation with 64-byte alignment,
  - fixed primitive rounding and clipping rules,
  - fixed text fixture and raster parameters,
  - fixed snapshot test discovery order,
  - no host font discovery, locale fallback, wall-clock randomness, or filesystem-order dependence.
- **No fake success**:
  - no `*: ready`, `SELFTEST: ... ok`, `present ok`, or similar OS marker may be emitted by host-only renderer code,
  - snapshot tests may report pass/fail only after actual pixel/damage assertions.
- **Bounded resources**:
  - maximum frame width/height/pixel count,
  - maximum source image dimensions,
  - maximum glyph count per draw,
  - maximum damage rect count with deterministic coalesce or reject,
  - explicit checked arithmetic for stride and buffer length.
- **Security floor**:
  - reject malformed dimensions, stride, rect, image, glyph, and fixture paths before use,
  - no path traversal or absolute-path writes for snapshots/goldens,
  - normal tests must never rewrite goldens unless `UPDATE_GOLDENS=1`.
- **Rust type discipline**:
  - use `#[repr(transparent)]` newtypes where raw integers are easy to confuse,
  - prefer owned or explicitly borrowed frame/image views over shared mutable state,
  - no `unwrap`/`expect` on renderer inputs, fixture paths, image data, or golden reads,
  - no blanket `allow(dead_code)`,
  - `#![forbid(unsafe_code)]` for the host renderer crate unless a later RFC explicitly permits a low-level backend exception.
- **Stubs policy**:
  - unsupported OS/compositor/GPU paths return explicit unsupported/stub status if referenced at all,
  - stubs are non-authoritative and cannot produce positive success markers.

## Proposed design

### Contract / interface (normative)

The v1a host renderer exposes a narrow pure-compute API. Names may change during implementation, but the semantics are normative.

#### Core newtypes

Use newtypes for renderer quantities that must not be mixed accidentally:

```rust
#[repr(transparent)]
pub struct Px(u32);

#[repr(transparent)]
pub struct StrideBytes(u32);

#[repr(transparent)]
pub struct SurfaceWidth(Px);

#[repr(transparent)]
pub struct SurfaceHeight(Px);

#[repr(transparent)]
pub struct DamageRectCount(u16);
```

The final implementation may choose different names, but raw `u32`/`usize` APIs must not be the primary public contract for width, height, stride, or damage limits.

Required type properties:

- cheap value types derive `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq` where meaningful,
- conversion from raw values is checked when values are caller-provided,
- unchecked constructors, if any, are crate-private and documented,
- public constructors reject invalid or oversized values with stable error variants.

#### Pixel and frame contract

- Pixel format is BGRA8888, one pixel = four bytes in memory order `B, G, R, A`.
- Row stride is 64-byte aligned and at least `width * 4`.
- Buffer length is exactly `stride * height` for owned frames.
- Rendering clips to the frame bounds and must never write outside the owned buffer.
- Alpha/blending semantics must be documented before a primitive can be considered covered by goldens.

Frame ownership:

- `Frame` owns its backing buffer.
- `FrameViewMut`-style mutable views, if introduced, require exclusive mutable borrowing.
- Shared read-only snapshot views may be `Sync` if the backing type is immutable.
- No global mutable renderer state is permitted in v1a.

`Send`/`Sync` rule:

- Prefer automatic `Send`/`Sync` from owned safe Rust types.
- Do not write `unsafe impl Send` or `unsafe impl Sync`.
- If a later low-level backend needs unsafe synchronization, that is outside this RFC and requires a new contract or ADR.

#### Primitive contract

Required primitives:

- `clear(color)`: fills the full logical frame and reports full-frame damage.
- `rect(rect, paint)`: clips to frame, writes only intersecting pixels, and reports clipped damage.
- `round_rect(rect, radius, paint)`: uses deterministic simple coverage rules; anti-aliasing is off unless explicitly specified and proven.
- `blit(dst, source)`: source dimensions and stride are checked before allocation/use; metadata does not affect pixel comparison.
- `text(position, text, style)`: uses only the repo-owned deterministic fixture font path for v1a.

Each primitive must have positive pixel tests and at least one boundary/reject test where applicable.

#### Damage contract

`Damage` accumulates dirty rectangles with a bounded maximum. Behavior must be deterministic:

- invalid rectangles reject,
- zero-area rectangles are either ignored with documented behavior or rejected with a stable error,
- overlapping or adjacent rectangles coalesce according to a fixed rule,
- if the rect limit is exceeded, the implementation either:
  - coalesces to a full-frame damage rect, or
  - rejects with `DamageOverflow`.

The chosen behavior must be documented and tested as desired behavior.

#### Error model and `#[must_use]`

All fallible operations return explicit result types. Error enums and validation outcomes that represent rejected input must be `#[must_use]`.

Required stable error classes include:

- `InvalidDimensions`,
- `InvalidStride`,
- `FrameTooLarge`,
- `ImageTooLarge`,
- `ArithmeticOverflow`,
- `RectOutOfRange` or `InvalidRect`,
- `DamageOverflow`,
- `FixturePathRejected`,
- `GoldenMismatch`,
- `GoldenUpdateDisabled`,
- `Unsupported`.

Exact enum names may differ, but tests must assert stable classes rather than matching fragile display strings.

#### Snapshot/golden contract

- Goldens live under `tests/ui_host_snap/goldens/`.
- Output artifacts, if written, stay under the test output directory.
- Snapshot tests compare deterministic decoded pixels or deterministic raw buffers.
- PNG metadata, gamma, and iCCP chunks must not affect comparison results.
- Golden updates require explicit `UPDATE_GOLDENS=1`; normal tests must fail with `GoldenMismatch` and never rewrite tracked goldens.
- Snapshot case enumeration is sorted and deterministic.

### Phases / milestones (contract-level)

- **Phase 0**: Renderer core contract: BGRA8888 `Frame`, checked dimensions/stride, primitives, bounded `Damage`, safe Rust, no fake markers.
- **Phase 1**: Host snapshot contract: deterministic scenes, repo-owned fixture font, PNG/raw comparison, opt-in golden update path, path traversal rejects.
- **Phase 2**: Local hardening and escalation: `test_reject_*` coverage for bounds and fixture abuse, Rust ownership/newtype/`#[must_use]` review, explicit report if kernel/scheduler/MM/IPC simplicity blocks a real UI floor.

## Security considerations

This RFC is security-relevant because renderer inputs can become a denial-of-service or memory-corruption boundary once the same code is reused by OS services. TASK-0054 is host-only, but the contract must be strict enough not to train future OS code into unsafe habits.

- **Threat model**:
  - oversized frames/images/glyph runs causing memory or CPU exhaustion,
  - malformed strides or rectangles causing out-of-bounds writes,
  - snapshot path traversal or absolute-path writes escaping the test fixture root,
  - golden update abuse masking regressions,
  - accidental authority drift into device/MMIO/GPU/present code,
  - fake markers claiming OS behavior from host-only code.
- **Mitigations**:
  - checked newtypes and validated constructors,
  - checked arithmetic for stride and buffer lengths,
  - `#![forbid(unsafe_code)]` for the host renderer crate,
  - bounded damage and glyph/image/frame limits,
  - explicit fixture-root validation,
  - opt-in golden updates,
  - no OS marker emission in this scope.
- **DON'T DO**:
  - no direct MMIO, GPU, display, IRQ, or kernel calls,
  - no host font discovery,
  - no unbounded allocation based on caller-provided sizes,
  - no `unsafe impl Send/Sync`,
  - no `unwrap`/`expect` on renderer inputs or fixture data,
  - no "ok/ready/present ok" marker before real OS behavior exists.
- **Open risks**:
  - font coverage may be too small for future UI; TASK-0054 may use a deterministic fixture font and defer full font fallback.
  - PNG dependency behavior may vary if metadata is compared directly; implementation must compare decoded/canonical pixels.
  - kernel/UI smoothness cannot be inferred from host goldens; TASK-0054B/C/D and TASK-0288/0290 own those claims.

## Failure model (normative)

- Invalid dimensions, stride, or buffer length reject before allocation or write.
- Arithmetic overflow rejects with a stable error class.
- Oversized frame/image/glyph inputs reject before allocation.
- Out-of-bounds draw requests clip when the operation is valid and reject when the input itself is invalid; the distinction must be documented.
- Damage overflow follows one documented behavior: deterministic full-frame coalesce or stable reject.
- Missing or mismatched goldens fail the test unless `UPDATE_GOLDENS=1` is set.
- Attempts to write goldens outside the fixture root reject.
- Unsupported OS/GPU/present paths return `Unsupported` or are absent; they never succeed silently.
- No silent fallback to a different font, pixel format, stride rule, or comparison tolerance.

## Proof / validation strategy (required)

Tests must validate desired behavior. Do not assert only that the current implementation produced "some bytes" or that a helper returned success.

### Proof (Host)

Canonical proof command:

```bash
cd /home/jenning/open-nexus-OS && cargo test -p ui_host_snap -- --nocapture
```

Required behavior tests:

- clear fills every logical pixel with the expected BGRA bytes,
- rect clips and damages only the expected region,
- rounded-rect produces documented deterministic coverage,
- blit copies expected pixels from an in-memory image and rejects invalid source dimensions,
- fixture-font text renders "hello world" deterministically,
- stride is 64-byte aligned and buffer length is exact,
- damage coalescing/overflow follows the documented rule.

Required reject tests:

- `test_reject_oversized_frame_before_allocation`,
- `test_reject_oversized_image_before_allocation`,
- `test_reject_invalid_stride`,
- `test_reject_arithmetic_overflow`,
- `test_reject_invalid_rect_or_damage_overflow`,
- `test_reject_golden_update_without_env`,
- `test_reject_fixture_path_traversal`,
- `test_reject_absolute_golden_write_path`.

Recommended local proof commands for implementation:

```bash
cd /home/jenning/open-nexus-OS && cargo test -p ui_host_snap reject -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p ui_renderer -- --nocapture
```

If the final crate/package names differ, TASK-0054 is responsible for updating the proof commands while preserving the same proof classes.

### Proof (OS/QEMU)

No OS/QEMU proof is required or allowed for TASK-0054 closure.

If an implementation needs OS/QEMU markers, this RFC is no longer sufficient; use `TASK-0170`, `TASK-0055`, or a new RFC/task. Host-only code must not emit deterministic OS success markers.

### Deterministic markers

None for TASK-0054.

Future OS marker strings such as `windowd: present ok` or `SELFTEST: renderer v1 present ok` belong to `TASK-0170` / `TASK-0055` and must follow real compositor/present behavior.

## Production gate mapping

Per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`, this RFC maps to Gate E (`Windowing, UI & Graphics`, production-floor).

Within TASK-0054 scope, the following are production-grade local requirements:

- bounded allocations and checked arithmetic,
- safe Rust with no unsafe code,
- newtypes for confusing raw quantities,
- explicit ownership and no global mutable renderer state,
- no unsafe `Send`/`Sync` implementations,
- `#[must_use]` on validation/error outcomes where ignored errors could hide a security bug,
- behavior-first positive and negative tests.

The following remain outside TASK-0054 and cannot be claimed by this RFC:

- Gate A kernel/core production-grade scheduler, IPC, MM, and VMO behavior,
- runtime SMP/timer/IPI stress closure,
- kernel-enforced VMO sealing/write-map denial/reuse truth,
- OS compositor/present/input smoothness.

Escalation requirement:

- If TASK-0054 reveals that a later real UI path would rely on simplistic scheduler, IPC, MM, VMO, or timer behavior, document that finding in the task and route it to the owning follow-up instead of weakening this RFC or pretending host snapshots prove it.

## Alternatives considered

- **Implement `TASK-0169` directly instead of TASK-0054**:
  - Preferred if the project wants the Scene-IR + Backend trait abstraction immediately.
  - Rejected for this RFC because TASK-0054 is scoped as the smaller host renderer proof floor. The task already permits `TASK-0169` to supersede it.
- **Use host system fonts**:
  - Rejected because font discovery, fallback, hinting, and locale can vary across machines.
- **Use OS/QEMU markers for confidence**:
  - Rejected because TASK-0054 has no real OS present/compositor path. Markers would be fake proof.
- **Permit a quick unsafe pixel loop for speed**:
  - Rejected for v1a. Safe Rust plus bounded dimensions is the correct first contract; performance tuning belongs after correctness proof.
- **Treat host renderer goldens as production-grade UI smoothness evidence**:
  - Rejected. Goldens prove deterministic pixels, not kernel scheduling, memory reuse, or present latency.

## Resolved questions

- TASK-0054 implemented a repo-owned deterministic fixture font under `userspace/ui/fonts/`; no host font discovery or
  locale fallback is used.
- Snapshot comparison asserts canonical BGRA pixels and writes deterministic PNG artifacts; PNG metadata/gamma/iCCP does
  not affect equality.
- Bounded damage overflow deterministically coalesces to full-frame damage.
- `TASK-0169` was not promoted; TASK-0054 stayed the narrow host proof floor that `TASK-0169` may later absorb.
- Closure review strengthened proof quality with full rounded-rect/text masks, blit clipping with padded source stride,
  exact buffer-length accept/reject coverage, oversized height rejects, malformed fixture-font rejects, safe golden update
  proof under an explicit artifact root, and an anti-fake-marker source scan.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Bounded BGRA8888 renderer core + newtypes + safe ownership — proof: `cargo test -p ui_renderer -- --nocapture`.
- [x] **Phase 1**: Deterministic host snapshots + goldens + fixture font + update gating — proof: `cargo test -p ui_host_snap -- --nocapture`.
- [x] **Phase 2**: Reject tests and local production-grade hardening review — proof: `cargo test -p ui_host_snap reject -- --nocapture` plus TASK-0054 documented review of newtypes, `#[must_use]`, `Send`/`Sync`, and no fake markers.
- [x] Task linked with stop conditions + proof commands.
- [x] QEMU markers intentionally absent for TASK-0054.
- [x] Security-relevant negative tests exist (`test_reject_*`).
