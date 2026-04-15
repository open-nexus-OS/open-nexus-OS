---
title: TASK-0023 DSoftBus QUIC v2 (OS enabled): UDP over nexus-net + handshake + loss/congestion (gated)
status: In Progress
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0020
  - TASK-0022
follow-up-tasks:
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

- **Scope**: Track OS QUIC enablement decision and feasibility boundaries.
- **Deliver**: Explicit gate: OS QUIC remains blocked until `no_std` feasibility is proven; route implementation to UDP-sec path.
- **Out of scope**: Shipping half-enabled OS QUIC with fake-success markers.
- **Execution mode**: `In Progress` here means contract/gate closure work is active; it does **not** mean OS QUIC is already enabled.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock feasibility criteria and explicit block conditions for OS QUIC.
- **Phase B (Host proof)**: feasibility and reject-path suites are requirement-named and green.
- **Phase C (OS-gated proof)**: only real OS QUIC behavior may unlock marker claims; otherwise remain explicitly gated.
- **Phase D (Performance gate)**: deterministic transport budgets are required before production claims.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host feasibility: task-owned requirement suites for no_std/runtime viability.
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- 2-VM (if distributed behavior is asserted): `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

We already have a host-first QUIC v1 plan (TASK-0021) with OS disabled-by-default scaffolding.
This task turns the OS QUIC path into a real, tested transport.

## Decision (explicit)

**Decision: block OS QUIC v2 until no_std feasibility is proven.**

Rationale:

- OS userland is `no_std`, while the current QUIC ecosystem (`quinn` + `rustls`) is typically `std`-centric.
- Shipping “half QUIC” would create drift, fake-success markers, and a large maintenance burden.

**Instead, we will implement an OS-secure UDP transport (Noise+recovery) as the practical path** and keep host QUIC as-is.
That work is tracked in `TASK-0024` (created separately) and still runs **Mux v2 unchanged** over a reliable stream abstraction.

## Goal

In QEMU, with QUIC enabled:

- OS can establish a QUIC session over UDP (nexus-net),
- DSoftBus can run mux v2 over the QUIC connection (mux unchanged),
- loss/retransmission + congestion control behave correctly under moderate loss,
- TCP fallback remains intact and deterministic when QUIC is disabled.

## Non-Goals

- Perfect performance tuning (BBR, pacing, advanced ECN).
- 0-RTT.
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Default stays green: QUIC is opt-in (`DSOFTBUS_TRANSPORT=quic|auto`), and `tcp` remains the fallback.
- Bounded memory and deterministic timers.
- Do not fragment: enforce PMTU ~1200 bytes; chunk at higher layers.
- Rust discipline for follow-up implementation:
  - use `newtype` wrappers for mode/session/domain IDs where safety-relevant,
  - use `#[must_use]` for decision-bearing return values,
  - keep ownership transfer explicit across transport/session boundaries,
  - review `Send`/`Sync` expectations via compile-time assertions (no unsafe blanket trait shortcuts).

## Red flags / decision points

- **RED (feasibility, resolved as explicit gate outcome)**:
  - OS userland is `no_std`; `quinn`/`rustls`/`quinn-proto` suitability for this environment is not yet production-proven.
  - Gate outcome is locked: **OS QUIC remains disabled until feasibility evidence exists; OS-secure UDP is executed in `TASK-0024`.**
- **YELLOW (identity binding, carried to follow-up implementation)**:
  - Device identity keys in OS depend on keystore persistence and entropy; if keystored RNG is unavailable, certificate issuance remains blocked fail-closed.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport/quic os endpoint)
- `userspace/net/nexus-net/` (UDP sockets support, if needed)
- `source/services/dsoftbusd/` (selection + markers)
- `source/apps/selftest-client/` (QUIC markers / fallback markers)
- `tests/` (host lossy-link tests)
- `docs/distributed/`
- `scripts/qemu-test.sh`

## Security considerations

### Threat model

- Silent downgrade from requested QUIC mode to weaker transport without explicit evidence.
- Acceptance of invalid peer identity/cert material in QUIC handshake path.
- Resource exhaustion via malformed or oversized transport frames/timers.

### Security invariants (MUST hold)

- QUIC mode claims are fail-closed; no implicit success when gated/off.
- Authentication and identity binding complete before application data is processed.
- All untrusted frame/input sizes are bounded and rejected deterministically.

### DON'T DO (explicit prohibitions)

- DON'T emit QUIC success markers while the task remains gated.
- DON'T treat handshake/cert validation failures as warnings.
- DON'T bypass policy checks for convenience in test mode.

### Attack surface impact

- Significant: transport/auth/session boundary handling in distributed paths.

### Mitigations

- Explicit gating markers, strict reject paths, bounded queues/timers, and requirement-named negative tests.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- quic --nocapture`
- Required tests:
  - `test_reject_quic_strict_mode_downgrade`
  - `test_reject_quic_invalid_or_untrusted_cert`
  - `test_reject_quic_wrong_alpn`

### Hardening markers (QEMU, if applicable)

- `dsoftbus: quic os disabled (fallback tcp)` (while gated)
- `SELFTEST: quic fallback ok`

## Stop conditions (Definition of Done)

### Proof (Host) — gate integrity + feasibility checkpoint

- Keep gate integrity and strict-fail behavior green on host:
  - `cargo test -p dsoftbus --test quic_selection_contract -- --nocapture`
  - `cargo test -p dsoftbus --test quic_host_transport_contract -- --nocapture`
  - `cargo test -p dsoftbus -- quic --nocapture`
- If feasibility is re-opened, prove (via `cargo test` on a dedicated spike crate) whether the selected QUIC stack can build for OS constraints:
  - `no_std` viability (or a clearly isolated `std` boundary),
  - deterministic timers without OS async runtime assumptions,
  - crypto dependencies and their entropy requirements.

### Proof (OS / QEMU)

- While the feasibility gate remains **Blocked**, required proof is the explicit fallback marker contract only:
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`
- Real OS transport proof lives in `TASK-0024` (UDP-sec path) until feasibility changes.

## Slice plan (small PRs)

1. **Slice 1 (gate-contract closure)**: lock feasibility criteria, blocked-state semantics, and routing ownership (`TASK-0024`, `TASK-0044`).
2. **Slice 2 (security/reject floor)**: keep requirement-named strict-mode/auth reject suites green and aligned to task/RFC.
3. **Slice 3 (runtime boundary evidence)**: preserve deterministic fallback marker contract and sync docs/board/handoff without pre-enabling OS QUIC.

## Baseline evidence refresh (2026-04-15)

- Host gate baseline (green):
  - `just test-dsoftbus-quic`
- OS blocked-state marker baseline (green):
  - `REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - observed markers:
    - `dsoftbus: quic os disabled (fallback tcp)`
    - `SELFTEST: quic fallback ok`

## Docs

Update `docs/distributed/dsoftbus-lite.md`:

- OS QUIC: clearly marked as **future** and blocked on feasibility
- OS UDP-sec transport: documented in `TASK-0024`
