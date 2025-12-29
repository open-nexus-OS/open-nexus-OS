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

## Context

This task wires DSoftBus v1.1 control-plane features into OS/QEMU:

- service directory (busdir) as the source of truth for “what services exist on which peers”,
- rpcmux for generic req/resp calls on top of the existing secure/framed channel,
- health state from keepalive (up/degraded/down),
- and deterministic flow-control/quotas in the send path.

Loopback remains the default transport. UDP mode stays devnet-gated and must not claim success without real OS UDP sockets (`TASK-0196`).

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

## Non-Goals

- Kernel changes.
- Real LAN discovery correctness (UDP mode is separate and gated).

## Constraints / invariants (hard requirements)

- No fake success:
  - “mux parallel ok” must verify two concurrent req/resp flows, not log greps.
  - “health degrade ok” must be proven by a deterministic keepalive simulation (test mode is explicit).
- Bounded timeouts; no busy-wait.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture` (includes v1.1c coverage)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
  - Required markers:
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

Follow-up:

- DSoftBus v1.2 Media Remote (media.remote@1, cast/transfer/group state sync over busdir+rpcmux, share@1 fallback, SystemUI cast picker, nx-media remote) is tracked as `TASK-0219`/`TASK-0220`.
