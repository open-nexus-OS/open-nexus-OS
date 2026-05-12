---
title: TASK-0056C UI v2a extension: embedded reactor/runtime floor + present/input perf polish (input-to-frame latency + event coalescing + short-circuit compose)
status: In Progress
owner: @ui @runtime
created: 2026-03-29
depends-on:
  - TASK-0056
  - TASK-0056B
  - TASK-0253
follow-up-tasks:
  - TASK-0059
  - TASK-0062
  - TASK-0063
  - TASK-0064
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Security standards: docs/standards/SECURITY_STANDARDS.md
  - RFC (contract seed): docs/rfcs/RFC-0055-ui-v2a-embedded-reactor-runtime-floor-present-input-perf-contract.md
  - UI v2a baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Visible input bridge: tasks/TASK-0056B-ui-v2a-visible-input-cursor-focus-click.md
  - Input host core carry-in: tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md
  - Live QEMU input carry-in: tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md
  - UI perf floor baseline: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Kernel IPC fastpath: tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md
  - Kernel MM perf floor: tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0056` proves the first real-time UX semantics:

- double buffering,
- vsync-aligned present,
- input hit-testing and focus.

`TASK-0056B` owns the deterministic visible-input proof. `TASK-0252`/`TASK-0253`
then provide the live QEMU pointer/keyboard path. This follow-up exists after
that input pipeline to make the path feel responsive enough for the
Orbital-Level UX gate before scrolling, animation, window management, and
launcher work build on it.

This task is the embedded **reactor/runtime minimum floor** for the UI fast lane.
It should tighten the common path across `inputd` -> `fbdevd` -> `windowd`
without creating a detached parallel subsystem. `TASK-0059`, `TASK-0062`,
`TASK-0063`, and `TASK-0064` must extend this floor rather than re-invent it.

Current carry-in reality (2026-05-10):

- `TASK-0253` is now `Done`; `RFC-0053` and `RFC-0054` are `Done`.
- The live service-owned chain is real and already verified:
  - `virtio-input -> hidrawd -> inputd -> windowd -> fbdevd -> ramfb`
  - `selftest-client` remains observer-only for visible/input proof collection.
- Existing chain telemetry already exposes useful floor signals:
  - `inputd`: `recv_hz`, `raw_events`, `norm_events`, `dispatch`, `delivered`,
    overflow counters, idle yields, and live-pointer/keyboard presence,
  - `windowd`: `compose_hz`, `present_hz`, coalesced-event count, dropped count,
    and damage pixels,
  - `fbdevd`: `flush_hz`, `vsync_hz`, flushed bytes, flush failures, and stale
    scanout count.
- Existing code already contains narrow reactor/perf seams:
  - `windowd` tracks coalesced present fences and latency per present,
  - `fbdevd` already runs a bounded cadence gate in its reactor,
  - `inputd` already records bounded live-chain counters and idle-yield posture.
- What is not yet closed, and therefore belongs to this task:
  - no end-to-end common-case input-to-frame latency budget proof,
  - no task-owned host proof package for deterministic motion-burst coalescing,
  - no 56C marker ladder for present fastpath / no-damage skip / idle-cheap closure,
  - no explicit rule set yet for when coalescing is allowed vs forbidden across
    click/focus/wheel/key semantic boundaries.

## Goal

Deliver the minimum runtime/reactor floor and a focused present/input perf polish slice:

1. **Common-case input-to-frame latency tightening**:
   - reduce the time from live pointer/click/wheel/key delivery to visible frame update,
   - add stable counters for the common path.
2. **Event coalescing**:
   - coalesce live pointer-motion bursts deterministically within and across present cadence,
   - keep click/focus/wheel/key semantics correct while reducing redundant work.
3. **Short-circuit compose/present rules**:
   - no damage and no visible state change → skip compose/present deterministically,
   - unchanged surfaces and idle input should not trigger avoidable work,
   - idle desktop path should settle into a low-work state.
4. **Common-case caches / wakeup collapse**:
   - lightweight hit-test/focus shortcuts where correctness is obvious,
   - fence/wakeup collapse only where semantics stay explicit,
   - avoid unnecessary visible-state fetch / compose / present work in the common case.
5. **Embedded handoff to later fast-lane tasks**:
   - establish explicit counters, damage rules, and present reasons that `TASK-0059`,
     `TASK-0062`, `TASK-0063`, and `TASK-0064` can extend.
6. **Ist-state sync + neutralized risk posture**:
   - document the actual 0253 carry-in instead of the older "after live input" posture,
   - turn the main correctness/security red flags into explicit mitigations and proof obligations,
   - keep the task small and honest: no detached runtime track, no fake perf success claims.

## Non-Goals

- New full input device stacks; consume the live QEMU input path from `TASK-0252`/`TASK-0253`.
- A separate standalone runtime/platform track outside the UI fast lane.
- Blur, glass, or backdrop work (handled by `TASK-0059` / `TASK-0060B`).
- Full window manager behavior.
- Kernel redesign; consume the `TASK-0054B/C/D` floor if present.
- Re-opening ownership questions already closed in `TASK-0253` / `RFC-0054`.

## Constraints / invariants (hard requirements)

- Preserve `TASK-0056` present and focus semantics.
- Preserve `TASK-0056B` visible affordance semantics and `TASK-0253` live pointer/keyboard semantics.
- Preserve the verified driver-owned live-input chain from `TASK-0253`; 56C must
  consume it, not bypass or replace it.
- Event coalescing must be deterministic and bounded.
- No "fast path" that skips hit-testing correctness for clicks/focus.
- Pointer-motion coalescing may collapse redundant motion bursts, but click,
  focus-transfer, wheel, and keyboard transitions must keep their semantic edges.
- Preserve service ownership boundaries: `inputd` owns normalized input state, `fbdevd` owns display polling/present loop, `windowd` owns hit-test/focus/present semantics.
- Observer/proof latching must not become sticky render-state behavior.
- No latency marker can pass on selftest-only input if the live pointer path regresses.
- No "perf win" may come from hiding work behind stale visible state or a marker
  that fires before a real visible update.
- Any cache / wakeup collapse rule must be derivable from explicit no-damage /
  no-visible-state-change conditions.
- Boundedness remains mandatory for coalescing windows, queue growth, cached
  shortcuts, and telemetry payloads.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

This is a perf-oriented UI task, but it is still security-relevant at the
authority/integrity layer because it touches the live input and present chain.

### Threat model

- A "fast path" bypasses `windowd` hit-test/focus authority or `inputd`
  normalization semantics.
- Coalescing or wakeup collapse drops semantically important transitions
  (click/focus/wheel/key) and produces fake-green UX behavior.
- Unbounded motion bursts or telemetry/caching paths create a CPU/queue DoS
  posture instead of an idle-cheap one.
- Perf markers claim success on observer-only or selftest-only input rather than
  the real 0253 live device path.

### Security invariants

- `inputd` remains the only normalized input authority; `windowd` remains the
  only hit-test/focus/click authority.
- No ambient authority or policy shortcut is introduced while tightening the
  common path.
- All coalescing windows, caches, and counters stay explicitly bounded.
- Fastpath markers are emitted only after a real visible-state transition or a
  proven no-damage/no-visible-change decision.

### DON'T DO

- Do not bypass `inputd` / `windowd` authority to save a wakeup.
- Do not silently drop wheel/key/click/focus edges under "latest-wins"
  coalescing.
- Do not emit perf/latency success markers from selftest-only injection if the
  live path regresses.
- Do not introduce unbounded per-frame telemetry, queue growth, or cache state.

### Security proof expectation

- Add at least one `test_reject_*` or equivalent negative proof for:
  - forbidden semantic-edge coalescing,
  - overflow/budget breach behavior,
  - marker-before-visible-state dishonesty.

## Red flags / decision points

- **YELLOW (semantic-edge loss under coalescing)**:
  - Risk: motion-burst collapse accidentally eats click/focus/wheel/key edges.
  - Neutralization: latest-wins applies only to bounded pointer-motion bursts;
    semantic edges remain explicit and individually observable in host proofs.
- **YELLOW (fake-green perf markers)**:
  - Risk: `windowd: * fastpath ok` fires from observer/selftest behavior rather
    than the real live chain.
  - Neutralization: perf markers must be downstream of the verified 0253 live
    path and tied to a real visible update or explicit no-damage skip decision.
- **YELLOW (ownership drift via "runtime floor")**:
  - Risk: 56C grows a detached runtime or bypass loop beside `inputd`/`fbdevd`/`windowd`.
  - Neutralization: task wording and touched paths stay embedded in the current
    service chain only; later tasks extend the same floor.
- **YELLOW (idle-cheap claim without boundedness)**:
  - Risk: "faster" loops simply poll harder or accumulate hidden work.
  - Neutralization: idle-cheap closure requires bounded yields/counters and a
    no-damage/no-visible-change proof, not just higher FPS numbers.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v2c_host/` or equivalent:

- task docs reflect the real 0253 carry-in and the task's own red-flag/security posture,
- pointer burst is coalesced deterministically,
- live QEMU pointer burst is coalesced without losing the latest visible cursor position,
- click/wheel/key state changes each cause at most one visible frame update per cadence in the common case,
- no-damage / unchanged-state path skips avoidable fetch/compose/present work,
- idle path stays quiet and exposes stable low-work counters,
- focus correctness is unchanged,
- reject-path proof exists for forbidden semantic-edge collapse or dishonest fastpath marker emission.

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `windowd: present fastpath on`
- `windowd: pointer coalesce ok`
- `windowd: no-damage skip ok`
- `windowd: idle fastpath ok`
- `windowd: click latency ok`
- `windowd: keyboard latency ok`
- `SELFTEST: live pointer latency ok`
- `SELFTEST: live keyboard latency ok`
- `SELFTEST: ui v2 perf ok`

### Quality-gate closeout expectation

- `just dep-gate`
- `just diag-os`
- `just diag-host`
- `just ci-network`
- task-focused host tests for the 56C proof package
- task-focused QEMU proof lane

Repo-wide `scripts/fmt-clippy-deny.sh` / `just test-all` may still be run as a
later explicit broad-gate pass, but 56C must not rely on them as the only proof
of correctness.

## Touched paths (allowlist)

- `source/services/windowd/`
- `source/services/fbdevd/`
- `source/services/inputd/`
- `userspace/input-live-protocol/`
- `userspace/apps/launcher/` (or other small proof surface)
- `tests/ui_v2c_host/` (new)
- `source/apps/selftest-client/`
- `tools/nx/tests/interactive_os_startup.rs`
- `scripts/run-qemu-rv64.sh`
- `scripts/qemu-test.sh`
- `docs/dev/ui/input/input.md`
- `docs/dev/ui/foundations/rendering/renderer.md`
- `docs/dev/ui/foundations/quality/testing.md`

## Plan (small PRs)

1. add common-case chain counters and short-circuit rules across `inputd` / `fbdevd` / `windowd`
2. add deterministic pointer-motion burst coalescing without losing latest visible state
3. tighten input-to-frame visible update path and idle cheap behavior
4. add host/QEMU proof scenes, reject proofs, and docs
