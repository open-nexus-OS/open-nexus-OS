# RFC-0047: UI v1b windowd surface/layer/present contract seed

- Status: In Progress
- Owners: @ui @runtime
- Created: 2026-04-27
- Last Updated: 2026-04-27
- Links:
  - Tasks: `tasks/TASK-0055-ui-v1b-windowd-compositor-surfaces-vmo-vsync-markers.md` (execution + proof SSOT)
  - Related tasks: `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md`, `tasks/TASK-0055C-ui-v1d-windowd-visible-present-systemui-first-frame.md`, `tasks/TASK-0055D-ui-v1e-dev-display-profile-presets-qemu-hz.md`, `tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md`, `tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md`, `tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md`, `tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md`, `tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md`
  - Related RFCs: `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md`, `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`, `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md`

## Status at a Glance

- **Phase 0 (surface/layer IPC + host composition proof)**: ⬜
- **Phase 1 (OS headless present + markers/postflight)**: ⬜
- **Phase 2 (hardening/reject paths + Gate E sync)**: ⬜

Definition:

- "Complete" means `TASK-0055` defines and proves the headless `windowd` surface/layer/present contract with deterministic host and OS/QEMU evidence.
- This RFC does not claim visible display output, real input routing, GPU/display-driver fences, or kernel/core production-grade closure.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in `TASK-0055`.

- **This RFC owns**:
  - `windowd` surface/layer authority boundaries,
  - VMO-backed surface queueing semantics for the UI slice,
  - bounded damage-aware composition and headless present sequencing,
  - minimal present acknowledgement semantics,
  - deterministic `windowd` / launcher / UI selftest marker rules for the headless path.
- **This RFC does NOT own**:
  - visible QEMU scanout (`TASK-0055B` / `TASK-0055C`),
  - real input routing/focus/click (`TASK-0056B`),
  - rich dev display/profile presets (`TASK-0055D`),
  - renderer Scene-IR / Backend trait finalization (`TASK-0169` / `TASK-0170`),
  - kernel scheduling, IPC fastpath, MM, VMO sealing/reuse, or zero-copy production-grade closure.

### Relationship to tasks (single execution truth)

- `TASK-0055` is the execution SSOT for paths, stop conditions, and proof commands.
- This RFC is the contract/rationale SSOT for authority boundaries, invariants, and anti-drift scope.
- If implementation discovers that scheduler, IPC, MM, VMO, timer, or display-driver behavior is too weak for honest UI claims, route the gap to the explicit follow-ups instead of broadening `TASK-0055`.

## Context

`TASK-0054` closed the host-only BGRA8888 renderer proof floor. The next slice needs the first OS-gated `windowd` present spine without claiming visible display or input. The existing `source/services/windowd/` crate is only a placeholder checksum/helper scaffold, so `TASK-0055` must introduce real bounded surface/layer/present behavior before any `windowd: ready` or `present ok` marker can be trusted.

## Goals

- Define a bounded `windowd` authority for surfaces, layers, scene commits, and present sequencing.
- Define VMO-backed surface queueing rules compatible with the zero-copy and future `NexusGfx` posture.
- Define deterministic host composition proofs and OS/QEMU headless present markers.
- Define failure/reject behavior for invalid, stale, unauthorized, or oversized requests.
- Keep visible display, real input, rich profiles, and kernel production-grade performance in follow-up tasks.

## Non-Goals

- Kernel changes.
- Visible QEMU scanout or real display output.
- Real input routing, focus, or cursor behavior.
- GPU, virtio-gpu, display-driver, MMIO, or IRQ work.
- Latency-accurate GPU/display fence semantics.
- A second renderer architecture parallel to `TASK-0169`.

## Constraints / invariants (hard requirements)

- **Determinism**: fixed composition order, bounded damage handling, deterministic markers/postflight.
- **No fake success**: `windowd: ready`, `windowd: present ok`, launcher, and `SELFTEST: ui ... ok` markers only after real checked behavior.
- **Bounded resources**: caps for surfaces, layers, dimensions, stride, total bytes, queued buffers, and damage rects.
- **Security floor**: callers are identified by service metadata; VMO handles/rights and layer mutations fail closed.
- **Stubs policy**: input stubs are explicit and cannot produce positive input-routing markers.

## Proposed design

### Contract / interface (normative seed)

The v1b contract should define:

- surface creation with bounded dimensions, format, stride, and VMO rights,
- buffer queueing with damage and sequence numbers,
- layer membership/order and atomic scene commit semantics,
- vsync/present tick behavior for a deterministic headless path,
- minimal present acknowledgement after composition,
- explicit error classes for invalid, stale, unauthorized, oversized, or unsupported operations.

### Phases / milestones (contract-level)

- **Phase 0**: host protocol/composition contract and goldens.
- **Phase 1**: OS headless present path with deterministic marker/postflight evidence.
- **Phase 2**: reject/hardening coverage plus docs/status sync.

## Security considerations

- **Threat model**: forged VMO handles, confused-deputy layer mutation, stale commits, oversized buffers, marker-only fake proof, and unbounded diagnostic leakage.
- **Mitigations**: service-metadata identity, capability/rights checks, bounded validation before allocation or composition, deterministic reject tests, and marker emission after checked state only.
- **Open risks**: kernel-level VMO sealing/reuse and IPC fastpath performance remain follow-up closure work.

## Failure model (normative)

- Invalid dimensions, stride, pixel format, VMO rights, stale surface IDs, stale commit sequence numbers, and unauthorized layer changes fail closed.
- A non-zero-copy fallback, if used, must be explicitly named and excluded from zero-copy/perf claims.
- Minimal present acknowledgement is not a latency-accurate GPU/display fence.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p ui_windowd_host -- --nocapture
```

If the host proof remains in `windowd`, the task must update this command while preserving the same proof classes.

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `windowd: ready (w=..., h=..., hz=60)`
- `windowd: systemui loaded (profile=desktop|mobile)`
- `windowd: present ok (seq=... dmg=...)`
- `launcher: first frame ok`
- `SELFTEST: ui launcher present ok`
- `SELFTEST: ui resize ok`

## Alternatives considered

- **Treat existing `windowd` checksum output as readiness**:
  - Rejected. Placeholder output is not surface/layer/present behavior.
- **Fold visible scanout into TASK-0055**:
  - Rejected. Visible output belongs to `TASK-0055B` / `TASK-0055C`.
- **Invent a parallel renderer/display contract**:
  - Rejected. `TASK-0055` must consume the TASK-0054 renderer floor and stay compatible with `TASK-0169` / `TASK-0170`.

## Open questions

- Whether the host proof should be a new `tests/ui_windowd_host` package or live under `source/services/windowd/tests`.
- Exact v1 error taxonomy names for surface/layer/present rejects.
- Whether `ui.profile` / display dimensions land as minimal config schema in TASK-0055 or are deferred entirely to TASK-0055D.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: Surface/layer IPC + host composition proof — proof: `cargo test -p ui_windowd_host -- --nocapture` or task-updated equivalent.
- [ ] **Phase 1**: OS headless present + deterministic markers/postflight — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`.
- [ ] **Phase 2**: Reject tests and security hardening — proof: `test_reject_*` host tests plus marker/postflight failure cases.
- [ ] Task linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
