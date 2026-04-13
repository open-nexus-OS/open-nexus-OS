---
title: TASK-0212 DSoftBus v1.1d (OS/QEMU): busdir wiring + rpcmux calls + health indicators + flow-control enforcement + nx-bus + SystemUI integration + selftests/docs
status: Draft
owner: @runtime
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus v1 OS slice baseline (localSim + share demo + nx-bus): tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - DSoftBus v1.1 secure channels + encrypted framing: tasks/TASK-0195-dsoftbus-v1_1a-host-secure-channels-encrypted-streams-share.md
  - DSoftBus v1.1c busdir/rpcmux/health (host-first): tasks/TASK-0211-dsoftbus-v1_1c-host-busdir-rpcmux-health-flow.md
  - UDP devnet discovery gating: tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md
  - Policy caps baseline: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Wire busdir/rpcmux/health capabilities into OS services, SystemUI, and selftests.
- **Deliver**: Deterministic QEMU proofs for directory watch, parallel req/resp, health degradation, and share resume.
- **Out of scope**: Claiming UDP/LAN success without unblocked OS networking.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock OS wiring boundaries and marker truth rules.
- **Phase B (Host proof)**: requirement-named host control-plane rejects are green.
- **Phase C (OS-gated proof)**: canonical QEMU marker ladder and bounded selftests are green.
- **Phase D (Performance gate)**: deterministic parallel req/resp and health-transition budgets are met.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: `cargo test -p dsoftbus_v1_1_host -- --nocapture`
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
- 2-VM (if distributed behavior is asserted): `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

This task wires DSoftBus v1.1 control-plane features into OS/QEMU:

- service directory (busdir) as the source of truth for “what services exist on which peers”,
- rpcmux for generic req/resp calls on top of the existing secure/framed channel,
- health state from keepalive (up/degraded/down),
- and deterministic flow-control/quotas in the send path.

Loopback remains the default transport. UDP mode stays devnet-gated and must not claim success without real OS UDP sockets (`TASK-0196`). Init must enforce a ready gate before dependent flows proceed.

## Goal

Deliver:

1. `busdir` service running in OS:
   - populated by `dsoftbusd` announce/unannounce updates
   - TTL expiry
   - watch notifications delivered over rpcmux (control stream)
2. `dsoftbusd` integration:
   - enable rpcmux on top of the authenticated framed channel (or on top of Mux v2 if present)
   - keepalive/health transitions exposed to busdir metadata and CLI/UI
   - markers:
     - `busdir: up name=<name> fp=<fp>`
     - `bus: health degraded fp=<fp>`
3. SystemUI integration (minimal):
   - share panel lists peers via `busdir.list("share")`
   - shows health state and disables send when down
   - uses watch token to live-update
   - markers:
     - `ui: share panel watch token=<t>`
     - `ui: share health fp=<fp> state=<state>`
4. `nx-bus` extensions (host tool):
   - `dir list/watch`, `call`, `stats`
   - NOTE: QEMU selftests must not require running host tools inside QEMU
5. OS selftests (bounded):
   - `SELFTEST: bus dir watch ok`
   - `SELFTEST: bus mux parallel ok`
   - `SELFTEST: bus health degrade ok`
   - `SELFTEST: bus share resume ok`
   - and devnet gating sanity:
     - `SELFTEST: bus mode gate ok` (reuse from `TASK-0196` if present)
6. Docs:
   - busdir contract and watch semantics
   - rpcmux header + sequencing + errors
   - health states and determinism policy
   - nx-bus usage
7. **Init-ready gate (strukturell)**:
   - promote dsoftbus readiness into init-orchestrated gating (init waits on the Ready RPC before declaring dependent tests ready).

## Non-Goals

- Kernel changes.
- Real LAN discovery correctness (UDP mode is separate and gated).

## Constraints / invariants (hard requirements)

- No fake success:
  - “mux parallel ok” must verify two concurrent req/resp flows, not log greps.
  - “health degrade ok” must be proven by a deterministic keepalive simulation (test mode is explicit).
- Bounded timeouts; no busy-wait.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (marker honesty)**:
  - `SELFTEST: bus mux parallel ok` must reflect real concurrent req/resp, not log-only evidence.
- **YELLOW (ui coupling)**:
  - SystemUI watch/update wiring must remain non-authoritative for health; busdir/dsoftbusd state remains source of truth.

## Security considerations

### Threat model

- Unauthorized rpc calls or busdir watch subscriptions in OS wiring path.
- UI-triggered operations bypassing capability checks.
- False health state projection causing unsafe remote actions.

### Security invariants (MUST hold)

- busdir/rpcmux operations require authenticated and authorized session context.
- UI actions are policy-gated; disabled state is enforced when peer health is down.
- marker-based success claims require real protocol-level verification.

### DON'T DO (explicit prohibitions)

- DON'T accept unauthenticated control-plane traffic.
- DON'T let UI state override protocol health truth.
- DON'T mark selftests green based only on marker grep without state assertions.

### Attack surface impact

- Significant: OS control-plane exposure and UI action path.

### Mitigations

- policy checks on call paths, deterministic keepalive state assertions, bounded rpcmux quotas.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
- Required tests:
  - `test_reject_busdir_watch_without_auth`
  - `test_reject_rpc_call_without_required_cap`
  - `test_reject_send_when_peer_health_down`

### Hardening markers (QEMU, if applicable)

- `SELFTEST: bus dir watch ok`
- `SELFTEST: bus mux parallel ok`
- `SELFTEST: bus health degrade ok`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture` (includes v1.1c coverage)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
  - Required markers:
    - `dsoftbusd: ready` (init-orchestrated ready gate enforced before dependent flows)
    - `SELFTEST: bus dir watch ok`
    - `SELFTEST: bus mux parallel ok`
    - `SELFTEST: bus health degrade ok`
    - `SELFTEST: bus share resume ok`

## Touched paths (allowlist)

- `source/services/busdir/`
- `source/services/dsoftbusd/`
- `userspace/systemui/` (share panel integration)
- `tools/nx-bus/`
- `source/apps/selftest-client/`
- `schemas/dsoftbus_v1_1.schema.json`
- `docs/dsoftbus/` + `docs/tools/nx-bus.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. busdir service bring-up + integration in dsoftbusd
2. rpcmux enabled on OS path + minimal call path
3. SystemUI share panel watch + health indicators
4. selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, busdir watch, parallel rpcmux calls, keepalive-derived health transitions, and share resume are proven deterministically via selftest markers.
- Readiness gating is enforced: init must hold dependent flows until `dsoftbusd: ready` is observed.

Follow-up:

- DSoftBus v1.2 Media Remote (media.remote@1, cast/transfer/group state sync over busdir+rpcmux, share@1 fallback, SystemUI cast picker, nx-media remote) is tracked as `TASK-0219`/`TASK-0220`.
