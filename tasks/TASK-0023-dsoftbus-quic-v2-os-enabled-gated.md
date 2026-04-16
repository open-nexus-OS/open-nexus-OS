---
title: TASK-0023 DSoftBus QUIC v2 (OS enabled): UDP over nexus-net + handshake + loss/congestion (gated)
status: Done
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0020
  - TASK-0022
follow-up-tasks:
  - TASK-0023B
  - TASK-0024
  - TASK-0044
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed: docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - ADR: docs/adr/0006-device-identity-architecture.md
  - Depends-on (DSoftBus core in OS): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Depends-on (OS networking UDP): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: implement and verify real OS QUIC v2 session behavior on the no_std OS path.
- **Deliver**: UDP datagram QUIC-v2 framing + Noise XK auth + mux-v2 continuity, with deterministic QEMU evidence.
- **Out of scope**: full IETF QUIC/TLS feature parity (0-RTT, advanced congestion tuning, pacing/BBR).
- **Execution mode**: `Done` means the OS QUIC session path is implemented and proven at the task floor and broader regression/harness verification is green.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without linked proof evidence.

- **Phase A (Contract lock)**: lock enablement semantics and invariants.
- **Phase B (Host proof)**: fail-closed host reject suites and host QUIC contracts stay green.
- **Phase C (OS proof)**: OS path emits QUIC success markers only after real handshake/session behavior.
- **Phase D (Performance/feasibility floor)**: deterministic bounds and reject-path contracts remain explicit.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state synced to measured behavior.

Current phase snapshot (implementation refresh, 2026-04-16):

- **Phase A**: ✅
- **Phase B**: ✅
- **Phase C**: ✅ (real OS QUIC session markers observed in QEMU)
- **Phase D**: ✅ (feasibility/reject budget contracts green)
- **Phase E**: ✅ (proof + status surfaces synchronized)

Canonical commands:

- Host QUIC floor: `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
- Host + service floor: `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture`
- OS marker floor: `cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- OS hygiene: `cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os`

## Context

`TASK-0021` delivered host-first QUIC and OS scaffold.
This task completes the OS-side transport behavior with real QUIC-v2 session evidence.

## Decision (explicit)

**Decision: enable OS QUIC v2 session path using no_std-friendly UDP datagram framing + Noise XK.**

Rationale:

- OS remains `no_std`; the chosen path avoids hidden runtime/std coupling.
- Marker honesty is preserved: success markers only after real handshake + ping/pong + mux checks.
- Host QUIC (`quinn`/`rustls`) remains the host contract floor; OS path uses explicit no_std transport framing.

## Goal

In QEMU:

- OS establishes authenticated QUIC-v2 sessions over UDP facade,
- DSoftBus runs mux v2 over the authenticated session path unchanged,
- strict reject behavior remains deterministic for malformed/untrusted input classes,
- harness rejects fallback markers in QUIC-required profile.

## Non-Goals

- full IETF QUIC stack parity in OS path,
- advanced congestion tuning (BBR/pacing/ECN optimization),
- kernel/MMIO contract changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Deterministic/bounded loops, payload sizes, and retry surfaces.
- QUIC success markers only after real auth/session progression.
- No silent downgrade to fallback markers in QUIC-required QEMU profile.
- Rust discipline:
  - `#[must_use]` for decision-bearing returns,
  - explicit ownership transfer across transport/session boundaries,
  - `Send`/`Sync` expectations validated via compile-time assertions/tests.

## Security considerations

### Threat model

- Silent downgrade from requested QUIC behavior.
- Invalid identity/cert assumptions being accepted.
- Malformed/oversized frames causing unsafe parser/resource behavior.

### Security invariants (MUST hold)

- Authentication and identity binding complete before session success claims.
- Untrusted frame boundaries are strictly validated and bounded.
- Reject paths remain deterministic (no warn-and-continue semantics).

### DON'T DO

- DON'T emit fallback markers in QUIC-required profile.
- DON'T treat auth/identity failures as warnings.
- DON'T bypass policy checks for convenience.

## Security proof

### Audit tests (negative cases)

- `cargo test -p dsoftbus -- quic --nocapture`
- `cargo test -p dsoftbusd -- reject --nocapture`
- `cargo test -p dsoftbusd --test p0_unit -- --nocapture` (includes QUIC frame reject-paths)

### Hardening markers (QEMU)

- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

## Stop conditions (Definition of Done)

### Proof (Host)

- `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
- `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
- `cargo test -p dsoftbus --test quic_feasibility_contract -- --nocapture`
- `cargo test -p dsoftbusd -- --nocapture`

### Proof (OS / QEMU)

- `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
- Required markers:
  - `dsoftbusd: transport selected quic`
  - `dsoftbusd: auth ok`
  - `dsoftbusd: os session ok`
  - `SELFTEST: quic session ok`
- Forbidden markers in this profile:
  - `dsoftbusd: transport selected tcp`
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`

### Hygiene gates

- `just dep-gate`
- `just diag-os`

## Slice summary

1. Replace fallback-only marker path with real QUIC-v2 UDP session path in `dsoftbusd`.
2. Switch selftest-client probe to real UDP/QUIC-v2 handshake + ping/pong.
3. Harden netstackd loopback UDP routing for multi-port datagram exchange.
4. Tighten QEMU harness to require QUIC markers and fail on fallback markers.
5. Add QUIC frame reject-path unit tests (`dsoftbusd/tests/p0_unit.rs`).

## Baseline evidence refresh (2026-04-16)

- Host floors green:
  - `just test-dsoftbus-quic`
  - `cargo test -p dsoftbusd -- --nocapture`
- OS floor green:
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`
  - observed markers:
    - `dsoftbusd: transport selected quic`
    - `dsoftbusd: auth ok`
    - `dsoftbusd: os session ok`
    - `SELFTEST: quic session ok`
  - fallback markers absent in QUIC-required profile.

## Docs

Update all QUIC status surfaces to reflect enabled OS session path and marker contract.
