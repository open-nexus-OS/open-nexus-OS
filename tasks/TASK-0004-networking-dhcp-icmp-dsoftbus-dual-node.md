---
title: TASK-0004 Networking step 2 (OS): DHCP + ARP/ICMP + DSoftBus subnet discovery + dual-node (single VM)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Depends-on: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Networking step 1 establishes userspace virtio-net + smoltcp + a working OS DSoftBus local transport.
Step 2 upgrades the network stack to be more realistic and self-configuring:

- DHCP instead of static IPv4
- correct ARP neighbor handling and ICMP echo
- subnet discovery for DSoftBus over multicast/broadcast
- and a **dual-node** proof inside one VM (two logical nodes with distinct device IDs/ports in a single process),
  so CI can validate discovery + handshake + ping without spinning a second QEMU VM.

## Goal

In QEMU, prove:

- DHCP lease acquisition configures the interface (IP/mask/gw) and networking continues to work.
- ICMP echo works and we can ping the gateway (`10.0.2.2` under QEMU usernet).
- DSoftBus OS discovery runs over multicast/broadcast and dual-node mode establishes a Noise-authenticated session and ping/pong.

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
    - If multicast join/bind is unsupported, fall back to broadcast and still allow/require `dsoftbusd: discovery up (mcast/bcast)`.
    - If both multicast and broadcast are unsupported, emit a deterministic marker indicating *discovery transport unavailable* (no “ok”)
      and do **not** emit `dsoftbusd: dual-node session ok`.
    - If CI needs to accept a backend where discovery is unavailable, update `scripts/qemu-test.sh` explicitly (separate task), do not silently skip.
- **GREEN (confirmed assumptions)**:
  - Host `userspace/dsoftbus` tests remain the authoritative reference for Noise handshake + framing behavior.

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
    - `dsoftbusd: discovery up (mcast/bcast)`
    - `dsoftbusd: dual-node session ok`

Notes:

- Postflight scripts are not proof unless they only delegate to the canonical harness/tests and do not invent their own “OK”.

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
   - Emit marker: `dsoftbusd: discovery up (mcast/bcast)` once sockets are bound and receive loop is active.

5. **Dual-node mode (single VM, one process)**
   - Add a runtime flag/env/config to run two logical nodes (A/B) inside one `dsoftbusd` process:
     - distinct device IDs
     - distinct TCP ports
     - independent sockets / state machines
   - Prove A↔B discovery + TCP session + Noise handshake + ping/pong.
   - Emit marker: `dsoftbusd: dual-node session ok` only after the roundtrip completes.

6. **Docs**
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
  - `dsoftbusd: discovery up (mcast/bcast)`
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
