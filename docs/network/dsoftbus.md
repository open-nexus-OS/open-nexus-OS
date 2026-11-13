---
schema: 1
title: "DSoftBus (dsoftbusd)"
role: guide
subsystem: distributed
sources: [userspace/dsoftbus, source/services/dsoftbusd, source/libs/nexus-noise-xk, source/libs/nexus-discovery-packet, tools/os2vm.sh]
contracts: [ADR-0005, RFC-0007, RFC-0008, RFC-0037, RFC-0060, RFC-0028, RFC-0030]
verified: 2026-11-13
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSoftBus (dsoftbusd)

> **Purpose** — how two devices find each other, authenticate, multiplex streams and reach each other's services and files, on the host and on the OS.
> **Owns** — `userspace/dsoftbus/` (host library + `core/` no_std contract), `source/services/dsoftbusd/` (OS daemon)
> **Contracts** — [ADR-0005](../adr/0005-dsoftbus-architecture.md) (architecture), [RFC-0007](../rfcs/RFC-0007-dsoftbus-os-transport-v1.md) (UDP discovery + TCP sessions), [RFC-0008](../rfcs/RFC-0008-dsoftbus-noise-xk-v1.md) (Noise XK + identity binding), [RFC-0037](../rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md) (OS QUIC v2), [RFC-0060](../rfcs/RFC-0060-dsoftbus-streams-v2-mux-flow-control-keepalive.md) (mux v2), [RFC-0028](../rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md) (remote packagefs RO), [RFC-0030](../rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md) (remote statefs RW)
> **Invariants** — dsoftbusd never owns MMIO; all I/O goes through netstackd's IPC facade. Nothing crosses a session before Noise XK authenticates it. Remote access is deny-by-default and bounded, and every reject fails closed.
> **Read next** — [authority.md](authority.md), [address-matrix.md](address-matrix.md), [../security/identity.md](../security/identity.md)

## Shape

| Layer | Host (`userspace/dsoftbus`, `nexus_env="host"`) | OS (`source/services/dsoftbusd`) |
|---|---|---|
| Discovery | in-process registry (`InProcAuthenticator`) or sockets facade over `userspace/nexus-net` (`FacadeAuthenticator`, `FakeNet` in tests) | UDP announce/receive through netstackd (`nexus-discovery-packet`, `nexus-peer-lru`); port 37020 |
| Handshake | Noise XK (`snow`), static key derived from the device `Identity`, signed proof | Noise XK (`source/libs/nexus-noise-xk`), bring-up test static keys; binding = expected peer static key ([identity](../security/identity.md)) |
| Transport | TCP; QUIC v1 selection `auto\|tcp\|quic` (`transport_selection.rs`, `host_quic.rs`) | TCP streams; QUIC-v2 frames over UDP (`src/os/session/quic_frame.rs`: `MSG1..3`, `PING`/`PONG`) |
| Mux | `mux_v2` | the same `userspace/dsoftbus/src/mux_v2.rs`, included by path |
| Remote services | — | gateway: samgrd resolve, bundle list, packagefs RO, statefs RW |

The discovery announce has a versioned, bounded byte layout with golden vectors (`userspace/dsoftbus/src/discovery_packet.rs`). `userspace/dsoftbus/core` (`dsoftbus-core`, `core + alloc`) holds the transport-neutral contract: nonce guards, payload-vs-channel identity checks, bounded record rejects and the mux.

## OS daemon modes

`dsoftbusd` waits for netstackd's IPv4 address, then picks a mode from it:

| Local IP | Mode | What runs |
|---|---|---|
| `10.42.0.x` (os2vm profile) | cross-VM | UDP discovery and TCP sessions between two QEMU instances, Noise XK, mux v2 sessions, remote proxy gateway |
| anything else, proof boot | held | `dsoftbusd: single-vm session held (proof boot, network family HOLD)` — kernel wait, no CPU, no exit |
| anything else, dev boot | single-VM | dual-node bring-up over loopback (`dsoftbusd: dual-node session ok`), then the selftest server: Noise XK over QUIC-v2 UDP frames plus the mux contract self-test |

`select_os_transport_for_session` always returns QUIC-over-UDP and prints `dsoftbusd: transport selected quic`. The `quic-required` profile forbids the TCP fallback markers (`dsoftbusd: transport selected tcp`, `dsoftbus: quic os disabled (fallback tcp)`).

IPC with netstackd: every RPC carries a trailing `u64` nonce, and dsoftbusd accepts only the reply that carries its op and nonce. Slots come from `nexus-service-topology` (`slots::dsoftbusd::REPLY`, `slots::dsoftbusd::NETSTACKD`), and a CAP_MOVE reply cap is closed on every exit path.

## Mux v2

`userspace/dsoftbus/src/mux_v2.rs`:

| Bound | Value |
|---|---|
| streams per session | 128 |
| frame payload | 32 KiB |
| buffered bytes per stream | 256 KiB |
| initial stream credit | 64 KiB |
| priorities | `0..=7` (`0` highest), high-priority burst limit 8, then lower queues get a round-robin turn |
| keepalive | interval 3 ticks, timeout 9 ticks |

Reject labels: `mux.reject.frame_oversize`, `mux.reject.invalid_stream_state_transition`, `mux.reject.window_credit_overflow_or_underflow`, `mux.reject.unknown_stream_frame`, `mux.reject.unauthenticated_session`, `mux.reject.stream_limit_exceeded`, `mux.reject.duplicate_stream_name`, `mux.reject.invalid_stream_name`. Types are newtypes: `StreamId`, `PriorityClass`, `StreamName`, `WindowCredit`.

## Remote gateway (cross-VM)

`source/services/dsoftbusd/src/os/gateway/`, entered only over an authenticated session:

| Service id | Backend | Contract |
|---|---|---|
| `SVC_SAMGR_RESOLVE_STATUS` (1) | samgrd | resolve status |
| `SVC_BUNDLE_LIST` (2) | bundlemgrd | bundle list |
| `SVC_PACKAGEFS_RO` (3) | packagefsd | `STAT`/`OPEN`/`READ`/`CLOSE` on `pkg:/…` and `/packages/…` only; path ≤ 192 B, read ≤ 128 B, ≤ 8 open handles (`packagefs_ro.rs`) |
| `SVC_STATEFS_RW` (4) | statefsd | GET/PUT/DELETE/LIST; writes only under `/state/shared/`; frame ≤ 256 B, key ≤ 96 B, value ≤ 128 B, list ≤ 64; replies matched by nonce (`statefs_rw.rs`) |

Every remote PUT/DELETE reject prints an audit line (`dsoftbusd: audit remote statefs put reject <reason>`). Reasons: `bad_request`, `unauthenticated`, `outside_acl`, `prefix_escape`, `oversized`.

## Proof

| Layer | Command |
|---|---|
| Host, full | `just test-dsoftbus-host` (`cargo test -p dsoftbus`) |
| Host, mux | `just test-dsoftbus-mux` (`mux_contract_rejects_and_bounds`, `mux_frame_state_keepalive_contract`, `mux_open_accept_data_rst_integration`) |
| Host, QUIC | `just test-dsoftbus-quic` (`quic_host_transport_contract`, `quic_selection_contract`) |
| Host, daemon | `cargo test -p dsoftbusd` (`reject_remote_statefs`, `reject_remote_packagefs`, `reject_transport_validation`, `session_steps`, `p0_unit`) |
| QEMU, QUIC | `just ci-os-quic` (`just test-os quic-required`) |
| QEMU, 2-VM | `just ci-os-os2vm` (runs `tools/os2vm.sh`, checks the `os2vm` marker group); logs in `build/logs/os2vm--<timestamp>/` |

Cross-VM markers include `dsoftbusd: cross-vm session ok`, `dsoftbusd: remote packagefs served`, `dsoftbusd: remote statefs served`, `SELFTEST: remote statefs rw ok`, `dsoftbus:mux crossvm session up`, `dsoftbus:mux crossvm data ok`, `SELFTEST: mux crossvm bulk ok`, `SELFTEST: mux crossvm backpressure ok`.
