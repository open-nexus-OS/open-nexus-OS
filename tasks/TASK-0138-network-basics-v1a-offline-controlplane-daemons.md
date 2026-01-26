---
title: TASK-0138 Network Basics v1a (offline): netcfgd + sim-dhcpcd + dnsd (hosts+cache) + timesyncd (local offset) + host tests
status: Draft
owner: @platform
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Real OS networking (virtio-net/smoltcp): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Real DHCP/ICMP (on-device): tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md
  - Config system (future defaults): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy gates (network.configure): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

Real networking milestones (`TASK-0003`/`TASK-0004`) are blocked on a userspace device/MMIO access model.
We still need an **offline, deterministic** networking slice for QEMU and host tests so UI and platform
code can depend on stable “network state” semantics without sending any external traffic.

This task defines a **control-plane simulation** only:

- no packets are sent,
- DNS resolves from local hosts + cache only,
- time sync provides a deterministic offset model (no external NTP).

## Goal

Deliver:

1. `netcfgd` (interface manager for a single logical interface `simnet0`):
   - modes: `offline`, `airplane`, `static`, `simDhcp`
   - stores current iface config and exposes it via a small IDL
   - in `simDhcp` mode, requests a simulated lease from `dhcpcd` and applies it
   - markers:
     - `netcfgd: ready`
     - `net: mode=<...>`
     - `net: iface up name=simnet0 addr=...`
2. `dhcpcd` (simulated, deterministic):
   - returns a fixed lease for `simnet0` and calls `netcfgd.applyLease`
   - never sends packets
   - markers:
     - `dhcpcd: ready`
     - `dhcp: lease 10.0.0.10/24 gw=10.0.0.1`
3. `dnsd` (hosts + cache only):
   - resolves from:
     - `pkg://net/hosts.json` (fixtures)
     - optional `state:/net/hosts.nxs` overrides (Cap'n Proto snapshot; canonical) (gated until `/state` exists)
   - cache with negative caching (deterministic TTL using injectable clock)
   - **never** performs upstream lookups
   - markers:
     - `dnsd: ready`
     - `dns: resolve q=... rc=Ok ip=...`
     - `dns: resolve q=... rc=NxDomain`
4. `timesyncd` (deterministic, offline time model):
   - reads a deterministic seed epoch (e.g. `pkg://net/time.seed`)
   - computes and exposes a bounded offset relative to a userspace “wallclock shim”
   - **does not** require a kernel “set time” syscall; consumers can use `timesyncd` as a time source
   - markers:
     - `timesyncd: ready`
     - `timesync: sync offsetUs=...`
5. Host tests proving deterministic behavior for all of the above.

## Non-Goals

- Kernel changes.
- Real packet I/O, routing, ARP/ICMP, or upstream DNS.
- Claiming “system time was set” without a real kernel time API.

## Constraints / invariants (hard requirements)

- Offline-by-design: no sockets opened for upstream networking, no broadcast/mcast.
- Deterministic behavior: injectable clock in tests; stable markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/network_basics_v1_host/`:

- sim DHCP lease request/renew stable
- netcfgd mode transitions stable and reflected in `list()`
- dnsd:
  - hosts resolve OK
  - cache hit/miss stats deterministic
  - negative caching TTL deterministic
- timesyncd:
  - syncNow produces bounded offset and getLast returns the same value deterministically

## Touched paths (allowlist)

- `source/services/netcfgd/` (new)
- `source/services/dhcpcd/` (new, sim)
- `source/services/dnsd/` (new)
- `source/services/timesyncd/` (new)
- `tools/nexus-idl/schemas/{netcfg,dhcp,dns,time}.capnp` (new)
- `tests/network_basics_v1_host/` (new)
- `docs/network/` (added in follow-up task)

## Plan (small PRs)

1. Add IDL schemas + netcfgd core + markers
2. Add sim dhcpcd + applyLease wiring + markers
3. Add dnsd hosts+cache (injectable clock) + markers
4. Add timesyncd local-offset model + markers
5. Host tests
