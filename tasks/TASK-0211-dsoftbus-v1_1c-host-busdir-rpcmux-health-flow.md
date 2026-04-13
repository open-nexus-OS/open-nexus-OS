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

## Short description

- **Scope**: Add host-first control-plane stack: bus directory, rpc multiplexing, health transitions, and quota/backpressure rules.
- **Deliver**: Deterministic watch/rpc/keepalive behavior on top of secure framed transport (reusing Mux v2 flow semantics where present).
- **Out of scope**: Building a parallel mux layer that duplicates TASK-0020 responsibilities.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock busdir/rpcmux/health semantics and quota boundaries.
- **Phase B (Host proof)**: requirement-named host suites for control-plane rejects and ordering are green.
- **Phase C (OS-gated proof)**: OS marker claims require real protocol-level evidence.
- **Phase D (Performance gate)**: deterministic inflight/backpressure/latency budgets are validated.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: `cargo test -p dsoftbus_v1_1_host -- --nocapture`
- OS (if touched): `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- 2-VM (if distributed behavior is asserted): `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

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

## Security considerations

### Threat model

- Unauthorized service directory updates or watcher subscriptions.
- rpcmux inflight abuse to trigger memory/queue exhaustion.
- Keepalive/health spoofing that masks dead peers as healthy.

### Security invariants (MUST hold)

- busdir updates are accepted only from authenticated/authorized session context.
- inflight streams/bytes are bounded with deterministic reject behavior.
- health state transitions are deterministic and not user-forgeable through payload-only inputs.

### DON'T DO (explicit prohibitions)

- DON'T run rpcmux as an unbounded request queue.
- DON'T duplicate flow-control logic that bypasses Mux v2 guarantees.
- DON'T emit healthy/up markers without verified keepalive evidence.

### Attack surface impact

- Significant: directory/rpc control-plane and health surfaces.

### Mitigations

- authenticated control stream, bounded inflight windows, and strict keepalive state machine checks.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus_v1_1_host -- --nocapture`
- Required tests:
  - `test_reject_busdir_update_from_unauthenticated_peer`
  - `test_reject_rpcmux_inflight_limit_exceeded`
  - `test_reject_health_state_spoof_without_keepalive_path`

### Hardening markers (QEMU, if applicable)

- `SELFTEST: bus dir watch ok`
- `SELFTEST: bus health degrade ok`

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
