---
schema: 1
title: "Network address matrix"
role: reference
subsystem: networking
sources: [source/services/netstackd/src/os/entry_pure.rs, source/services/netstackd/src/os/bootstrap.rs, source/services/dsoftbusd/src/os/entry_pure.rs, source/services/dsoftbusd/src/os/session/cross_vm.rs, tools/os2vm.sh]
contracts: [ADR-0026, ADR-0025]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Network address matrix

> **Purpose** — the one normative table of subnets, addresses, gateways and DNS targets used by runtime proofs.
> **Owns** — the address constants in `source/services/netstackd/src/os/entry_pure.rs` and `source/services/dsoftbusd/src/os/entry_pure.rs`
> **Contracts** — [ADR-0026](../adr/0026-network-address-profiles-and-validation.md) (address profiles and validation), [ADR-0025](../adr/0025-qemu-smoke-proof-gating.md) (smoke proof gating)
> **Invariants** — no address outside this table. A change updates the ADR, this table and the harness gates together. DNS proofs are protocol-semantic, never source-IP-pinned.
> **Read next** — [authority.md](authority.md), [dsoftbus.md](dsoftbus.md)

## Address Profiles

| Profile | Scope | Address source | Local address/cidr | Gateway | DNS target(s) | Primary consumers |
| --- | --- | --- | --- | --- | --- | --- |
| `qemu-smoke-dhcp` | 1-VM QEMU smoke | slirp DHCP lease | lease-provided (commonly `10.0.2.15/24`) | lease-provided (commonly `10.0.2.2`) | probe targets `10.0.2.3` and `10.0.2.2` | `netstackd` bootstrap proofs |
| `qemu-smoke-fallback` | 1-VM QEMU smoke when DHCP unavailable | deterministic static fallback | `10.0.2.15/24` | `10.0.2.2` | `10.0.2.3`, `10.0.2.2` | `netstackd`, loopback mode in single-VM proofs |
| `os2vm-static` | 2-VM socket/mcast harness | deterministic from NIC MAC LSB | `10.42.0.<lsb-or-1>/24` | none | n/a | `netstackd` (os2vm path) |
| `os2vm-node-roles` | 2-VM role mapping for session bootstrap | fixed role assignment | node-a `10.42.0.10`, node-b `10.42.0.11` | none | n/a | `dsoftbusd` cross-VM session orchestration |

## Validation Rules

1. No new subnet/range may be introduced without updating:
   - this matrix,
   - the relevant ADR,
   - runtime tests/harness gates.
2. DNS proof acceptance must be protocol-semantic, not source-IP-pinned:
   - source port `53`,
   - DNS response header bit (QR),
   - expected TXID for probe correlation.
3. Marker contracts must remain deterministic:
   - no random IDs in gating markers,
   - no success markers on fallback/error paths unless contract says so.

## Runtime Code Anchors

- `source/services/netstackd/src/os/entry_pure.rs`
  - fallback profile mapping
  - QEMU loopback target helper
  - DNS probe response semantics helper
- `source/services/netstackd/src/os/bootstrap.rs`
  - DHCP/fallback selection
  - DNS probe send/recv proof path
- `source/services/dsoftbusd/src/os/session/cross_vm.rs`
  - node-a/node-b cross-VM role mapping

## Test and Harness Anchors

- `source/services/netstackd/tests/p0_unit.rs`
  - profile mapping and DNS probe helper behavior
- `scripts/qemu-test.sh`
  - DHCP strict gate and DNS proof checks
- `tools/os2vm.sh`
  - cross-VM marker and typed failure classification
