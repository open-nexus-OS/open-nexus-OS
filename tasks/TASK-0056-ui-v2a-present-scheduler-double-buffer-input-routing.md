---
title: TASK-0056 UI v2a: double-buffered surfaces + present scheduler (vsync/fences/latency) + input routing (hit-test/focus)
status: Done
owner: @ui
created: 2025-12-23
depends-on:
  - TASK-0055
  - TASK-0055B
  - TASK-0055C
follow-up-tasks:
  - TASK-0056B
  - TASK-0056C
  - TASK-0199
  - TASK-0200
  - TASK-0253
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v1a renderer (baseline): tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - UI v1b windowd (baseline): tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md
  - RFC seed contract: docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md
  - Drivers/Accelerators contracts: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (vsync spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Production gates: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - Present/input perf follow-up: tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md
  - Config broker (ui knobs): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (permissions): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

UI v1 brought up a minimal compositor and surface protocol. UI v2 introduces the first “real-time UX” aspects:

- double buffering (no tearing),
- a present scheduler aligned to vsync,
- input routing with hit-testing and focus.

This task is deliberately **render-backend agnostic**:

- CPU backend is used for v2a proofs.
- A future GPU backend (Imagination/PowerVR or virtio-gpu) can plug into the same interfaces once the
  device-class service stack is available (see `TRACK-DRIVERS-ACCELERATORS`).

Scope note:

- A focused “Windowing/Compositor v2” integration slice (damage regions, input regions hit-testing, deterministic screencaps/thumbs, WM-lite + alt-tab) is tracked separately as `TASK-0199`/`TASK-0200` to avoid stretching v2a into WM/screencap territory.
- If early QEMU fluidity work needs tighter click-to-frame latency, event coalescing, and no-damage short-circuit rules,
  use `TASK-0056C` rather than expanding the v2a functional baseline indefinitely.

## Goal

Deliver:

1. **Double-buffered surfaces**:
   - client acquires a back buffer VMO, writes into it, then presents by frame index.
2. **Present scheduler** in `windowd`:
   - vsync-aligned present,
   - coalescing of rapid submits,
   - bounded fences and deterministic markers,
   - basic latency metrics (internal counters; log markers are enough for v2a).
3. **Input routing**:
   - hit-testing through the layer tree,
   - focus model (focus follows click),
   - pointer and keyboard delivery to surfaces.
4. Host tests for present scheduling + input routing, and OS markers for QEMU.

## Non-Goals

- Kernel changes.
- Text shaping and SVG (v2b).
- Real HW vsync; v2 uses a timer-driven vsync spine.
- Low-level input device drivers (HID/touch) or input event pipeline (handled by `TASK-0252`/`TASK-0253`; this task focuses on input routing within windowd).

## Constraints / invariants (hard requirements)

- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Bounded scheduler state:
  - cap queue depth per surface,
  - cap coalesced damage rect count.
- Premultiplied alpha rules must be consistent across present/composition.
- No parallel sync model: fences must be versioned and minimal (documented as v2a semantics).
- No sidecar compositor/input authority; v2a extends the existing `windowd` state machine.
- Marker honesty is mandatory: no `ok/ready` marker before corresponding present/input state transitions are real.

## Security / authority invariants

- `windowd` remains single authority for scene ownership, present sequencing, hit-test, and focus transitions.
- Input/focus routing rejects unauthorized or stale surface references deterministically.
- Queue/fence/input event handling stays bounded to avoid DoS-style unbounded growth.
- Logs/markers expose bounded metadata only (ids/counts/seq), never raw frame/input payload dumps.

## Red flags / decision points

- **YELLOW (fence semantics)**:
  - v2a “present fence” can be a minimal event signaled after composition tick.
  - Must be documented as minimal and not yet suitable for true low-latency pipelines.
- **YELLOW (CPU-only wording)**:
  - We do not lock ourselves into CPU-only. Interfaces must not assume CPU blits.
  - But v2a proofs remain CPU-based to keep the task QEMU-tolerant.
- **YELLOW (authority drift)**:
  - introducing a parallel present/input routing lane in launcher/SystemUI or selftest would invalidate v1/v1d carry-in assumptions.
- **YELLOW (fake-green marker risk)**:
  - marker ladders can go green while focus/hit-test/fence semantics are wrong unless host assertions check actual routing outcomes.
- **YELLOW (scope creep)**:
  - avoid absorbing visible cursor polish (`TASK-0056B`), perf tuning (`TASK-0056C`), or WM/compositor-v2 breadth (`TASK-0199`/`TASK-0200`) into the v2a baseline.

Red-flag mitigation now:

- Keep one `windowd` authority path for present scheduler and input routing.
- Gate success markers on post-state evidence (`present ack`, focused surface id, deterministic click path) plus host assertions.
- Treat cursor visuals, click-latency tuning, and WM-lite breadth as explicit follow-ups.
- Keep kernel untouched and consume existing carry-in floor from 55/55B/55C.

## Gate E quality mapping (TRACK alignment)

`TASK-0056` contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) in
`tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` by extending 55C from visible first-frame proof into
deterministic present scheduling and input-routing correctness.

- **first-frame/present/input path:** v2a owns deterministic scheduler + focus/hit-test baseline.
- **surface ownership/reuse:** must preserve 55/55C ownership boundaries; no sidecar authority.
- **perf closure:** intentionally not claimed here; measured optimization follows in `TASK-0056C`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v2a_host/`:

- present scheduler:
  - client produces N frames rapidly → scheduler coalesces and presents fewer times deterministically
  - fences are signaled after present
  - “no damage → no present”
- input routing:
  - two overlapping surfaces → pointer hit-test selects the topmost visible surface
  - focus transitions on click and keyboard delivery goes to focused surface

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: present scheduler on`
- `windowd: input on`
- `windowd: focus -> <surface_id>`
- `launcher: click ok`
- `SELFTEST: ui v2 present ok`
- `SELFTEST: ui v2 input ok`

### Quality gates (must be green for closure)

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run` (in order)

### Evidence so far (2026-04-30)

Implemented and proven:

- `windowd` owns the v2a double-buffer/scheduler/input authority path:
  - frame-indexed back-buffer acquisition,
  - bounded per-surface pending present state,
  - deterministic coalescing of rapid submits,
  - minimal fences signaled only after scheduler tick processing,
  - hit-test/focus/keyboard routing from committed layer ordering.
- `launcher` uses the v2a path only as a client/demo; `launcher: click ok` requires a routed click state.
- `selftest-client` emits v2a markers only after `windowd::run_ui_v2a_smoke()` returns real scheduler/input evidence; `SELFTEST: ui v2 input ok` additionally requires launcher click evidence.
- `visible-bootstrap` remains a marker/proof profile, not a desktop/start-profile or screenshot proof.

Green proofs:

- Closure rerun `cargo test -p windowd -p launcher -p ui_v2a_host -- --nocapture` — 22 tests across the three target packages.
- Closure rerun `cargo test -p ui_v2a_host reject -- --nocapture` — 5 reject-filtered tests.
- `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture`.
- `RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' NEXUS_DISPLAY_BOOTSTRAP=1 cargo check -p selftest-client --target riscv64imac-unknown-none-elf --release --no-default-features --features os-lite`.
- Closure rerun `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`.
- Header/doc closure sync was completed for touched Rust headers, `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`, `docs/architecture/README.md`, `docs/testing/index.md`, and `docs/dev/ui/foundations/quality/testing.md`.

Observed v2a QEMU marker ladder:

- `windowd: present scheduler on`
- `windowd: input on`
- `windowd: focus -> 1`
- `launcher: click ok`
- `SELFTEST: ui v2 present ok`
- `SELFTEST: ui v2 input ok`

Closure gates now green:

- `scripts/fmt-clippy-deny.sh`
- `just test-all`
- `just ci-network`
- `make clean`, `make build`, `make test`, `make run` (in order)

Known claim boundaries after review:

- v2a back buffers are modeled as validated VMO-shaped `SurfaceBuffer` submissions under `windowd`; this does not claim kernel VMO acquire/reuse/seal production semantics.
- OS/QEMU input proof is a deterministic `windowd` v2a smoke path in `selftest-client`; it does not claim low-level HID/touch pipeline closure.
- `visible-bootstrap` proves guest-side marker/proof-manifest evidence; it does not claim an independent GTK screenshot/display refresh proof.

## Touched paths (allowlist)

- `source/services/windowd/` + `source/services/windowd/idl/` (extend)
- `userspace/apps/launcher/` (update to double-buffer and input click demo)
- `tests/ui_v2a_host/` (new)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui-v2a.sh` (delegating)
- `docs/dev/ui/input/input.md` + `docs/dev/ui/foundations/rendering/renderer.md` (new/extend)
- `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`, `docs/architecture/README.md`, `docs/testing/index.md`, `docs/dev/ui/foundations/quality/testing.md` (closure documentation sync)
- `Cargo.toml` (workspace registration for `tests/ui_v2a_host`)
- `scripts/qemu-test.sh` (visible-bootstrap marker tail extended for v2a proof)

## Plan (small PRs)

1. **IDL updates**
   - Add `AcquireBackBuffer()` + `Present(frame_idx, damage, fence)`
   - Extend `input.capnp` with pointer/keyboard events

2. **Present scheduler**
   - per-surface double buffer state
   - vsync tick alignment
   - damage coalescing rules documented
   - markers:
     - `windowd: present scheduler on`
     - `windowd: present (seq=... frames=coalesced|single latency_ms=...)`

3. **Input routing**
   - hit-test on layer tree
   - focus manager + delivery channels
   - markers:
     - `windowd: input on`
     - `windowd: focus -> ...`
     - `windowd: pointer hit ...`

4. **Launcher update**
   - double-buffer API
   - click toggles a highlight rectangle → `launcher: click ok`

5. **Proof + docs**
   - host tests + OS markers + docs
