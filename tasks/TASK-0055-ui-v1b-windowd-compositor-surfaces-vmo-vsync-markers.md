---
title: TASK-0055 UI v1b (OS-gated): windowd compositor + surfaces/layers IPC + VMO buffers + vsync timer + markers
status: Done
owner: @ui
created: 2025-12-23
depends-on:
  - TASK-0054
  - TASK-0031
  - TASK-0013
  - TASK-0046
  - TASK-0047
follow-up-tasks:
  - TASK-0055B
  - TASK-0055C
  - TASK-0055D
  - TASK-0056
  - TASK-0056B
  - TASK-0056C
  - TASK-0169
  - TASK-0170
  - TASK-0170B
  - TASK-0250
  - TASK-0251
links:
  - RFC: docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Gfx resource model: docs/architecture/nexusgfx-resource-model.md
  - Gfx sync/lifetime model: docs/architecture/nexusgfx-sync-and-lifetime.md
  - Gfx command/pass model: docs/architecture/nexusgfx-command-and-pass-model.md
  - UI track dependencies: tasks/TRACK-DRIVERS-ACCELERATORS.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers (vsync spine): tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Config broker (ui profile): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Dev display/profile presets follow-up: tasks/TASK-0055D-ui-v1e-dev-display-profile-presets-qemu-hz.md
  - Policy as Code (permissions): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Logging/audit sink: tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - UI v1a renderer: tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - Deterministic parallelism policy (thread pools): tasks/TASK-0276-parallelism-v1-deterministic-threadpools-policy-contract.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want the first UI slice runnable in QEMU **without kernel display/input drivers**. That implies:

- “headless present” is acceptable: compose into a VMO-backed framebuffer and emit deterministic markers
  (and optionally export snapshots to `/state` once persistence exists).
- control plane uses typed IPC (Cap’n Proto) consistent with Vision.
- data plane uses VMO/filebuffer for shared buffers.

This task is OS-gated on VMO plumbing and a timing spine.

Current-state check (2026-04-27 prep sync):

- `RFC-0047` is `Done` (contract); this task is `Done` (execution and proof evidence closed and synced).
- `TASK-0054` / `RFC-0046` are `Done` and provide only the host BGRA8888 renderer/snapshot proof floor.
- `source/services/windowd/` now contains a modular bounded headless surface/layer/present state machine used by the host
  and OS marker proofs; `lib.rs` is a facade over focused modules instead of a monolith.
- `userspace/apps/launcher/` is now the canonical `launcher` package; the old `source/apps/launcher` placeholder was
  removed and the launcher recipe points at the userspace app.
- UI-present markers (`windowd: present ok`, `launcher: first frame ok`, `SELFTEST: ui launcher present ok`) are wired in
  `selftest-client` / `scripts/qemu-test.sh` and verified through the proof-manifest deny-by-default post-pass.
- The OS proof uses a small deterministic headless profile (`desktop`, `64x48`, `60Hz`) to stay within current selftest
  heap limits. This is not a visible scanout or consumer display preset claim.

Scope note:

- Renderer Abstraction v1 (`TASK-0169`/`TASK-0170`) defines the Scene-IR + Backend trait and the deterministic cpu2d default.
  `windowd` composition should call into that backend rather than inventing separate rendering primitives.
- Until `TASK-0169` / `TASK-0170` lands, `TASK-0055` may consume the narrow `ui_renderer` CPU proof floor from
  `TASK-0054`, but it must not turn that crate into a competing long-term renderer architecture.

## Goal

Deliver:

1. Surface/layer IPC contracts (Cap’n Proto) for:
   - creating surfaces with VMO buffers,
   - queueing buffers with damage,
   - an atomic scene commit,
   - vsync subscription (events),
   - input stubs (no routing yet).
2. `windowd` compositor:
   - manages a layer tree,
   - composites on a vsync tick (default 60Hz),
   - is damage-aware (skip present if nothing changed),
   - signals a minimal “present fence” (v1 semantics).
3. SystemUI host concept:
   - minimal “desktop/mobile” plugins may start as in-process modules (v1),
   - later extracted to separate processes once plugin ABI is ready.
4. OS selftest markers + postflight (delegating to canonical harness).

## Non-Goals

- Kernel changes.
- Real display output or virtio-gpu integration.
- Real input routing and focus (stubs only in v1).
- A full plugin ABI system (v1 can keep it simple).
- Simplefb framebuffer output (handled by `TASK-0250`/`TASK-0251` as an extension; this task focuses on headless present with VMO buffers).

## Constraints / invariants (hard requirements)

- Must not invent a parallel buffer/sync model:
  - use VMO handles for buffers (TRACK contract),
  - vsync driven by timed service / monotonic timer (QoS/timers contract),
  - fences must be bounded and auditable.
- Bounded composition:
  - cap number of surfaces,
  - cap layer depth,
  - cap pixel dimensions and total bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Deterministic markers in QEMU.
- Parallelism (optional):
  - `windowd` may later parallelize composition/raster work (e.g. tiles), but must follow `TASK-0276`
    (fixed workers, deterministic partitioning, canonical merge order, bounded queues, proof parity workers=1 vs N).

## Security / authority invariants

- `windowd` is the authority for surface IDs, layer membership, scene commits, and present sequencing in this slice.
- IPC callers must be identified from kernel/service metadata, never from client-provided strings.
- Surface creation and buffer queueing must fail closed for:
  - missing or forged VMO handles,
  - wrong rights or non-surface buffers,
  - dimensions/stride/format mismatches,
  - oversized surfaces/layer trees/total bytes,
  - stale surface IDs, stale commit sequence numbers, and unauthorized layer mutation.
- VMO-backed buffers must not be logged or copied into unbounded diagnostics; logs/markers may include bounded metadata
  such as IDs, dimensions, sequence numbers, and damage counts only.
- `windowd: ready`, `windowd: present ok`, `launcher: first frame ok`, and `SELFTEST: ui ... ok` markers are allowed only
  after the corresponding behavior has happened and been checked by the harness.
- Add `test_reject_*` coverage for invalid dimensions, invalid VMO rights/handles, unauthorized surface/layer access,
  stale queue/commit operations, and marker/postflight failure cases.

## Anti-drift posture for future `NexusGfx` wiring

- `windowd` is the first bounded present/composition spine, but it must **not** become a separate graphics contract that
  later competes with `TRACK-NEXUSGFX-SDK`.
- Surface buffers, queue/submit flow, damage, and minimal present fences should stay compatible with the resource/sync
  posture documented in the `nexusgfx-*` architecture docs above.
- If a GPU backend arrives later, `windowd` should swap executors/backends behind the same surface/composition contract
  instead of redefining IPC or buffer ownership.

## Red flags / decision points

- **VMO availability (partly de-risked, still must be re-proven for UI)**:
  - `TASK-0031` gives the baseline VMO plumbing, but `TASK-0055` must still prove the UI-shaped handle/rights/stride
    contract at the `windowd` boundary.
  - A copy fallback is allowed only if it is explicitly named `non-zero-copy fallback`, tested, and excluded from
    zero-copy/perf claims.
- **Present fence semantics (bounded v1 contract)**:
  - v1 may implement a minimal present acknowledgement/event after the composition tick.
  - Do not call it a latency-accurate GPU/display fence; real latency-sensitive semantics remain follow-up scope.
- **Existing `windowd` placeholder (must not become fake proof)**:
  - Current checksum/helper behavior is useful only as scaffold. It cannot emit success markers or satisfy the surface,
    VMO, present, or launcher proof requirements.
- **Visible output boundary (deferred)**:
  - `TASK-0055` proves headless present. Visible QEMU scanout and visible `windowd`/SystemUI frames belong to
    `TASK-0055B` / `TASK-0055C` and must not be claimed here.
- **Config/policy coupling (bounded minimum only)**:
  - `ui.profile` / display dimensions may be introduced here only as the minimum needed for deterministic headless
    present. Rich dev presets remain `TASK-0055D`.

## Gate E mapping

`TASK-0055` contributes to Gate E (`Windowing, UI & Graphics`, `production-floor`) by proving the first real
`windowd` surface/composition/present control path.

- Gate E "first-frame/present" expectation:
  - this task owns headless first-present markers and postflight validation,
  - visible first-frame proof is deferred to `TASK-0055B` / `TASK-0055C`.
- Gate E "input paths" expectation:
  - this task may define input stubs only,
  - real focus/cursor/click routing is deferred to `TASK-0056B`.
- Gate E "surface ownership/reuse" expectation:
  - this task must define bounded surface ownership, VMO queueing, damage, and present sequencing,
  - kernel/MM/IPC production-grade reuse/perf closure remains delegated to `TASK-0054B`, `TASK-0054C`,
    `TASK-0054D`, `TASK-0288`, and `TASK-0290`.
- Gate E "perf claims" expectation:
  - v1 may claim deterministic skip-present/no-damage behavior and bounded work,
  - frame-budget or smoothness claims require measured scenes or follow-up perf tasks.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Host tests can be limited to protocol codec and in-proc composition (no QEMU):

- `tests/ui_windowd_host/`:
  - compose two surfaces with damage and verify resulting pixels match a golden.
  - verify “no damage → no present” behavior deterministically.
  - reject invalid dimensions, wrong format/stride, stale surface IDs, stale commit sequence numbers, and unauthorized
    surface/layer mutation.
  - prove marker/postflight helpers cannot report success before real compose/present state is observed.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: ready (w=..., h=..., hz=60)`
- `windowd: systemui loaded (profile=desktop|mobile)`
- `windowd: present ok (seq=... dmg=...)`
- `launcher: first frame ok`
- `SELFTEST: ui launcher present ok`
- `SELFTEST: ui resize ok`

### Closeout evidence after critical remediation (2026-04-27)

Green proof commands:

- `cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture` — `windowd` tests,
  `ui_windowd_host` 22 tests, launcher ack-before-marker proof, and selftest-client build/proof-manifest coverage.
- `cargo test -p ui_windowd_host reject -- --nocapture` — reject-filtered host proof for invalid dimensions/stride/format,
  missing/forged/wrong-rights VMO handles, stale surface IDs, stale commit sequence numbers, unauthorized layer mutation,
  buffer length mismatch, bounds rejects, marker/postflight-before-present rejection, postflight log-only rejection, and IDL shape checks.
- `cargo test -p ui_windowd_host capnp -- --nocapture` — generated Cap'n Proto codec/roundtrip proof for surface create,
  queue-buffer damage, scene commit, vsync subscribe, and input subscribe schemas.
- `cargo test -p launcher -- --nocapture` — minimal launcher package builds and proves no marker without present ack.
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os` — QEMU marker proof including the TASK-0055 headless UI ladder.
- `scripts/fmt-clippy-deny.sh` — workspace fmt/clippy/deny gate.
- `make build` → `make test` — valid repo test gate using the fresh `make build` artifacts.
- `make build` → `make run` — valid repo run/QEMU smoke gate using the fresh `make build` artifacts.

Proof notes:

- `source/services/windowd` is now the authority for surface IDs, layer commits, and present sequencing in the slice.
- Host tests assert desired behavior: exact pixels for two damaged surfaces, no-damage present skip, deterministic
  layer ordering, minimal present acknowledgements, vsync subscription behavior, explicit unsupported input stubs, atomic
  scene commit preservation after rejects, and marker rendering from ack evidence.
- VMO behavior is modeled as a typed handle/rights/buffer validation contract at the `windowd` boundary. No kernel
  zero-copy, VMO sealing/reuse, IPC fastpath, or perf claim is made.
- `tools/postflight-ui.sh` delegates to the canonical QEMU harness and rejects log-only closure attempts.
- Visible output, real input routing/focus/click, rich dev presets, and GPU/display-driver work remain follow-up scope.
- VMO scope closure: TASK-0055 proves UI-shaped VMO handle/rights/size validation at `windowd`; real kernel VMO
  capability transfer was not changed here and remains outside this task's claim.

## Touched paths (allowlist)

- `source/services/windowd/` (existing placeholder; replace with real bounded service/compositor logic)
- `source/services/windowd/idl/` (new Cap'n Proto contract if not generated through an existing IDL path)
- `userspace/apps/launcher/` (new demo client, minimal)
- `tests/ui_windowd_host/` (new host proof crate/package if needed)
- `source/apps/selftest-client/` (markers)
- `tools/postflight-ui.sh` (delegates)
- `scripts/qemu-test.sh` (marker list)
- `docs/dev/ui/overview.md` + `docs/dev/ui/foundations/quality/testing.md` + `docs/dev/ui/foundations/layout/profiles.md`
- `Cargo.toml` / `Cargo.lock` only for required workspace membership or generated package metadata; call out before editing.
- Config/policy docs or manifests only if `ui.profile`, display dimensions, or service permissions are introduced.

## Plan (small PRs)

1. **IDLs**
   - `surface.capnp`, `layer.capnp`, `vsync.capnp`, `input.capnp` (stub).
   - VMO handle types and rights documented.

2. **`windowd` compositor**
   - layer tree + surface registry
   - vsync tick (60Hz default) using the timing spine
   - damage-aware composition using renderer primitives (from v1a)
   - markers: ready/systemui loaded/present ok

3. **Minimal launcher**
   - creates a surface
   - draws a simple scene via CPU renderer into its VMO buffer
   - queues buffer with damage
   - marker `launcher: first frame ok`

4. **Config + policy**
   - config schema for `ui.profile` and display dimensions (host-first, OS-gated)
   - policy permissions for reading assets and spawning plugins (minimal)

Sequencing note:

- `TASK-0055` establishes the first `ui.profile` + display-dimension hooks.
- Deterministic QEMU developer presets (phone/tablet/laptop, orientation, Hz) are tracked separately in `TASK-0055D`
  so the base compositor task stays focused on the present/composition contract.

1. **Proof**
   - host tests for composition snapshots
   - OS selftest markers and postflight-ui
