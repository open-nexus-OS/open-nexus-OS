---
title: TASK-0004 Networking step 2 (OS): DHCP + ICMP + DSoftBus dual-node + discovery-driven sessions + identity binding
status: Done
owner: @runtime
created: 2025-12-22
updated: 2026-01-10
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on: tasks/TASK-0003B-dsoftbus-noise-xk-os.md
  - Depends-on: tasks/TASK-0003C-dsoftbus-udp-discovery-os.md
  - RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
  - RFC: docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Networking step 1 (TASK-0003/3B/3C) establishes:

- ✅ userspace virtio-net + smoltcp
- ✅ Noise XK handshake (loopback)
- ✅ UDP discovery announce/receive (loopback scope)

**What's missing from TASK-0003x** (needs dual-node validation):

- Discovery-driven TCP connect (not hardcoded `127.0.0.1`)
- Identity binding enforcement (`device_id <-> noise_static_pub` verification)
- Session rejection on identity mismatch

Step 2 upgrades the network stack to be more realistic and self-configuring:

- DHCP instead of static IPv4
- correct ARP neighbor handling and ICMP echo
- subnet discovery for DSoftBus over multicast/broadcast
- **dual-node** proof inside one VM (two logical nodes with distinct device IDs/ports in a single process)
- **discovery-driven sessions**: TCP connect to peer IP/port from discovery, not hardcoded loopback
- **identity binding**: verify `device_id` from discovery matches authenticated `noise_static_pub` from handshake

## Goal

In QEMU, prove:

1. **DHCP**: lease acquisition configures the interface (IP/mask/gw) and networking continues to work.
2. **ICMP**: echo works and we can ping the gateway (`10.0.2.2` under QEMU usernet).
3. **DSoftBus dual-node mode**: two logical nodes (A/B) with distinct device IDs/ports discover each other.
4. **Discovery-driven TCP connect**: Node A connects to Node B using IP/port from UDP discovery (NOT hardcoded loopback).
5. **Identity binding enforcement**: After Noise handshake, verify that the authenticated `noise_static_pub` matches the `device_id` from discovery.
6. **Session rejection on mismatch**: If identity binding fails, session is rejected (no `auth ok` marker for that session).

This completes RFC-0007 Phase 1 (OS transport) and RFC-0008 Phase 1b (identity binding).

## Non-Goals

- Kernel unchanged (in this task): no kernel DHCP/ARP/ICMP stack work lands here. This step is blocked on
  userspace virtio-net availability from `TASK-0003` and its kernel prerequisite `TASK-0010`.
- Multi-VM OS↔OS networking (step 3).
- mDNS, QUIC, performance tuning, power tuning.
- Simulated/offline “DHCP” and “DNS” stubs (those are Network Basics v1: `TASK-0138`/`TASK-0139`).

## Constraints / invariants (hard requirements)

- **Kernel unchanged (in this task)**: no kernel edits land here; see gating notes above.
- **No fake success**: markers must only appear after real behavior.
- **Stubs are explicit**: stub paths must emit `stub`/`placeholder` markers or return deterministic `Unsupported/Placeholder` errors (never “ok/ready”).
- **Determinism**:
  - Marker strings are stable and non-random.
  - If discovery uses periodic announces with “jitter”, it must be deterministic (e.g. fixed schedule derived from device id),
    and must not affect marker semantics.
- **Security boundaries**: protocol/auth remains in userland; do not expand kernel networking surface.
- **No new unwrap/expect in OS daemons**; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (blocking / must decide now)**:
  - Step 2 cannot ship until step 1 (`TASK-0003`) is real in QEMU, which is gated on `TASK-0010` (MMIO access model).
- **YELLOW (risky / likely drift / needs follow-up)**:
  - **QEMU net backend variability**: multicast/broadcast behavior varies by backend. For this step we assume **QEMU usernet** (slirp) and a **single VM**.
    Deterministic rules:
    - QEMU bring-up uses deterministic UDP loopback (netstackd) for discovery and MUST emit `dsoftbusd: discovery up (udp loopback)`.
    - Real subnet multicast/broadcast discovery is a follow-on phase (TASK‑0005 / TASK‑0024).
    - If both multicast and broadcast are unsupported, emit a deterministic marker indicating *discovery transport unavailable* (no “ok”)
      and do **not** emit `dsoftbusd: dual-node session ok`.
    - If CI needs to accept a backend where discovery is unavailable, update `scripts/qemu-test.sh` explicitly (separate task), do not silently skip.
- **GREEN (confirmed assumptions)**:
  - Host `userspace/dsoftbus` tests remain the authoritative reference for Noise handshake + framing behavior.

## Security considerations

### Threat model

- **Spoofed discovery announcements**: Attacker sends fake peer info with wrong `device_id` or `noise_static_pub`
- **Identity confusion**: `device_id` does not match authenticated `noise_static_pub` → impersonation
- **Replay attacks**: Old discovery announcements re-sent to confuse peer selection
- **MITM during session establishment**: Attacker intercepts TCP connect and attempts handshake
- **Resource exhaustion**: Attacker floods discovery port with invalid packets → DoS

### Security invariants (MUST hold)

- `device_id` MUST be cryptographically bound to `noise_static_pub` before session is accepted
- Noise XK handshake MUST complete before any application data is exchanged
- Identity binding verification MUST occur after handshake, before `auth ok` marker
- Test keys MUST be explicitly labeled `// SECURITY: bring-up test keys, NOT production custody`
- Discovery announcements MUST be validated (version, length, format) before processing

### DON'T DO

- DON'T accept session if identity binding fails (no "warn and continue")
- DON'T log Noise private keys, session keys, or derived secrets
- DON'T skip identity verification even for loopback/localhost
- DON'T trust `device_id` strings without cryptographic binding to authenticated key
- DON'T use deterministic/test keys in production builds (enforce via `cfg`)

### Attack surface impact

- **Significant**: New multicast/broadcast listener on port `37020` (discovery)
- **Significant**: Dual-node mode increases code complexity and potential state confusion
- **Mitigation required**: Identity binding enforcement is the primary defense

### Mitigations

- Noise XK handshake provides mutual authentication and forward secrecy
- Identity binding (`device_id` ↔ `noise_static_pub`) verified post-handshake
- Bounded peer LRU prevents memory exhaustion from discovery floods
- Invalid announce packets ignored with deterministic error handling (no crash)
- Session rejected if identity mismatch → `dsoftbusd: identity mismatch peer=<id>`

## Security proof

### Audit tests (negative cases)

- Command(s):
  - `cargo test -p dsoftbus -- reject --nocapture`
- Required tests:
  - `test_reject_identity_mismatch` — wrong key → session rejected
  - `test_reject_malformed_announce` — invalid packet → ignored, no crash
  - `test_reject_oversized_announce` — bounded input enforced
  - `test_reject_replay_announce` — duplicate/old announces de-duplicated

### Hardening markers (QEMU)

- `dsoftbusd: identity mismatch peer=<id>` — binding enforcement works
- `dsoftbusd: announce ignored (malformed)` — parsing robust
- `dsoftbusd: auth ok` — only emitted after successful identity binding

### Fuzz coverage (optional)

- `cargo +nightly fuzz run fuzz_discovery_packet` — discovery packet parsing

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **DSoftBus contract**: `userspace/dsoftbus` traits + on-wire expectations (host backend is the reference)
- **Device access prerequisite**: `tasks/TASK-0010-device-mmio-access-model.md`

## Stop conditions (Definition of Done)

- **Proof (tests / host)**:
  - Command(s):
    - `cargo test -p dsoftbus -- --nocapture`
  - Required coverage (deterministic):
    - handshake happy path + ping/pong
    - auth-failure case
    - discovery de-dup and ignore invalid announces

- **Proof (QEMU)** (gated on `TASK-0003` and `TASK-0010`):
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required markers (must exist in `scripts/qemu-test.sh` expected list):
    - `net: dhcp bound <ip>/<mask> gw=<gw>`
    - `SELFTEST: icmp ping ok`
    - `dsoftbusd: discovery up (udp loopback)`
    - `dsoftbusd: session connect peer=<id>` ← **discovery-driven TCP connect (RFC-0007 GAP 2)**
    - `dsoftbusd: identity bound peer=<id>` ← **identity binding enforcement (RFC-0008 Phase 1b)**
    - `dsoftbusd: dual-node session ok`

Notes:

- Postflight scripts are not proof unless they only delegate to the canonical harness/tests and do not invent their own “OK”.

## Findings (2026-01-09) — status and gaps

This section records what was verified in-repo so we can iterate without drift.

### Verified (host)

- `just test-host` passes (workspace host suite).
- `cargo test -p dsoftbus -- --nocapture` passes, including `test_reject_identity_mismatch`.
- `cargo test -p dsoftbus -- reject --nocapture` is **green** and includes:
  - `test_reject_identity_mismatch` (hard `AuthError::Identity`)
  - `test_reject_malformed_announce`
  - `test_reject_oversized_announce`
  - `test_reject_replay_announce`

### Blocked / not yet proven (OS/QEMU)

- Canonical OS/QEMU proof `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` is now runnable and was executed successfully after fixing a kernel `deny(warnings)` bring-up issue (see notes below). Keep this section updated as markers evolve.
- Canonical OS/QEMU proof `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` is **green** and verifies the marker contract.
- **Quality note (no fake success)**:
  - OS discovery uses the canonical AnnounceV1 codec (`nexus-discovery-packet`) and a bounded LRU (`nexus-peer-lru`).
  - Dual-node mode now learns `node-b` via the *same discovery receive path* (no seeded/synthetic peer entry); connect target is taken from the discovered peer entry.
  - Real subnet peer learning across different QEMU/net backends (multicast join + broadcast fallback) remains a follow-on milestone (see TASK‑0005 / TASK‑0024).

### QEMU proof notes (2026-01-09)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` executed successfully after:
  - fixing `neuron` `deny(warnings)` build failure (targeted `dead_code` allowances for staged ASID/AS handle helpers), and
  - fixing netstackd loopback listener routing for dual-node ports (loop connect must target the correct loop listener by port).

## Touched paths (allowlist)

- `userspace/net/` (nexus-net smoltcp integration + DHCP/ICMP)
- `userspace/dsoftbus/` (OS backend + discovery logic; host tests)
- `source/services/dsoftbusd/` (dual-node mode wiring + markers)
- `source/apps/selftest-client/` (ICMP proof marker)
- `scripts/qemu-test.sh` (canonical marker contract update)
- `docs/` (update os-net + dsoftbus-os + testing)

## Plan (small PRs)

1. **DHCP client + neighbor cache maintenance**
   - Add DHCP client loop to `userspace/net/...` (smoltcp integration).
   - On lease acquire: configure iface (ip/mask/gw), reset neighbor cache.
   - Emit marker: `net: dhcp bound <ip>/<mask> gw=<gw>`.

2. **ICMP echo + ping helper**
   - Enable ICMP echo reply.
   - Provide a bounded `icmp_ping(addr, timeout)` helper (no busy loops; cooperative yield).

3. **Selftest: ICMP proof**
   - In `selftest-client`, ping gateway (QEMU default `10.0.2.2`).
   - Emit marker: `SELFTEST: icmp ping ok` on success; on failure, emit a clear error marker and abort the selftest tail.

4. **DSoftBus OS discovery (subnet)**
   - Replace fixed announce with multicast (`239.42.0.1:37020`) and fallback broadcast if multicast join fails.
   - Maintain a small bounded LRU of peers and debounce duplicates.
   - Ignore invalid announce packets deterministically (length/version checks).
   - Emit marker: `dsoftbusd: discovery up (udp loopback)` once sockets are bound and receive loop is active.

5. **Dual-node mode (single VM, one process)**
   - Add a runtime flag/env/config to run two logical nodes (A/B) inside one `dsoftbusd` process:
     - distinct device IDs (e.g. `node-a`, `node-b`)
     - distinct TCP ports (e.g. `34567`, `34568`)
     - distinct Noise static keys (port-based derivation already exists)
     - independent sockets / state machines
   - Prove A↔B discovery + TCP session + Noise handshake + ping/pong.
   - Emit marker: `dsoftbusd: dual-node session ok` only after the roundtrip completes.

6. **Discovery-driven TCP connect (RFC-0007 GAP 2)**
   - Replace hardcoded `127.0.0.1:port` with peer selection from PeerLru.
   - Flow: `dsoftbusd: discovery peer found device=node-b` → look up in LRU → TCP connect to `peer.addr:peer.port`.
   - Emit marker: `dsoftbusd: session connect peer=node-b` (peer ID from discovery, NOT hardcoded).
   - This proves that the session is truly discovery-driven.

7. **Identity binding enforcement (RFC-0008 Phase 1b)**
   - After Noise handshake completes, verify identity binding:
     - Extract `device_id` and `noise_static_pub` from the discovery announcement that led to this session.
     - Compare authenticated remote static key from Noise handshake with expected `noise_static_pub`.
     - If match: emit `dsoftbusd: identity bound peer=<id>` and proceed.
     - If mismatch: emit `dsoftbusd: identity mismatch peer=<id>` and reject session (no `auth ok`).
   - Deterministic mapping table: For bring-up, use port-based derivation (already exists).
   - For production (Phase 2): integrate with `keystored`/`identityd`.

8. **Docs**
   - Extend `docs/networking/os-net.md` with DHCP flow + neighbor cache + ICMP support.
   - Extend/introduce `docs/distributed/dsoftbus-os.md` with subnet discovery + dual-node mode + limits.
   - Extend `docs/testing/index.md` with how to run step-2 markers and troubleshoot DHCP in QEMU usernet.

## Acceptance criteria (behavioral)

- Host tests in `userspace/dsoftbus` cover discovery de-dup + handshake happy + auth-fail deterministically.
- OS/QEMU (after `TASK-0003` and `TASK-0010`) shows required DHCP/ICMP/DSoftBus discovery + dual-node markers and `scripts/qemu-test.sh` passes.
- This task lands no kernel changes; the virtio-net MMIO prerequisite remains `TASK-0010`.

## Evidence (to paste into PR)

- Host: `cargo test -p dsoftbus -- --nocapture` summary (include the new cases)
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` + a short `uart.log` tail with:
  - `net: dhcp bound ...`
  - `SELFTEST: icmp ping ok`
  - `dsoftbusd: discovery up (udp loopback)`
  - `dsoftbusd: dual-node session ok`

## RFC seeds (for later, once green)

- Decisions made:
  - DHCP state machine integration points + lease renewal policy.
  - Neighbor cache maintenance policy and bounds.
  - DSoftBus announce format/versioning + de-dup rules.
  - Dual-node test mode interface and determinism constraints.
- Open questions:
  - When to switch from single-process dual-node to multi-VM OS↔OS (step 3) in CI.
  - Multicast viability across different QEMU net backends; fallback policies.
