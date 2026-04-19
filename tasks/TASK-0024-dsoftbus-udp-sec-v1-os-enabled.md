---
title: TASK-0024 DSoftBus QUIC-v2 OS follow-up: reliability + bounded recovery + transport hardening
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0020
  - TASK-0022
follow-up-tasks:
  - TASK-0044
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - ADR: docs/adr/0006-device-identity-architecture.md
  - Depends-on (DSoftBus core in OS): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Depends-on (OS networking UDP): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Related (proof manifest infra needed for new QUIC-required markers; Phase 4 closed 2026-04-17): tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Close the remaining transport features on top of the already shipped OS QUIC-v2 session path from `TASK-0023`.
- **Deliver**: Real reliability/recovery/congestion behavior (bounded, deterministic) for the OS QUIC-v2 datapath and explicit proof coverage.
- **Out of scope**: full IETF QUIC/TLS parity and kernel-side changes.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock packet/recovery/crypto invariants and bounded queue rules.
- **Phase B (Host proof)**: requirement-named host loss/recovery tests and negative tests are green.
- **Phase C (OS proof)**: canonical OS QUIC marker ladder is green with real recovery/backpressure behavior.
- **Phase D (Performance gate)**: deterministic latency/throughput/recovery budgets are defined and met.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host:
  - `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
  - `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbus -- quic --nocapture`
  - `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture`
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- 2-VM distributed: `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

`TASK-0023` already shipped a real OS QUIC-v2 session path over UDP with:

- `dsoftbusd: transport selected quic`,
- Noise-XK auth over framed UDP datagrams,
- `dsoftbusd: auth ok`,
- `dsoftbusd: os session ok`,
- `SELFTEST: quic session ok`,
- fallback-marker rejection in QUIC-required QEMU profile.

What is still missing is not "session exists", but "transport behaves production-grade under loss and pressure".
`TASK-0024` owns this follow-up closure.

## Current implementation baseline (must be preserved)

- OS QUIC-v2 handshake/session flow is implemented in `source/services/dsoftbusd` and `source/apps/selftest-client`.
- UDP IPC primitives (`OP_UDP_BIND`, `OP_UDP_SEND_TO`, `OP_UDP_RECV_FROM`) are wired end-to-end via `netstackd`.
- QUIC frame parse/encode reject floor exists (bad magic/truncation/oversize encode) in `dsoftbusd` unit tests.
- QEMU harness requires QUIC markers and rejects legacy fallback markers in QUIC-required mode.

## Goal

In QEMU and host testbeds, prove:

- QUIC-v2 data transfer tolerates bounded packet loss/reordering via explicit ACK/retransmit behavior.
- Recovery queues, in-flight accounting, and receive reorder windows stay bounded.
- Mux-v2 traffic classes run unchanged over the hardened QUIC-v2 transport.
- Backpressure and congestion signals are deterministic and test-proven.

## Non-Goals

- Full IETF QUIC wire compatibility.
- 0-RTT, migration, multipath, and advanced BBR/pacing tuning (belongs in `TASK-0044`).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Works in OS constraints (`no_std` + `alloc` in core).
- PMTU ~1200 bytes: never send a UDP datagram bigger than 1200 bytes.
- Bounded memory:
  - cap retransmission queue,
  - cap in-flight bytes (congestion window),
  - cap reassembly buffer.
- Deterministic timers and timeouts.

## Concrete feature closure scope (TASK-0024)

All items below are required for `TASK-0024` to be honest/Done:

1. **Reliable data frames on OS QUIC-v2 path**
   - add explicit DATA + ACK semantics (not only handshake/ping).
   - packet-number based retransmit on timeout and/or duplicate-loss signal.
2. **Bounded recovery machinery**
   - hard caps for retransmit queue entries, in-flight bytes, and reorder buffer.
   - explicit reject/drop behavior when bounds are exceeded.
3. **Deterministic congestion/backpressure behavior**
   - conservative Reno-like controller (slow-start, CA, loss reaction) or equally simple bounded model.
   - deterministic backpressure propagation to mux path under pressure.
4. **Session liveness lifecycle**
   - keepalive/idle-timeout behavior with bounded reconnect attempts.
   - fail-closed transition on auth/session desync.
5. **Cross-path proof integrity**
   - host loss/recovery tests for transport logic.
   - OS QEMU proof that markers represent real transfer/recovery behavior.

## Protocol sketch (v1 follow-up)

### Handshake

- Reuse existing device identity + Noise (XK) semantics:
  - derive static Noise key from device identity (as in current host implementation),
  - authenticate peer device identity the same way as today (signature proof).
- After handshake, derive session keys and switch to encrypted packets.

### Packet format (post-handshake; concrete closure target)

- Keep the existing QUIC-v2 framing envelope from `TASK-0023` and extend with data-plane opcodes:
  - handshake ops (`MSG1`, `MSG2`, `MSG3`) stay unchanged,
  - add explicit `DATA`, `ACK`, and optional `WINDOW_UPDATE` ops,
  - keep authenticated framing and nonce/session correlation.
- Rekeying remains out of scope for v1.

### Reliability + ordering

- Provide one reliable ordered stream semantics for v1 (Mux-v2 unchanged above transport).
- Implement and prove:
  - retransmission on loss/timeout (bounded RTO policy),
  - in-order delivery with bounded reorder window,
  - bounded flow-control surface (transport or documented mux-coupled model).

### Congestion control

- Implement a conservative Reno-like cwnd with:
  - slow start,
  - congestion avoidance,
  - loss reaction (halve cwnd).

## Red flags / decision points

- **YELLOW**: entropy/RNG availability still gates strong ephemeral behavior; no "warn and continue".
- **YELLOW**: transport-vs-mux flow-control split must remain explicit to avoid hidden unbounded buffering.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport reliability/recovery/congestion logic)
- `userspace/net/nexus-net/` (UDP send/recv/bind API)
- `source/services/dsoftbusd/` (OS session datapath + markers)
- `source/services/netstackd/` (loopback UDP behavior/bounds where required)
- `source/apps/selftest-client/` (QUIC-v2 datapath probes and marker proofs)
- `tests/` (host lossy-link emulator tests)
- `docs/distributed/`
- `scripts/qemu-test.sh`

## Security considerations

### Threat model

- Spoofed or replayed UDP packets attempt session takeover.
- Loss/recovery abuse attempts memory pressure via retransmit or reassembly growth.
- Fallback confusion allows unauthenticated traffic to bypass secure path expectations.

### Security invariants (MUST hold)

- Session authentication must complete before DATA is accepted.
- Retransmit/reassembly/inflight buffers are strictly bounded.
- No silent downgrade marker or fake-success path.

### DON'T DO (explicit prohibitions)

- DON'T accept unauthenticated packets into stream state.
- DON'T allow unbounded retransmit/reassembly growth.
- DON'T mark QUIC-v2 transfer/recovery success without real data-plane behavior.

### Attack surface impact

- Significant: network-facing encrypted transport and recovery logic.

### Mitigations

- Nonce/packet-number validation, bounded queues, deterministic timeout policy, and fail-closed parser rejects.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- quic --nocapture`
  - `cargo test -p dsoftbusd -- reject --nocapture`
- Required tests:
  - `test_reject_replay_or_stale_packet_number`
  - `test_reject_unauthenticated_data_before_handshake`
  - `test_reject_oversize_datagram_or_reassembly_overflow`
  - `test_reject_quic_frame_bad_magic`
  - `test_reject_quic_frame_truncated_payload`
  - `test_reject_quic_frame_oversized_payload_encode`

### Hardening markers (QEMU, if applicable)

- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

## Stop conditions (Definition of Done)

### Proof (Host)

- Lossy-link tests (drop/reorder) complete handshake + data transfer + recovery.
- PMTU enforcement: no outbound datagram exceeds 1200 bytes.
- Congestion assertions:
  - under induced loss, cwnd decreases deterministically,
  - under clean path, cwnd grows deterministically.
- Bound assertions:
  - retransmit queue cap enforced,
  - in-flight cap enforced,
  - reorder/reassembly cap enforced.

### Proof (OS / QEMU)

Required floor:

- `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- required markers:
  - `dsoftbusd: transport selected quic`
  - `dsoftbusd: auth ok`
  - `dsoftbusd: os session ok`
  - `SELFTEST: quic session ok`
- additional TASK-0024 markers to add for real feature closure:
  - `SELFTEST: quic data transfer ok`
  - `SELFTEST: quic recovery ok`
  - `SELFTEST: quic backpressure ok`
- forbidden markers in QUIC-required profile:
  - `dsoftbusd: transport selected tcp`
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`

## Feature-to-proof matrix (concrete)

| Feature | Required proof |
| --- | --- |
| Data-plane DATA/ACK frames | host transport tests + `SELFTEST: quic data transfer ok` |
| Retransmit on loss/timeout | host lossy/reorder tests + `SELFTEST: quic recovery ok` |
| Bounded retransmit/inflight/reorder | requirement-named `test_reject_*` and explicit bound asserts |
| Congestion/backpressure behavior | host cwnd/backpressure tests + `SELFTEST: quic backpressure ok` |
| Marker honesty in OS profile | qemu harness requires QUIC markers and rejects fallback markers |

## Plan (small PRs)

1. **Data-plane closure**: implement DATA/ACK + retransmit core and requirement-named host tests.
2. **Boundedness closure**: enforce queue/window caps with deterministic reject/drop policy.
3. **OS proof closure**: extend selftest markers for transfer/recovery/backpressure and gate them in harness.
4. **Perf floor closure**: lock coarse deterministic budgets and sync docs/testing surfaces.

## Docs

Update `docs/distributed/dsoftbus-transport.md`:

- define concrete QUIC-v2 OS follow-up feature set (DATA/ACK/recovery/congestion bounds),
- PMTU/timeout/budget defaults,
- security caveats (RNG requirement, replay/reorder reject policy),
- explicit split: `TASK-0024` correctness/hardening vs `TASK-0044` advanced tuning.
