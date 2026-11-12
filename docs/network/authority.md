---
schema: 1
title: "Networking authority"
role: guide
subsystem: networking
sources: [source/services/netstackd, source/services/dsoftbusd, source/services/ingressd, source/apps/selftest-client/proof-manifest/profiles/harness.toml, tools/os2vm.sh]
contracts: [RFC-0006, RFC-0007, RFC-0092, ADR-0025, ADR-0026]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Networking authority

> **Purpose** — which service owns the network stack, which paths are canonical, and the rules that keep parallel implementations from drifting.
> **Owns** — `source/services/netstackd/`, `source/services/dsoftbusd/`, `source/services/ingressd/`
> **Contracts** — [RFC-0006](../rfcs/RFC-0006-userspace-networking-v1.md) (userspace networking, sockets facade), [RFC-0007](../rfcs/RFC-0007-dsoftbus-os-transport-v1.md) (DSoftBus OS transport), [RFC-0092](../rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md) (one inbound gateway), [ADR-0025](../adr/0025-qemu-smoke-proof-gating.md) (smoke proof gating), [ADR-0026](../adr/0026-network-address-profiles-and-validation.md) (address profiles)
> **Invariants** — one stack: netstackd owns the NIC and every socket. One inbound gateway: ingressd. Addresses are normative only in [address-matrix.md](address-matrix.md).
> **Read next** — [dsoftbus.md](dsoftbus.md), [address-matrix.md](address-matrix.md), [../security/network-policy.md](../security/network-policy.md)

## The canonical stack

| Service | Owns | Holds |
|---|---|---|
| `netstackd` | virtio-net MMIO, smoltcp, the IPC sockets facade (TCP, UDP, loopback, DNS probe); policy seam for `net.connect` / `net.bind` | `device.mmio.net`, `ipc.core`, `policy.delegate` |
| `dsoftbusd` | discovery, Noise XK sessions, mux, remote gateway — all through netstackd's facade, no MMIO | `ipc.core`, `statefs.read`, `statefs.write` |
| `ingressd` | the only non-loopback listener; exposure intents → loopback backends | `ipc.core`, `policy.delegate` |

No other path exists in the tree: there are no `net-canonical` / `net-bringup-lite` features and no `*-lite` network services.

The three stand on the network plane (`nexus_service_topology::planes::PLANE_NEEDS`): on a machine whose device tree names no network device init leaves them suspended and says so (`init: withheld svc=<name> (no net plane)`) — none of them starts and probes for a device that is not there.

## Authority rules

1. **One stack.** A new networking feature extends netstackd's facade or sits above it. It never opens a second NIC driver or socket layer.
2. **No raw NIC for apps.** `device.mmio.net` is the NIC. Granting it anywhere else bypasses every network policy check ([network-policy.md](../security/network-policy.md)).
3. **One inbound gateway.** Only ingressd may bind `address = "any"`; the policy grammar enforces it.
4. **Name collisions.** A future minimal bring-up network path (planned in `tasks/TASK-0248-bringup-rv-virt-v1_2a-host-virtio-net-dhcp-stub-loopback-deterministic.md` / `tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md`) uses a `-lite` suffix (`netstackd-lite`, `virtionetd-lite`). It is opt-in, chosen by the boot profile, and must not run beside the canonical stack.
5. **State the path.** Every networking task says which path it belongs to and links here.
6. **Addresses.** A subnet, IP, gateway or DNS target is normative only in [address-matrix.md](address-matrix.md). Changing one needs the ADR, the matrix and the harness gates in the same change.
7. **No two paths forever.** If a second path ever exists, it either replaces the canonical one by an explicit deprecation or stays opt-in with a stated use case.

## Proof lanes

| Lane | Recipe | Profile | Proves |
|---|---|---|---|
| single-VM smoke | `just test-os headless` | `headless` | `net: smoltcp iface up …` is the default "interface configured" proof; without DHCP netstackd falls back to `10.0.2.15/24` (slirp convention) |
| DHCP | `just ci-os-dhcp` / `just ci-os-dhcp-strict` | `dhcp` / `dhcp-strict` | DHCP lease + dependent L3/L4 proofs; `REQUIRE_QEMU_DHCP_STRICT=1` enforces them; icount off (real host I/O) |
| QUIC | `just ci-os-quic` | `quic-required` | DSoftBus markers required (`REQUIRE_DSOFTBUS=1`), TCP fallback markers forbidden |
| 2-VM | `just ci-os-os2vm` | `os2vm` (runner `tools/os2vm.sh`) | cross-VM discovery, sessions, remote packagefs + statefs; never depends on slirp DHCP |
| all | `just ci-network` | — | dhcp + quic + os2vm |
