---
title: TASK-0023 DSoftBus QUIC v2 (OS enabled): UDP over nexus-net + handshake + loss/congestion (gated)
status: Blocked
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

## Red flags / decision points

- **RED (feasibility)**:
  - OS userland is `no_std`. `quinn`/`rustls`/`quinn-proto` suitability for `no_std` must be proven up-front.
  - This has been decided: **OS QUIC remains disabled until proven feasible; OS-secure UDP becomes the v2 transport plan.**
- **YELLOW (identity binding)**:
  - Device identity keys in OS depend on keystore persistence and entropy. If keystored RNG isn’t available in OS builds, certificate issuance must be deferred.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (transport/quic os endpoint)
- `userspace/net/nexus-net/` (UDP sockets support, if needed)
- `source/services/dsoftbusd/` (selection + markers)
- `source/apps/selftest-client/` (QUIC markers / fallback markers)
- `tests/` (host lossy-link tests)
- `docs/distributed/`
- `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host) — feasibility spike only

- Prove (via `cargo test` on a dedicated spike crate) whether the selected QUIC stack can build for OS constraints:
  - `no_std` viability (or a clearly isolated `std` boundary),
  - deterministic timers without OS async runtime assumptions,
  - crypto dependencies and their entropy requirements.

### Proof (OS / QEMU)

- Not applicable while status is **Blocked**. Use `TASK-0024` for OS/QEMU proof of a UDP-based transport.

## Docs

Update `docs/distributed/dsoftbus-transport.md`:

- OS QUIC: clearly marked as **future** and blocked on feasibility
- OS UDP-sec transport: documented in `TASK-0024`
