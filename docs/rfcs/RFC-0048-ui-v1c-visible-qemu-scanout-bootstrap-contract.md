# RFC-0048: UI v1c visible QEMU scanout bootstrap contract seed

- Status: Done
- Owners: @ui @runtime
- Created: 2026-04-29
- Last Updated: 2026-04-29
- Links:
  - Tasks: `tasks/TASK-0055B-ui-v1c-visible-qemu-scanout-bootstrap.md` (execution + proof SSOT)
  - Related RFCs: `docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md`, `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`, `docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md`

## Status at a Glance

- **Phase 0 (QEMU visible scanout bootstrap contract)**: ✅ complete
- **Phase 1 (authority + marker hardening)**: ✅ complete
- **Phase 2 (Gate E sync + follow-up handoff)**: ✅ complete

Definition:

- "Complete" means this RFC contract is implemented and the required proof gates are green with deterministic visible-scanout evidence. It does not grant cursor/input/perf/kernel production-grade closure.

## Scope boundaries (anti-drift)

This RFC is a design seed/contract. Implementation planning and proofs live in `TASK-0055B`.

- **This RFC owns**:
  - minimal visible QEMU scanout bootstrap contract over the existing `windowd` present path,
  - deterministic visible-scanout marker contract and proof requirements,
  - bootstrap display authority boundary compatible with later Display v1 tasks.
- **This RFC does NOT own**:
  - full Display v1.0 closure (`TASK-0250`, `TASK-0251`),
  - visible SystemUI scene closure (`TASK-0055C`),
  - real input routing/focus/cursor (`TASK-0056B`),
  - latency/smoothness or kernel production-grade perf closure (`TASK-0054B/C/D`, `TASK-0288`, `TASK-0290`).

### Relationship to tasks (single execution truth)

- `TASK-0055B` is the execution/proof SSOT for this contract slice.
- `TASK-0055` stays closed as headless-present baseline; this RFC must not retroactively broaden `RFC-0047`.
- Follow-on capability expansion must use new task/RFC slices instead of scope creep inside this contract.

## Context

`TASK-0055` closed headless `windowd` present with deterministic proofs but intentionally no guest-visible scanout claim. UI bring-up now needs a real QEMU graphics window with deterministic first visible frame evidence while still avoiding a second compositor/display stack.

## Goals

- Define one deterministic visible scanout bootstrap mode for QEMU `virt`.
- Reuse existing `windowd` + renderer ownership boundaries; no parallel display authority.
- Require real visible-frame evidence (not log-only optimism) with deterministic marker verification.
- Keep the scope small and hand off richer display/input behavior to follow-up tasks.

## Non-Goals

- Multi-display/hotplug support.
- virtio-gpu/GPU acceleration.
- Cursor, focus, click routing.
- Rich profile matrix and dynamic mode switching.

## Constraints / invariants (hard requirements)

- **Determinism**: fixed mode, fixed marker ladder, reproducible harness behavior for a fixed build/profile.
- **No fake success**: `display: first scanout ok` and `SELFTEST: display bootstrap guest ok` only after guest-side framebuffer write and `ramfb` configuration complete; harness verification is post-run evidence, not a guest-emitted fact.
- **Bounded resources**: fixed bootstrap dimensions/format/stride rules; bounded queue/depth/error paths.
- **Security floor**: MMIO/display capability access remains under `RFC-0017` authority model; fail closed for invalid rights/mode handoff.
- **Rust discipline**: no `unsafe` shortcuts, no `unwrap/expect` in production paths, explicit error returns and reject coverage for malformed/unauthorized states.
- **Stubs policy**: any bootstrap stub is explicitly labeled non-authoritative and cannot emit success markers.

## Proposed design

### Contract / interface (normative)

- QEMU bootstrap runs in graphics-capable mode for this task slice when `NEXUS_DISPLAY_BOOTSTRAP=1` is selected by the harness.
- `visible-bootstrap` is a proof-manifest harness/marker profile, not a SystemUI/launcher start profile.
- QEMU uses `ramfb`; the guest configures `etc/ramfb` through capability-gated `fw_cfg` MMIO and a contiguous VMO backing framebuffer.
- `windowd` remains source-of-truth for fixed-mode validation, surface/layer/present sequencing evidence, deterministic bootstrap pixels, and marker gating.
- Marker contract for this slice:
  - `display: bootstrap on`
  - `display: mode 1280x800 argb8888`
  - `display: first scanout ok`
  - `SELFTEST: display bootstrap guest ok`
- Marker emission is gated by real guest-side state transitions (mode set, first visible frame write, `ramfb` configuration complete); harness verification remains post-run evidence.

### Phases / milestones (contract-level)

- **Phase 0**: QEMU visible bootstrap mode + first deterministic scanout marker contract.
- **Phase 1**: authority and reject-path hardening (invalid mode/capability/pre-scanout marker fail closed).
- **Phase 2**: Gate E production-floor sync and explicit handoff to `TASK-0055C` / `TASK-0251`.

## Security considerations

- **Threat model**:
  - fake marker emission without real visible frame,
  - unauthorized/ambient MMIO display access,
  - confused authority via second ad-hoc scanout path,
  - malformed mode/stride/format inputs leading to unsafe memory behavior.
- **Mitigations**:
  - capability-gated MMIO and single display authority,
  - bounded mode/format validation before enablement,
  - marker gating tied to real scanout state + verify-uart pass,
  - explicit reject-path tests for unauthorized/invalid/precondition-fail cases.
- **Open risks**:
  - richer display profile support and production-grade perf remain follow-up scope.

## Failure model (normative)

- Invalid mode/stride/format/capability state fails closed with stable error classes.
- Pre-scanout success markers are rejected.
- No silent fallback to headless success markers for visible-scanout claims.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p windowd -p launcher -p ui_windowd_host -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap
```

### Deterministic markers

- `display: bootstrap on`
- `display: mode 1280x800 argb8888`
- `display: first scanout ok`
- `SELFTEST: display bootstrap guest ok`

## Alternatives considered

- Keep using headless-only markers from TASK-0055:
  - Rejected; does not prove guest-visible scanout.
- Add a temporary parallel compositor/display service:
  - Rejected; creates authority drift and migration risk.
- Claim visible success via screenshot/manual checks only:
  - Rejected; non-deterministic and not CI-proof-grade.

## Open questions

- `fbdevd` remains follow-up scope for `TASK-0251`; this slice intentionally uses a minimal `windowd`-owned bootstrap authority over QEMU `ramfb`.
- The absence of `display: first scanout ok` remains sufficient for pre-scanout failure. Success is additionally guarded by host rejects and `scripts/qemu-test.sh` marker prerequisites.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: QEMU visible bootstrap mode + deterministic first-scanout marker ladder — proof: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap`
- [x] **Phase 1**: authority/reject hardening (invalid mode/capability/pre-marker rejects) — proof: `cd /home/jenning/open-nexus-OS && cargo test -p ui_windowd_host reject -- --nocapture`
- [x] **Phase 2**: Gate E sync + handoff closure to `TASK-0055C`/`TASK-0251`.
- [x] Task linked with stop conditions + proof commands.
- [x] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
