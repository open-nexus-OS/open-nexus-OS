# RFC-0055: UI v2a embedded reactor/runtime floor + present/input perf contract seed

- Status: Complete
- Owners: @ui @runtime
- Created: 2026-05-10
- Last Updated: 2026-05-11
- Links:
  - Tasks: `tasks/TASK-0056C-ui-v2a-present-input-perf-latency-coalescing.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md`
    - `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md`
    - `docs/rfcs/RFC-0053-input-v1_0b-os-qemu-live-input-hidrawd-touchd-inputd-contract.md`
    - `docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md`

## Status at a Glance

- **Phase 0 (contract freeze + carry-in map)**: ✅
- **Phase 1 (deterministic coalescing + skip rules)**: ✅
- **Phase 2 (latency proof + downstream handoff)**: ✅

Definition:

- "Complete" means the embedded runtime/reactor contract is defined and the required host + QEMU proof gates are green.
- "Complete" does not include live-input ingress closure; that remains carry-in from RFC-0053 / RFC-0054 and `TASK-0253`.
- "Complete" does not include scroll/effects/IME/window-management breadth; those remain downstream follow-up scope.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Execution sequencing, stop conditions, and proof reruns remain task-owned.

- **This RFC owns**:
  - the shared embedded runtime/reactor floor across `inputd -> windowd -> fbdevd`,
  - deterministic pointer-motion coalescing rules and explicit "do not coalesce" semantic-edge boundaries,
  - no-damage / no-visible-state-change skip rules,
  - idle-cheap / wakeup-collapse / stable counter expectations,
  - deterministic marker-honesty rules for the 56C perf slice.
- **This RFC does NOT own**:
  - live device ingress and driver ownership already closed in RFC-0053 / RFC-0054,
  - visible cursor/hover/focus/click semantics already owned by RFC-0051,
  - scroll, clip, effects, IME/text-input, animation, or WM breadth (`TASK-0059`, `TASK-0062`, `TASK-0063`, `TASK-0064`),
  - kernel perf-floor redesign in `TASK-0054B` / `TASK-0054C` / `TASK-0054D`,
  - a detached runtime/platform subsystem beside the existing UI fast lane.

### Relationship to tasks (single execution truth)

- `TASK-0056C` is the execution SSOT for this contract seed.
- `TASK-0056C` must consume the real live-input carry-in from `TASK-0253`; it must not reopen input ownership or back-claim 0253 perf closure.

## Context

The live QEMU input chain is now real and review-closed enough to stop talking
about "future input someday". `virtio-input -> hidrawd -> inputd -> windowd ->
fbdevd -> ramfb` exists, and current code already exposes bounded telemetry and
some narrow cadence/coalescing seams.

What is still missing is a contract for the common-case responsiveness floor:

- when motion bursts may coalesce,
- when compose/present may skip,
- how idle-cheap behavior is proven honestly,
- which markers/counters later UI tasks may extend instead of reinventing.

Without this RFC, later scroll/runtime/WM tasks are likely to grow parallel fast
paths, fake-green perf markers, or semantic-edge bugs at the `inputd` /
`windowd` / `fbdevd` boundary.

## Goals

- Define one embedded runtime/reactor floor for the current UI fast lane.
- Tighten common-case input-to-frame behavior without changing authority ownership.
- Define deterministic coalescing, no-damage skip, and idle-cheap contracts that later tasks extend.
- Require proof surfaces that distinguish real visible latency wins from marker-only optimism.

## Non-Goals

- Replacing `inputd`, `windowd`, or `fbdevd` with a new runtime owner.
- Reopening live-input device ownership already closed by RFC-0053 / RFC-0054.
- Claiming scroll/effects/IME/window-management closure.
- Claiming kernel/core production-grade perf closure.

## Constraints / invariants (hard requirements)

- **Determinism**: coalescing windows, present reasons, counters, and markers must be deterministic.
- **No fake success**: `ok` / `ready` / `latency ok` markers are emitted only after a real visible update or an explicit proven no-damage/no-visible-change decision.
- **Bounded resources**: motion bursts, queue growth, cached shortcuts, wakeup collapse, and telemetry payloads stay explicitly bounded.
- **Security floor**:
  - `inputd` remains the only normalized input authority,
  - `windowd` remains the only hit-test/focus/click/present-semantics authority,
  - `fbdevd` remains the cadence/scanout owner,
  - no ambient authority or routing shortcut is introduced for speed.
- **Stubs policy**: degraded or placeholder fast paths must say so explicitly and must not claim closure markers.
- **Semantic-edge integrity**: pointer-motion bursts may coalesce; click, focus transfer, wheel, and keyboard edges must stay individually observable.

## Proposed design

### Contract / interface (normative)

- `inputd`:
  - may collapse redundant pointer-motion bursts into bounded latest-wins batches,
  - must preserve wheel/key/click/focus-relevant edge delivery,
  - must expose stable chain counters for receive, normalize, dispatch, deliver, overflow, and idle-yield posture.
- `windowd`:
  - owns hit-test/focus/click correctness and any short-circuit compose decision,
  - may skip compose/present only when there is no damage and no visible state change,
  - must expose stable compose/present/coalesce/drop/damage counters and honest fastpath markers.
- `fbdevd`:
  - owns present cadence and scanout handoff,
  - may collapse redundant wakeups only when the visible contract above remains explicit,
  - must expose stable cadence/flush/scanout counters.
- marker/counter vocabulary:
  - marker strings must stay stable and bounded,
  - counters must remain extendable by later tasks without redefining the same behavior under a second name.

### Phases / milestones (contract-level)

- **Phase 0**: freeze authority boundaries, marker vocabulary, and carry-in assumptions from RFC-0053 / RFC-0054.
- **Phase 1**: land deterministic pointer-motion coalescing, no-damage skip, and idle-cheap rules with reject proofs.
- **Phase 2**: prove end-to-end latency posture honestly and hand the same floor to `TASK-0059`, `TASK-0062`, `TASK-0063`, and `TASK-0064`.

## Security considerations

- **Threat model**:
  - semantic-edge loss under overly aggressive coalescing,
  - authority bypass where "runtime floor" starts making routing or focus decisions,
  - unbounded motion/counter/cache growth causing CPU or queue DoS,
  - fake-green markers emitted from observer-only or selftest-only paths.
- **Mitigations**:
  - keep authority split explicit (`inputd` normalize, `windowd` decide, `fbdevd` present),
  - bound coalescing windows and cache/counter growth,
  - require negative proofs for forbidden semantic-edge collapse and marker-before-visible-state dishonesty,
  - tie QEMU marker success to the real live-input carry-in rather than selftest-only injection.
- **Open risks**:
  - exact counter vocabulary may evolve during the first implementation cut,
  - some host proof packaging details (`tests/ui_v2c_host/` vs crate-local tests) remain task-owned.

## Failure model (normative)

- no-damage but visible-state change -> must not skip,
- semantic-edge event inside a motion burst -> must not be silently coalesced away,
- overflow or budget breach -> deterministic bounded degrade / reject behavior,
- marker request before real visible outcome -> reject / withhold marker,
- no silent fallback from 56C fast paths into a stale visible state claim.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p windowd -p inputd -p fbdevd -- --nocapture
```

Task-local host proofs must additionally cover the 56C contract with a dedicated
`tests/ui_v2c_host/` package or equivalent requirement-named suites before
closure is claimed.

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os visible-bootstrap
```

The deterministic QEMU lane must stay downstream of the real RFC-0053 /
RFC-0054 live-input chain; selftest-only injection cannot be the sole perf proof.

### Deterministic markers (required, non-exhaustive)

- `windowd: present fastpath on`
- `windowd: pointer coalesce ok`
- `windowd: no-damage skip ok`
- `windowd: idle fastpath ok`
- `windowd: click latency ok`
- `windowd: keyboard latency ok`
- `SELFTEST: live pointer latency ok`
- `SELFTEST: live keyboard latency ok`
- `SELFTEST: ui v2 perf ok`

## Alternatives considered

- Build a detached runtime subsystem beside `inputd` / `windowd` / `fbdevd` (rejected: ownership drift and duplicate fast paths).
- Treat higher FPS or lower wakeups as sufficient proof without semantic-edge tests (rejected: easy fake-green path).
- Extend RFC-0053 / RFC-0054 instead of creating a new contract seed (rejected: those RFCs are already closed and should not become backlog containers).

## Open questions

- Should the task introduce a dedicated `ui_v2c_host` crate or keep the proof floor inside existing packages?
- Which counter names should be considered stable SSOT vs provisional bring-up detail?

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: contract freeze + carry-in map synced — proof: `task+RFC review`
- [x] **Phase 1**: deterministic coalescing + skip rules landed — proof: `TASK-0056C host proofs + reject suites`
- [x] **Phase 2**: latency proof + downstream handoff closed — proof: `TASK-0056C QEMU perf ladder + quality gates`
- [x] Task linked with stop conditions + proof commands.
- [x] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
