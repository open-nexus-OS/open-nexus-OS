---
title: RFC-0010 DSoftBus cross-VM harness v1 (2-VM opt-in, real subnet discovery)
status: Accepted
owners: runtime
created: 2026-01-12
updated: 2026-01-13
audience: networking, dsoftbus, qa
links:
  - Tasks: tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Depends-on: docs/rfcs/RFC-0006-userspace-networking-v1.md
  - Depends-on: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
  - Depends-on: docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
  - Depends-on: docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md
---

## Status at a Glance

- Phase: Accepted (implemented as part of TASK-0005)
- Scope: Opt-in 2-VM proof harness + cross-VM discovery/session + remote proxy markers
- Out of scope: Transport redesign (UDP-sec/QUIC), capability transfer, kernel changes

## Problem Statement

We need a deterministic, rootless 2-VM harness to prove real subnet discovery and Noise-authenticated DSoftBus sessions between two OS VMs, plus a minimal remote proxy (samgrd/bundlemgrd) with honest markers. Current RFC-0007/0008 cover loopback and single-VM bring-up; they do not define cross-VM networking backend, discovery fallback, or harness contract.

## Goals

- Define the canonical 2-VM harness entrypoint and required markers.
- Pin the networking backend choice and discovery semantics for cross-VM runs.
- Constrain remote proxy exposure (allowlist, bounds) and proof expectations.
- Keep single-VM default behavior unchanged.

## Non-Goals

- No kernel changes.
- No new transport (UDP-sec/QUIC) or mux v2 flow-control decisions (covered by TASK-0020/0021/0024).
- No capability transfer across network.

## Decisions (contract)

1) Harness: `tools/os2vm.sh` is the canonical opt-in runner. It must:
   - Launch two QEMU instances with separate UART/QEMU logs.
   - Optional PCAP capture via QEMU `filter-dump` (Wireshark-readable) for deterministic network debugging.
   - Use a deterministic rootless net backend:
     - Default (deterministic): socket listen/connect pair on localhost for point-to-point L2.
     - Optional (when supported): `-netdev socket,mcast=239.42.0.1:37020` for both VMs (shared L2 hub).
   - Pass distinct env/config to each VM (device_id, discovery port 37020, session ports 34567/34568, Noise keys derived deterministically).
   - Fail unless required markers appear within bounded timeout.
   - Ensure networking is configured without relying on DHCP (the socket/mcast backend has no DHCP server):
     - Prefer a deterministic static IPv4 fallback in `netstackd` derived from the virtio-net MAC address.

2) Discovery semantics:
   - Bind UDP on 0.0.0.0:37020 with datagram semantics.
   - Send announce to mcast group; if backend lacks mcast, use broadcast or explicit peer unicast derived from harness config.
   - Peer IP is taken from `recv_from` source; no new advertised-IP field in v1 packet.
   - Maintain bounded LRU with TTL; do not seed synthetic peers in cross-VM mode.

3) Session establishment:
   - Connect via TCP to discovered IP:port; Noise XK handshake; verify device_id ↔ static_pub binding from discovery before application data.
   - Marker `dsoftbusd: cross-vm session ok <peer>` only after authenticated stream.

4) Remote proxy (gateway inside dsoftbusd):
   - Allowlist only `samgrd` and `bundlemgrd` OS-lite frames (SM/BN v1).
   - Bound request/response sizes; deny-by-default; no capability transfer.
   - Audit markers:
     - deny unauthenticated / disallowed service
     - ok marker with peer + service on success.

5) Markers required (cross-VM run, Node ownership):
   - Both nodes: `dsoftbusd: discovery cross-vm up`
   - Both nodes: `dsoftbusd: cross-vm session ok <peer>`
   - Node A only: `SELFTEST: remote resolve ok`
   - Node A only: `SELFTEST: remote query ok`

6) Default single-VM contract:
   - `scripts/qemu-test.sh` remains authoritative and unchanged unless a future task updates it; cross-VM path is opt-in only.

## Constraints / Invariants

- No fake success markers; emit only after real behavior.
- Deterministic schedules/timeouts; no RNG jitter.
- No secrets in logs; test keys labeled as bring-up only.
- Inputs bounded (discovery payload, proxy frames).
- OS services must stay `--no-default-features --features os-lite`; forbidden crates remain blocked (RFC-0009).

## Testing / Proof

- Opt-in: `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` must enforce markers above and fail otherwise.
- Default: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` remains green.
- Host tests: add `test_reject_*` for unauth/disallowed/oversized remote proxy requests in `userspace/dsoftbus`.

## Open Questions / Follow-ons

- If smoltcp/netstack cannot support mcast/bcast receive reliably, a future RFC must define an advertised-IP extension or explicit control-plane to carry peer addresses.
- Migration to schema-based RPC or mux v2 flow-control stays with TASK-0020/0021/0024.

## Checklist

- [x] Harness `tools/os2vm.sh` implemented with mcast + fallback and bounded timeouts.
- [x] 2-VM networking can run without DHCP (static IPv4 fallback in netstackd derived from virtio MAC).
- [x] Cross-VM discovery/session markers emitted only on real success.
- [x] Remote proxy allowlist/bounds + audit/deny markers implemented.
- [x] Host negative tests for remote proxy added.
- [x] Docs updated (testing + distributed notes).
