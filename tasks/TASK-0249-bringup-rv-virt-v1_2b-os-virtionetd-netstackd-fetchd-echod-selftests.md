---
title: TASK-0249 RISC-V Bring-up v1.2b (OS/QEMU): virtionetd + netstackd + fetchd + echod + TAP setup + selftests
status: Draft
owner: @kernel
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Bring-up core (host-first): tasks/TASK-0248-bringup-rv-virt-v1_2a-host-virtio-net-dhcp-stub-loopback-deterministic.md
  - Networking baseline (smoltcp): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Device MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need OS/QEMU integration for RISC-V Bring-up v1.2:

- `virtionetd` service (userspace virtio-net),
- `netstackd` service (DHCP stub + loopback),
- `fetchd` client smoke (HTTP-like over loopback),
- `echod` server (optional UDP echo),
- TAP setup/teardown.

The prompt proposes these services. `TASK-0003` already plans virtio-net + smoltcp for DSoftBus.

**Important extraction note (anti-drift):**

- `TASK-0003` now explicitly requires a **minimal networking ownership slice** (a `netstackd`-style owner that
  allows other services like `dsoftbusd` to use networking without owning MMIO).
- This task (`TASK-0249`) remains a **bring-up alternative/expansion** (TAP + DHCP stub + loopback + fetch/echo),
  and MUST NOT become an implicit prerequisite for completing `TASK-0003`.

## Goal

On OS/QEMU:

1. **DTB & runner updates**:
   - update `pkg://dts/virt-nexus.dts`: add `virtio_mmio@10002000` node for net (IRQ distinct from blk), `status = "okay"`
   - runner/QEMU: add TAP-backed NIC (deterministic, no external network): `-netdev tap,id=n0,script=no,ifname=${TAP_IF:-tap0} -device virtio-net-device,netdev=n0`
   - provide helper script `tools/net/setup-tap.sh` to create local, isolated tap0 bridged to nothing and pre-seed host echo daemon on tap interface; no root-persisting changes; teardown on exit
2. **virtionetd service** (`source/services/virtionetd/`):
   - implement virtio-mmio net frontend using library from `TASK-0248`
   - probe MMIO base/IRQ from DT
   - negotiate features: no multi-queue, no mergeable buffers, checksum offload off
   - TX/RX rings with fixed-size descriptors; RX pre-posted buffers from a slab
   - deterministic MAC (e.g., `02:00:00:00:00:01`) exposed in config
   - API (`net.capnp`): `info()` (name, mac, mtu, up), `send(frame)` (raw Ethernet), `recv()` (blocking with bounded wait)
   - markers: `virtionetd: ready`, `virtionetd: mmio=0x... irq=... mac=02:00:00:00:00:01 mtu=1500`, `virtionetd: rx frame len=...`
3. **netstackd service** (`source/services/netstackd/`):
   - loopback `lo` (127.0.0.1/8) with raw socket shim for `fetchd`
   - static IPv4 for `virt0` (from virtionetd) if DHCP disabled
   - DHCP stub: perform single-host deterministic lease by exchanging frames with built-in stub server when TAP is up; DISCOVER → OFFER/ACK with fixed lease (10.0.2.15/24, gw 10.0.2.2, dns 10.0.2.3)
   - simple ARP cache, minimal ICMP echo reply on `lo`
   - API (`net.capnp`): `ifUp(name)`, `ifDown(name)`, `dhcp(name)` → `addr`, `addrSet(name, addr)`, `addrGet(name)` → `addr`
   - markers: `netstackd: ready`, `netstackd: lo up`, `netstackd: virt0 dhcp addr=10.0.2.15`, `netstackd: recv arp who-has`
4. **echod service** (`source/services/echod/`):
   - optional UDP echo on `virt0` to validate RX/TX internally
   - marker: `echod: ready`, `echod: listen udp/7`
5. **fetchd service** (`source/services/fetchd/`):
   - minimal HTTP-like client over loopback: perform `GET /hello` from `echo://` shim or `http://127.0.0.1:8080/hello` and print body to log
   - integrate with `packagefs` to request `pkg://fixtures/hello.txt` via `fetchd` resolver for future webview plumbing
   - marker: `fetchd: ready`, `fetchd: get http://127.0.0.1:8080/hello ok bytes=...`
6. **nexus-init bring-up order**:
   - start in order: `virtionetd` → `netstackd` (`lo up`, DHCP virt0) → `echod` → `fetchd` smoke request → finish
   - markers for each step; bounded timeouts
7. **CLI diagnostics** (`tools/nx-net/`):
   - `nx net if`, `nx net addr virt0`, `nx net dhcp virt0`, `nx net sendraw --hex "..."`, `nx net ping 127.0.0.1 -c 2`, `nx net fetch http://127.0.0.1:8080/hello`
   - markers: `nx: net if n=2`, `nx: net fetch bytes=...`
8. **Host echo daemon** (`tools/net/echo-host.sh`):
   - binds to tap0 with tiny userland echo (UDP 7 or TCP 8080) returning deterministic payloads
9. **OS selftests + postflight**.

## Non-Goals

- Full smoltcp stack (handled by `TASK-0003`).
- Real DHCP server (in-process stub only).
- Multi-VM networking (single-VM only).

## Constraints / invariants (hard requirements)

- **No duplicate virtio-net authority**: `virtionetd` uses the library from `TASK-0248`. `TASK-0003` uses smoltcp for DSoftBus. If both coexist, they must share MMIO access and not conflict. Document the relationship explicitly.
- **No duplicate network stack authority**: this task's `netstackd`/loopback/DHCP stack is a bring-up alternative
  to the smoltcp-based system path in `TASK-0003`. If both exist, document which one is authoritative for the boot profile.
- **Determinism**: virtio-net operations, DHCP stub, and loopback must be stable given the same inputs.
- **Bounded resources**: DHCP stub is in-process only; loopback is bounded.
- **Device MMIO gating**: userspace virtio-net requires `TASK-0010` (device MMIO access model).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (virtio-net authority drift)**:
  - Do not create a parallel virtio-net service that conflicts with `TASK-0003` (smoltcp). If both coexist, they must share MMIO access and not conflict. Document the relationship explicitly.
- **RED (network stack authority drift)**:
  - Do not create a parallel network stack that conflicts with smoltcp (`TASK-0003`). `netstackd` is a lightweight alternative for bring-up. Document the relationship explicitly.
- **YELLOW (TAP setup)**:
  - TAP setup requires root or CAP_NET_ADMIN. Document the requirements explicitly and provide helper script.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Bring-up core: `TASK-0248`
- Networking baseline: `TASK-0003` (virtio-net + smoltcp)
- Device MMIO access: `TASK-0010` (prerequisite)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `virtionetd: ready`
- `virtionetd: mmio=0x... irq=... mac=02:00:00:00:00:01 mtu=1500`
- `netstackd: ready`
- `netstackd: lo up`
- `netstackd: virt0 dhcp addr=10.0.2.15`
- `echod: ready`
- `echod: listen udp/7`
- `fetchd: ready`
- `fetchd: get http://127.0.0.1:8080/hello ok bytes=...`
- `SELFTEST: net lo ping ok`
- `SELFTEST: net fetch echo ok`
- `SELFTEST: net udp echo ok`

### Docs gate (keep architecture entrypoints in sync)

- If init sequencing, networking owner boundaries, or marker ownership changes, update (or create):
  - `docs/architecture/06-boot-and-bringup.md` (boot/bring-up pipeline)
  - `docs/architecture/09-nexus-init.md` (bring-up orchestration order + init marker ownership)
  - `docs/architecture/07-contracts-map.md` (contract pointers for networking bring-up vs baseline tasks)
  - and the index `docs/architecture/README.md`

## Touched paths (allowlist)

- `pkg://dts/virt-nexus.dts` (extend: virtio-net device)
- `source/services/virtionetd/` (new)
- `source/services/netstackd/` (new)
- `source/services/echod/` (new)
- `source/services/fetchd/` (new)
- `source/init/nexus-init/` (extend: start net services + smoke sequence)
- `scripts/run-qemu-rv64.sh` (extend: TAP-backed NIC)
- `tools/net/setup-tap.sh` (new)
- `tools/net/echo-host.sh` (new)
- `tools/nx-net/` (new)
- `source/apps/selftest-client/` (markers)
- `docs/bringup/virt_net_v1_2.md` (new)
- `docs/network/fetchd.md` (new)
- `tools/postflight-bringup-rv_virt-v1_2.sh` (new)

## Plan (small PRs)

1. **DTB & runner updates**
   - DTB: virtio-net device
   - runner: TAP-backed NIC
   - TAP setup/teardown script

2. **virtionetd + netstackd**
   - virtionetd service
   - netstackd service (DHCP stub + loopback)
   - markers

3. **echod + fetchd + init wiring**
   - echod service
   - fetchd service
   - nexus-init bring-up order
   - markers

4. **CLI + selftests**
   - nx-net CLI
   - host echo daemon
   - OS selftests + postflight

## Acceptance criteria (behavioral)

- `virtionetd` probes & exchanges frames.
- `netstackd` brings up `lo` and deterministic DHCP on `virt0`.
- `fetchd` successfully retrieves from host echo.
- All three OS selftest markers are emitted.
