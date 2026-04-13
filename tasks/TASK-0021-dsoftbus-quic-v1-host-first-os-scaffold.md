---
title: TASK-0021 DSoftBus QUIC v1: host QUIC transport (quinn) + OS UDP scaffold (disabled) + TCP fallback
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0003
  - TASK-0005
  - TASK-0015
  - TASK-0020
  - TASK-0022
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC (modular daemon boundary): docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (modularization base): tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md
  - Depends-on (OS networking): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on (OS streams): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Depends-on (mux over transport): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (core split for OS backend): tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Testing methodology: docs/testing/index.md
  - Testing contract: scripts/qemu-test.sh
  - Testing contract (2-VM): tools/os2vm.sh
---

## Short description

- **Scope**: Add host-first QUIC transport selection (`auto|tcp|quic`) without destabilizing the default TCP path.
- **Deliver**: Host QUIC session proof, strict-mode downgrade rejects, and deterministic OS fallback markers while OS QUIC stays disabled-by-default.
- **Out of scope**: Enabling real OS QUIC by default or changing kernel/networking contracts.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock boundaries, security invariants, and explicit fallback semantics.
- **Phase B (Host proof)**: requirement-named host tests (including negative paths) are green.
- **Phase C (OS-gated proof)**: single-VM marker ladder is green; distributed proof is required where applicable.
- **Phase D (Performance gate)**: deterministic latency/throughput/backpressure budgets are defined and met.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: task-owned requirement suites (for transport selection and downgrade behavior).
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- 2-VM (if distributed behavior is asserted): `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

We want an optional QUIC/UDP transport alongside TCP for DSoftBus, without breaking the default
bring-up path. The best approach is:

- implement QUIC transport **host-first** (quinn),
- keep OS QUIC as a **disabled-by-default scaffold** until OS networking exists,
- provide a deterministic runtime selection with clean fallback to TCP.

## Goal

Prove:

- Host: QUIC session works and carries TASK-0020 mux traffic, including negative cases.
- OS/QEMU: default path remains green; OS reports deterministic “QUIC disabled → fallback TCP” markers.

## Target-state alignment (post TASK-0015 / RFC-0027)

- Transport selection (tcp/quic/auto) integrates through modular daemon seams (entry/session/netstack),
  not by re-growing `dsoftbusd/src/main.rs`.
- QUIC path and TCP fallback share the same session/gateway contract to keep follow-on tasks (`0016/0017/0020`)
  transport-agnostic.
- Marker/observability semantics remain deterministic and centralized.

## Non-Goals

- Enabling QUIC on OS by default (future step once nexus-net UDP exists and is stable).
- Datagram-mode protocols.
- Kernel changes.

## Constraints / invariants (hard requirements)

- **Kernel untouched**.
- **Default stays green**: QUIC must not destabilize existing TCP bring-up.
- **Deterministic selection**: `auto` tries QUIC then falls back; failures are logged/marked clearly.
- **No fake success**: do not emit “quic ok” markers unless QUIC was actually used.

## Security considerations

### Threat model

- Transport downgrade attacks that silently force weaker/legacy path.
- ALPN/certificate mismatch accepted accidentally in host QUIC path.
- Identity-binding drift between QUIC transport establishment and DSoftBus authenticated session semantics.

### Security invariants (MUST hold)

- `mode=quic` must fail closed when QUIC requirements are not met (no silent downgrade).
- `mode=auto` fallback to TCP must be explicit and auditable via deterministic markers.
- QUIC handshake validation (ALPN/cert expectations) must reject invalid peers deterministically.
- Sensitive operations remain gated on authenticated DSoftBus session semantics (transport alone is insufficient).

### DON'T DO

- DON'T emit QUIC success/fallback markers without corresponding real transport selection.
- DON'T treat ALPN/cert validation failures as warnings.
- DON'T couple auth identity to transport-local metadata only; preserve existing identity binding semantics.

### Required negative tests

- `test_reject_quic_wrong_alpn`
- `test_reject_quic_invalid_or_untrusted_cert`
- `test_reject_quic_strict_mode_downgrade`
- `test_auto_mode_fallback_marker_emitted`

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- quic --nocapture`
- Required tests:
  - `test_reject_quic_wrong_alpn`
  - `test_reject_quic_invalid_or_untrusted_cert`
  - `test_reject_quic_strict_mode_downgrade`

### Hardening markers (QEMU, if applicable)

- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`

## Red flags / decision points

- **RED**:
  - `userspace/dsoftbus` OS backend is currently a placeholder (`userspace/dsoftbus/src/os.rs`).
    OS QUIC must remain off by default and must not claim support until a reusable OS backend exists
    (TASK-0022) and UDP-sec/QUIC gating is in place.
- **YELLOW**:
  - Certificate/identity model: v1 can use ephemeral self-signed certs for host tests, but long-term should bind to device identity keys.
  - Async runtime: quinn is async; keep the host tests isolated and avoid pulling async into OS bring-up.

## Contract sources (single source of truth)

- DSoftBus traits: `userspace/dsoftbus`
- Mux v2 task: TASK-0020 (runs over any transport)
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host)

- Deterministic host tests:
  - QUIC connect + bidir stream + small echo (and/or mux smoke if available)
  - Negative: wrong ALPN / cert rejection path fails cleanly
  - Fallback: `auto` with QUIC unavailable selects TCP
  - strict-mode (`quic`) downgrade is rejected

### Proof (OS / QEMU)

- Default QEMU run passes and includes:
  - `dsoftbus: quic os disabled (fallback tcp)`
  - `SELFTEST: quic fallback ok`
  - and `dsoftbusd: transport selected tcp` (or equivalent selection marker)
- optional 2-VM validation uses `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- keep QEMU proofs sequential (single-VM then 2-VM)

Notes:

- Any postflight must only delegate to canonical harness/tests.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport abstraction + selection)
- `source/services/dsoftbusd/` (selection + discovery advertise)
- `tests/` (host QUIC tests)
- `source/apps/selftest-client/` (fallback marker)
- `docs/distributed/`
- `scripts/qemu-test.sh` (accept fallback markers)

## Plan (small PRs)

1. Add a transport abstraction layer (`TransportKind { Tcp, Quic }`) and runtime selection (`auto|tcp|quic`).
2. Implement host QUIC endpoint via quinn (single bidir stream per session).
3. Add OS QUIC scaffold behind a runtime flag; by default emit `quic os disabled` and fall back to TCP.
4. Integrate selection into dsoftbusd discovery payload and session establishment.
5. Add host tests + OS selftest markers (fallback path only until OS networking exists).
6. Docs: transport selection, security notes, and enablement plan.

## Follow-ups

- QUIC tuning (pacing/CC) + mux priorities load testing: see `TASK-0044`.
- **RPC Format Migration**: When QUIC transport is stable, migrate remote service calls from OS-lite byte frames to schema-based RPC (Cap'n Proto). See TASK-0005 "Technical Debt" section.

---

## Alignment with RFC-0007 Phase 3

This task implements the **QUIC transport** specified in RFC-0007 Phase 3:

> **Phase 3: QUIC + Noise Transport** — Replace custom UDP-sec with QUIC (IETF RFC 9000) for transport, using Noise XK for the handshake crypto.

**Key design decisions:**
- QUIC transport must preserve existing DSoftBus session/auth contracts and not bypass gateway/security checks.
- Mux semantics from TASK-0020 remain the logical stream contract; QUIC may optimize transport internals but must
  not silently change stream-level behavior.
- TCP fallback ensures bring-up path remains stable with explicit deterministic selection markers.
