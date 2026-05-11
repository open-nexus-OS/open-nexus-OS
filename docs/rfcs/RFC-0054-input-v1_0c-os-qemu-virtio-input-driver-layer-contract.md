# RFC-0054: Input v1.0c OS/QEMU virtio-input driver layer (`virtio-input` -> `hidrawd`)

- Status: Done
- Owners: @ui @runtime
- Created: 2026-05-05
- Last Updated: 2026-05-11
- Links:
  - Tasks: `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
    - `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md`
    - `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md`
    - `docs/rfcs/RFC-0006-userspace-networking-v1.md`

## Status at a Glance

- **Phase 0 (ownership + loop contract freeze)**: ✅
- **Phase 1 (minimal virtio-input MMIO driver)**: ✅
- **Phase 2 (hidrawd/inputd live route closure)**: ✅ complete; the real driver-owned `hidrawd -> inputd -> windowd` path, ingress/adapter truth, time-capped interactive `make run` / `just start` proofs, and the remaining broad repo-wide gates are green

Definition:

- "Complete" means the minimal virtio-input driver-layer contract is defined and the required host + OS proofs for the `TASK-0253` live lane are green.
- "Complete" does not include latency/perf-budget closure; that remains `TASK-0056C`.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract for the missing driver layer required by `TASK-0253`.

- **This RFC owns**:
  - the minimal userspace virtio-input MMIO driver layer for QEMU `virt`,
  - capability ownership for `device.mmio.input` in the live-input path,
  - the bounded cooperative polling/event-loop contract for that driver layer,
  - the explicit ingress/adapter seam from virtio-input receive truth into the existing `hidrawd` / `inputd` chain.
- **This RFC does NOT own**:
  - keymaps, repeat, pointer acceleration, or host input algorithms already owned by RFC-0052,
  - `inputd` routing authority or `windowd` hit-test/focus authority (owned by RFC-0053 / RFC-0050 / RFC-0051),
  - USB host-controller bring-up, I2C controller bring-up, or real hardware enumeration,
  - IRQ delivery to userspace (polling-only is acceptable for v1 per RFC-0017),
  - a permanent selftest-owned input bridge.

### Relationship to tasks (single execution truth)

- `TASK-0253` remains the execution SSOT and keeps RFC-0053 as the top-level live-input contract seed.
- This RFC exists because `TASK-0253` now needs an additional driver-layer contract that RFC-0053 should reference rather than silently absorb.

## Context

`TASK-0253` already proved the host/service-side `hidrawd` / `touchd` / `inputd` seams and the deterministic visible proof lane. The remaining live-QEMU closure gap is no longer "scene boot" or "runner setup"; the current gap is that the guest has visible UI and `device.mmio.input` capability plumbing, but no real driver/event-loop processing path for virtio-input events.

QEMU `virt` now exposes `virtio-keyboard-device`, `virtio-mouse-device`, and `virtio-tablet-device`, and init already discovers `VIRTIO_DEVICE_ID_INPUT` windows and can distribute `device.mmio.input` capabilities. What is missing is the bounded userspace driver layer that owns those caps, polls the virtqueues, and feeds the existing input authority chain.

## Goals

- Define a minimal userspace virtio-input driver layer for QEMU `virt` (device ID `18`).
- Make capability ownership explicit: the live input owner service receives `device.mmio.input`, not a long-term `selftest-client` bridge.
- Define a bounded cooperative event-loop model that fits the existing OS-lite runtime.
- Feed the existing `hidrawd -> inputd -> windowd` chain without introducing a second routing authority.

## Non-Goals

- A full USB/HID host stack.
- A full device manager or generalized hotplug framework.
- Interrupt-driven userspace input handling in v1.
- Replacing `inputd` as the single routing authority.
- Claiming perf closure for `TASK-0056C`.

## Constraints / invariants (hard requirements)

- **Determinism**: queue polling, event translation, and marker behavior must be deterministic under QEMU `-icount`.
- **No fake success**: `ready`/`ok` markers are emitted only after the driver owns a real virtio-input device and live events can traverse the real chain.
- **Bounded resources**: queue depth, staging buffers, retry counts, and `yield_()` loops must be explicitly bounded.
- **Single-writer ownership**: mutable driver/device state lives under a single cooperative event loop by default.
- **No selftest bridge for closure**: `selftest-client` may observe or help prove behavior, but it must not be the long-term owner of `device.mmio.input` for `TASK-0253` closure.
- **No duplicate routing authority**: the driver layer ingests and translates hardware events; `inputd` remains the only authority for routing and policy decisions.
- **Truth before distribution**: the receive layer must expose a testable raw-to-normalized adapter truth before `inputd` routing or UI observation is consulted.
- **Capability-gated device access**: MMIO mapping remains policy-gated and per-device bounded per RFC-0017.

## Proposed design

### Contract / interface (normative)

- New minimal driver crate under `source/drivers/input/virtio-input/`:
  - no_std-friendly,
  - MMIO probe for `VIRTIO_DEVICE_ID_INPUT`,
  - queue setup for a bounded event queue,
  - `poll(now)`-style bounded dequeue API that yields a finite batch of raw input events.
- Driver runtime ownership:
  - init grants `device.mmio.input` caps to the live driver owner service,
  - the closure target is `hidrawd` as the first-class owner/backend consumer,
  - `selftest-client` may remain an observer for proof markers but must not be the final input-driver authority.
- Event translation seam:
  - raw virtio-input receive batches are captured as explicit ingress truth owned by `hidrawd`,
  - keyboard events become `hidrawd` keyboard events,
  - relative/absolute pointer events become `hidrawd` mouse/pointer-facing events,
  - `inputd` consumes those events through its existing merge/config/route authority.
- Receive truth layering:
  - receive truth = what the virtqueue delivered,
  - adapter truth = how `hidrawd` normalized that receive batch,
  - distribution truth = what `inputd` routed onward,
  - visible truth = what `windowd` / the proof scene rendered.
- Observer posture:
  - `selftest-client` and live breadcrumbs may observe distribution/visible truth,
  - they must not be treated as the authority for receive truth.
- Loop model:
  - explicit cooperative poll loop,
  - `QueueEmpty` / "no event" is normal and must yield,
  - no background threads and no unbounded busy-waiting.

Suggested module shape (non-normative but recommended):

- `source/drivers/input/virtio-input/src/{lib,mmio,queue,types,poll}.rs`
- `source/services/hidrawd/src/{main,service,ingest,error,types}.rs`

### Phases / milestones (contract-level)

- **Phase 0**: freeze ownership + cooperative polling contract; reject a selftest-owned final architecture.
- **Phase 1**: land the minimal virtio-input MMIO driver layer with bounded polling and host/unit proofs.
- **Phase 2**: connect `hidrawd` / `inputd` / `windowd` to the real driver path, make the ingress adapter seam explicit, and prove live keyboard/pointer behavior in QEMU interactive start.

## Security considerations

- **Threat model**:
  - malformed or unexpected virtio-input frames,
  - capability confusion (wrong MMIO window / wrong owner),
  - unbounded polling loops that starve cooperative scheduling,
  - authority drift where driver code starts making routing decisions,
  - false diagnosis where UI-visible failure is blamed on routing even though the receive layer was never proven.
- **Mitigations**:
  - verify MMIO magic + device ID before claiming readiness,
  - keep per-device MMIO caps bounded and owner-specific,
  - use a single-writer bounded event loop with explicit `yield_()` points,
  - keep translation-only semantics in the driver layer; route/policy stays in `inputd`,
  - require host-testable proofs for the raw-to-normalized adapter seam instead of inferring receive truth from later UI behavior.
- **Open risks**:
  - exact classification of QEMU tablet-style absolute events (`hidrawd` vs `touchd`) must be frozen in `TASK-0253` Phase 0,
  - IRQ-driven wakeups remain deferred; v1 relies on polling.

## Failure model (normative)

- missing or wrong virtio-input MMIO device -> deterministic explicit failure (no synthetic ready marker),
- queue empty / no event -> normal loop outcome; caller yields and retries,
- malformed event record -> deterministic reject/drop with bounded logging,
- absent live driver owner or denied MMIO cap -> explicit startup failure, no fallback to selftest-owned authority,
- no silent fallback from the driver layer directly into `windowd`.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p virtio-input -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p hidrawd -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p nx --test interactive_os_startup
```

The dedicated `virtio-input` crate host proof is now mandatory for RFC-0054 progression.

### Truth-layer gate matrix (required in this RFC slice)

RFC-0054 closure requires both classes of gates:

- **General platform gates** (capability/routing/IPC): capability ownership handoff, deterministic named-route fallback behavior, and explicit routing exposure for the live owner chain.
- **Input-specific gates** (driver/ingest/live-lane honesty): virtio-input role detection, explicit raw-to-normalized adapter truth, hidrawd readiness honesty under late cap transfer, and live-route marker integrity.

Required truth layers inside the input-specific floor:

- **Receive truth**: raw virtio-input batches are observed without depending on `inputd` or UI behavior.
- **Adapter truth**: `hidrawd` normalization of those batches is host-testable and deterministic.
- **Distribution truth**: `inputd` consumes only normalized batches and remains the sole routing authority.
- **Visible truth**: proof-scene/UI assertions remain downstream evidence, not the first place raw-input arrival becomes visible.

Mandatory floor for this slice:

- `cargo test -p virtio-input -- --nocapture`
- `cargo test -p hidrawd -- --nocapture`
- `cargo test -p inputd -- --nocapture`
- `cargo test -p nx --test interactive_os_startup`
- `RUN_PHASE=input-startup RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s scripts/qemu-test.sh --profile=visible-bootstrap`

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && make build && just start
cd /home/jenning/open-nexus-OS && make build && make run
```

The deterministic visible proof lane from RFC-0053 must remain green; this driver-layer RFC adds the real live-input path rather than replacing the deterministic harness lane.

### Deterministic markers (required, non-exhaustive)

- `hidrawd: virtio-input mmio ready`
- `hidrawd: virtio-input keyboard ready`
- `hidrawd: virtio-input pointer ready`
- `hidrawd: ingress adapter ready`
- `inputd: live pointer route on`
- `inputd: live keyboard route on`

## Alternatives considered

- Keep `device.mmio.input` permanently owned by `selftest-client` (rejected: architecture drift and duplicate authority risk).
- Build USB/HID first instead of using virtio-input (rejected: much larger scope for QEMU `virt` closure).
- Route driver events directly to `windowd` (rejected: bypasses `inputd` authority and duplicates routing policy).

## Resolved classification note

- QEMU tablet-style absolute input is normalized through the explicit
  `virtio-input -> hidrawd` pointer path.
- Touch-specific semantics remain distinguishable through the carried
  `PointerSource::TouchAbsolute` wire classification where the upstream source is
  touch-like, but the driver layer itself does not fork a second routing
  authority around `hidrawd` / `inputd`.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: ownership + loop contract frozen — proof: `task+RFC review`
- [x] **Phase 1**: minimal virtio-input MMIO driver layer green — proof: `cargo test -p virtio-input -- --nocapture` + `cargo test -p hidrawd -- --nocapture`
- [x] **Phase 2**: live QEMU driver path reaches `hidrawd -> inputd -> windowd` with explicit ingress/adapter truth — proof: `make build && just start`
- [x] Task linked with stop conditions + proof commands.
- [x] Interactive QEMU path exposes keyboard + pointer devices and passes the focused `nx` contract test.
- [x] Security-relevant negative tests exist (`test_reject_*`) for wrong device ID, malformed events, bounded queue behavior, and wrong receive-to-adapter classification.
