---
title: TASK-0003 Networking step 1 (OS): virtio-net + smoltcp + DSoftBus local TCP/UDP
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Docs: docs/distributed/dsoftbus-lite.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want the first end-to-end OS networking milestone in **userspace**:

- a minimal virtio-net frontend feeding a smoltcp stack (static IPv4),
- a tiny sockets facade usable by OS services (TCP/UDP),
- and switching the OS DSoftBus backend from a placeholder to a working **local** transport
  (UDP discovery + TCP loopback sessions), with deterministic QEMU selftest markers.

This task is intentionally **Networking step 1**: single-VM local transport (loopback + local subnet
discovery). OS↔OS multi-VM sessions, DHCP, mDNS, QUIC, and perf are follow-up tasks.

Track alignment: virtio-net + `nexus-net` are “network driver” foundations and should remain aligned with
`tasks/TRACK-NETWORKING-DRIVERS.md` so DSoftBus transports can build on a stable, bounded sockets surface.

Repo reality today:

- Userspace virtio-net on QEMU `virt` requires a safe MMIO access model. That kernel prerequisite is tracked
  as `TASK-0010`. Until `TASK-0010` is complete, **OS/QEMU proof is blocked**, but host-side DSoftBus tests can
  still be used to keep protocol logic correct and deterministic.
- If we want to unblock DSoftBus app-layer flows without networking/MMIO, a deterministic offline localSim
  DSoftBus slice is tracked separately as `TASK-0157`/`TASK-0158`.

## Goal

Boot in QEMU and prove:

- OS userspace brings up a network interface and can send/receive UDP and TCP frames via smoltcp.
- `dsoftbusd` OS backend is functional (no `todo!/panic!/ENOTSUP`), emits honest markers, and can
  establish a local authenticated session and perform a ping/pong roundtrip.

## Non-Goals

- Kernel networking stack or kernel sockets.
- DHCP, DNS, mDNS, routing robustness, congestion control tuning.
- Multi-device discovery across multiple QEMU VMs (future step).
- Offline simulated network control-plane (netcfgd/dnsd/timesyncd) — tracked separately as `TASK-0138`/`TASK-0139`.
- Lightweight DHCP stub or loopback-only networking (handled by `TASK-0248`/`TASK-0249` as a bring-up alternative).

## Constraints / invariants (hard requirements)

- **Kernel unchanged (in this task)**: no kernel edits land in this task. Userspace virtio-net requires a safe
  MMIO capability/access path tracked as kernel work in `TASK-0010` (hard prerequisite).
- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened.
- **Stubs are explicit**: any remaining stub must emit `stub`/`placeholder` markers or return a
  deterministic `Unsupported/Placeholder` error (never “ok/ready”).
- **Determinism**: proof markers are stable strings; no timestamps/random bytes in markers.
- **Security boundaries**: no kernel networking stack; no parsers/crypto in kernel; protocol and auth live in userland.
- **Rust hygiene**: no new `unwrap/expect` in OS daemons; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (blocking / must decide now)**:
  - Userspace virtio-net requires MMIO access. If we do not already have a MMIO-cap/VMO/broker path,
    this task cannot be implemented. The required kernel work is tracked as `TASK-0010` (device MMIO access model).
- **YELLOW (risky / likely drift / needs follow-up)**:
  - The sockets facade must stay minimal/bounded; avoid turning step 1 into a full POSIX sockets surface.
- **GREEN (confirmed assumptions)**:
  - `userspace/dsoftbus` host backend already implements Noise XK handshake + stream framing we can reuse.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **DSoftBus contract**: `userspace/dsoftbus` public traits (`Discovery`, `Authenticator`, `Session`, `Stream`)
- **Device access prerequisite**: `tasks/TASK-0010-device-mmio-access-model.md`
- **Track alignment**: `tasks/TRACK-NETWORKING-DRIVERS.md`

## Stop conditions (Definition of Done)

- **Proof (tests / host)**:
  - Command(s):
    - `cargo test -p dsoftbus -- --nocapture`
  - Required coverage (deterministic):
    - handshake happy path + ping/pong
    - auth-failure case

- **Proof (QEMU)** (gated on `TASK-0010`):
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required markers (must exist in `scripts/qemu-test.sh` expected list):
    - `net: virtio-net up`
    - `net: smoltcp iface up 10.0.2.15`
    - `dsoftbusd: os transport up (udp+tcp)`
    - `dsoftbusd: os session ok`
    - `SELFTEST: net iface ok`
    - `SELFTEST: dsoftbus os connect ok`
    - `SELFTEST: dsoftbus ping ok`

Notes:

- Postflight scripts are not proof unless they only delegate to the canonical harness/tests and do not invent their own “OK”.

## Touched paths (allowlist)

- `source/drivers/net/virtio/` (reuse/extend the existing driver crate if it fits)
- `userspace/net/` (virtio-net adapter + smoltcp integration + minimal sockets facade)
- `userspace/dsoftbus/` (implement OS backend; keep host backend stable)
- `source/services/dsoftbusd/` (OS entrypoint wiring + markers; ensure OS build works)
- `source/apps/selftest-client/` (add OS markers + local DSoftBus roundtrip)
- `scripts/qemu-test.sh` (canonical marker contract update)
- `docs/` (networking + dsoftbus-os notes)

## Plan (small PRs)

1. **Unblock feasibility (gated on TASK-0010)**
   - Confirm userspace can map virtio-net MMIO safely (capability/VMO/broker per `TASK-0010`).

2. **VirtIO net frontend (userspace)**
   - Implement a virtio-net device driver usable from userspace.
   - Prefer reusing `source/drivers/net/virtio` as the low-level component and add a thin adapter
     that implements smoltcp `phy::Device` (Rx/Tx token model).
   - Emit marker: `net: virtio-net up (mtu=..., mac=...)`.

3. **Smoltcp integration + minimal sockets facade**
   - Static IPv4: `10.0.2.15/24`, gateway `10.0.2.2` (QEMU usernet defaults).
   - Provide a minimal TCP/UDP facade that services can use without embedding smoltcp types.
   - Emit marker: `net: smoltcp iface up 10.0.2.15`.

4. **DSoftBus OS backend**
   - Replace the OS placeholder (`todo!/panic`) with a real OS backend using the sockets facade:
     - Discovery: UDP multicast (e.g. `239.10.0.1:37020`) announce packet (deviceId, port, noise static).
     - Sessions: TCP loopback milestone first (connect to self), then generalize.
     - Auth: reuse Noise XK handshake and identity checks from host backend.
   - Markers:
     - `dsoftbusd: os transport up (udp+tcp)`
     - `dsoftbusd: os session ok`

5. **Selftest markers**
   - Add bounded, non-busy-wait selftest steps (use cooperative yield):
     - `SELFTEST: net iface ok`
     - `SELFTEST: dsoftbus os connect ok`
     - `SELFTEST: dsoftbus ping ok`

6. **Docs**
   - `docs/networking/os-net.md`: virtio-net frontend overview, static config, polling model, limits.
   - `docs/distributed/dsoftbus-os.md`: OS backend design and local milestone scope.
   - `docs/testing/index.md`: how to run host tests + QEMU marker suite.

## Acceptance criteria (behavioral)

- Host: `cargo test -p dsoftbus` covers happy path + auth-fail deterministically.
- OS/QEMU (after `TASK-0010`): `scripts/qemu-test.sh` passes and includes the networking + dsoftbus markers listed above.
- This task lands no kernel changes; `TASK-0010` is the required prerequisite.

## Evidence (to paste into PR)

- Host: `cargo test -p dsoftbus -- --nocapture` summary
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` and a short `uart.log` tail showing the new markers.

## RFC seeds (for later, once green)

- Decisions made:
  - Userspace virtio-net frontend model (polling vs interrupts, buffer strategy).
  - OS sockets facade surface and error mapping.
  - DSoftBus OS transport scope (UDP discovery + TCP session + Noise XK reuse).
- Open questions:
  - How to expose MMIO devices to userspace in a capability-safe way (if not already present).
  - Multi-VM discovery/session model (future step).
