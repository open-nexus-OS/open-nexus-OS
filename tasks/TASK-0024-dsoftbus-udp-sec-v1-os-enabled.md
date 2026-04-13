---
title: TASK-0024 DSoftBus UDP-sec v1 (OS enabled): Noise-over-UDP reliable stream + recovery + congestion + TCP fallback
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - ADR: docs/adr/0006-device-identity-architecture.md
  - Depends-on (DSoftBus core in OS): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Depends-on (OS networking UDP): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Build an OS-friendly secure UDP transport (Noise + reliability + congestion) under `no_std` constraints.
- **Deliver**: Reliable ordered stream abstraction for Mux v2, bounded recovery queues, deterministic timers, and TCP fallback.
- **Out of scope**: QUIC wire compatibility and kernel-side changes.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock packet/recovery/crypto invariants and bounded queue rules.
- **Phase B (Host proof)**: requirement-named host loss/recovery tests and negative tests are green.
- **Phase C (OS-gated proof)**: canonical OS marker ladder is green with explicit TCP fallback semantics.
- **Phase D (Performance gate)**: deterministic latency/throughput/recovery budgets are defined and met.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: task-owned requirement suites for reliability/recovery/congestion.
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- 2-VM distributed: `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

OS “real QUIC” is blocked by `no_std` feasibility (see TASK-0023). We still want the properties that
motivated QUIC in the first place:

- UDP path (no head-of-line blocking on packet loss),
- keepalive and path liveness,
- recovery (retransmission/ack),
- congestion control,
- PMTU discipline,
- a reliable ordered byte-stream abstraction for higher layers (Mux v2 unchanged).

This task implements an OS-friendly, no_std-capable **secure UDP transport** for DSoftBus that keeps
TCP as a deterministic fallback.

## Goal

In QEMU (single-VM first; cross-VM later), prove:

- UDP-sec transport establishes a session over `nexus-net` UDP,
- recovery works under moderate loss (host tests),
- Mux v2 runs unchanged over the UDP-sec connection,
- TCP fallback remains intact when UDP-sec disabled/unavailable.

## Non-Goals

- QUIC wire-compatibility.
- Datagram service APIs.
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

## Protocol sketch (v1)

### Handshake

- Reuse existing device identity + Noise (XK) semantics:
  - derive static Noise key from device identity (as in current host implementation),
  - authenticate peer device identity the same way as today (signature proof).
- After handshake, derive session keys and switch to encrypted packets.

### Packet format (post-handshake)

- Little-endian header:
  - `ver:u8=1`, `typ:u8`, `flags:u8`, `rsv:u8`
  - `session_id:u64`
  - `pn:u32` (packet number)
  - `ack:u32` + `ack_bits:u32` (simple ack + bitmap)
  - `len:u16`
- Payload is AEAD-encrypted; rekeying is out of scope for v1.

### Reliability + ordering

- Provide a single reliable ordered byte-stream (“conn”) for v1.
- Mux v2 runs over this byte-stream unchanged.
- Implement:
  - retransmission on loss/timeout (RTO),
  - in-order delivery with a small receive reorder window,
  - simple flow control (receiver credit / window update) or rely on mux v2 windows (documented).

### Congestion control

- Implement a conservative Reno-like cwnd with:
  - slow start,
  - congestion avoidance,
  - loss reaction (halve cwnd).

## Red flags / decision points

- **YELLOW**: Entropy / RNG availability in OS impacts Noise ephemeral keys. If RNG is weak/unavailable,
  we must block “real network security” and keep transport disabled by default.
- **YELLOW**: If we rely solely on mux flow control, we must ensure UDP-sec cannot buffer unboundedly;
  in-flight caps still required.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport/udp-sec implementation)
- `userspace/net/nexus-net/` (UDP send/recv/bind API)
- `source/services/dsoftbusd/` (transport selection + markers)
- `source/apps/selftest-client/` (UDP-sec markers + fallback)
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
- Transport fallback is explicit and auditable; no silent downgrade.

### DON'T DO (explicit prohibitions)

- DON'T accept unauthenticated packets into stream state.
- DON'T allow unbounded retransmit/reassembly growth.
- DON'T mark udp-sec success without real encrypted session behavior.

### Attack surface impact

- Significant: network-facing encrypted transport and recovery logic.

### Mitigations

- Nonce/packet-number validation, bounded queues, deterministic timeout policy, and fail-closed parser rejects.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- udp --nocapture`
- Required tests:
  - `test_reject_replay_or_stale_packet_number`
  - `test_reject_unauthenticated_data_before_handshake`
  - `test_reject_oversize_datagram_or_reassembly_overflow`

### Hardening markers (QEMU, if applicable)

- `dsoftbusd: auth ok`
- `SELFTEST: dsoftbus ping ok`

## Stop conditions (Definition of Done)

### Proof (Host)

- Lossy link emulator (drop 5–10% datagrams) completes handshake + transfers data.
- PMTU enforcement: assert no sent datagram > 1200 bytes.
- Congestion: under loss, cwnd decreases; under clean path, cwnd increases (coarse assertions).
- Fallback selection: `auto` chooses TCP if UDP-sec disabled/unavailable.

### Proof (OS / QEMU)

When UDP-sec is enabled and available:

- `dsoftbus: udp-sec os listener up <port>`
- `dsoftbus: udp-sec os session ok`
- `dsoftbusd: transport selected udp-sec`
- `dsoftbus:mux data ok (udp-sec)`
- `SELFTEST: udp-sec control ok`
- `SELFTEST: udp-sec bulk ok`
- `SELFTEST: udp-sec backpressure ok`

When UDP-sec is disabled/unavailable:

- `dsoftbus: udp-sec os disabled (fallback tcp)`
- `SELFTEST: udp-sec fallback ok`

## Plan (small PRs)

1. Land bounded handshake/packet format and recovery core with requirement-named host tests.
2. Integrate OS listener/session path with explicit fallback markers and negative-path coverage.
3. Close performance gates (loss/cwnd/backpressure budgets) and sync docs/harness expectations.

## Docs

Update `docs/distributed/dsoftbus-transport.md`:

- define `udp-sec` transport kind and selection policy (`auto|udp-sec|tcp`),
- PMTU/timeout defaults,
- security caveats (RNG requirement, key derivation).
