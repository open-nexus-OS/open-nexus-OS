---
title: TASK-0211 DSoftBus v1.1c (host-first): busdir (service directory + watch) + rpcmux (req/resp over framed channels) + keepalive/health + quotas/backpressure + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - DSoftBus v1 localSim baseline: tasks/TASK-0157-dsoftbus-v1a-local-sim-pairing-streams-host.md
  - DSoftBus v1 OS wiring baseline: tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - DSoftBus v1.1 secure channels + encrypted framing + share resume: tasks/TASK-0195-dsoftbus-v1_1a-host-secure-channels-encrypted-streams-share.md
  - DSoftBus mux/flow control substrate: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - UDP discovery devnet-gated (follow-up): tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We already have:

- v1 localSim (discovery/pairing/streams) (`TASK-0157`),
- v1.1 secure channels and encrypted framed streams (`TASK-0195`),
- a dedicated Mux v2 plan for stream multiplexing + flow control + keepalive (`TASK-0020`),
- optional UDP discovery gating (`TASK-0196`).

This task adds “control-plane” features on top of the secure channel/framed stream substrate:

- a first-class service directory (busdir) with watch/notifications,
- an RPC request/response multiplexer (rpcmux) suitable for small Cap’n Proto RPC-like calls,
- deterministic keepalive/health state surfaced for UI/CLI,
- and deterministic quota/backpressure behavior (prefer reuse of Mux v2 windows where available).

Host-first proofs come first; OS wiring is in v1.1d.

## Goal

Deliver:

1. `userspace/libs/rpcmux`:
   - request/response protocol on top of an authenticated framed stream:
     - stream_id, seq, flags, msg_type
     - payload bytes = Cap’n Proto message bytes (already project-wide)
   - deterministic priority:
     - control (PING/PONG, busdir notify) > rpc > bulk
   - backpressure:
     - cap inflight streams and inflight bytes (config-driven)
     - deterministic errors on exceed (stable error enum)
   - NOTE: if Mux v2 (`TASK-0020`) is present, `rpcmux` should run on top of a named/priority mux stream rather than inventing a parallel flow-control mechanism.
2. `source/services/busdir`:
   - directory index keyed by `(peer_fp, name, ver)` with TTL expiry
   - list/listAll/info
   - watch/unwatch with notify delivery over rpcmux control stream
3. Keepalive/health:
   - PING/PONG idle keepalive with deterministic timers (injected clock in tests)
   - missing PONG transitions: `up -> degraded -> down` deterministically
4. Flow-control & quotas:
   - schema knobs:
     - max inflight streams/bytes, window bytes, max frame bytes, optional rate limit
   - integration with file-share/send paths:
     - violations surface as stable errors (`EWINDOW`, `ERATE`, `EDQUOT` or similar)
5. Host CLI `nx-bus` (host tool):
   - directory list/watch
   - generic call (req/resp)
   - stats (health + inflight)
6. Deterministic host tests `tests/dsoftbus_v1_1_host/` additions (or a new crate if needed):
   - directory index + TTL expiry + watch notifications
   - rpcmux parallelism across multiple streams (ordering preserved per stream)
   - keepalive state transitions via injected clock
   - flow-control exceed + recovery
   - share resume still works under the same channel scheduling policy

## Non-Goals

- Kernel changes.
- Replacing the Mux v2 plan: this task should reuse it where possible.
- Full cross-VM networking discovery (separate tasks).

## Constraints / invariants (hard requirements)

- Determinism: injected clock in tests; stable ordering; stable tie-breaks.
- Bounded memory: hard caps on inflight streams/bytes and max frame bytes.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (parallel multiplexers drift)**:
  - Avoid building a second “mux” that duplicates `TASK-0020`. `rpcmux` is for req/resp semantics; byte-level flow control should come from Mux v2 where possible.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture`
  - Additional coverage proves busdir+rpcmux+health+flow-control deterministically.

## Touched paths (allowlist)

- `userspace/libs/rpcmux/` (new)
- `source/services/busdir/` (new)
- `source/services/dsoftbusd/` (integration)
- `tools/nx-bus/` (extend)
- `tests/` (new/extend host tests)
- `docs/dsoftbus/` (docs may land in v1.1d if preferred)

## Plan (small PRs)

1. rpcmux header + backpressure + unit tests
2. busdir service + TTL + watch notify + tests
3. keepalive/health + tests
4. nx-bus extensions + docs + any marker contract updates (OS gated)

## Acceptance criteria (behavioral)

- Host tests deterministically prove directory/watch, RPC mux parallelism, keepalive health transitions, and bounded flow-control behavior.
