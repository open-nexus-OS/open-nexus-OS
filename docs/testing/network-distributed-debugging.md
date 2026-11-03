---
schema: 1
title: "Network and distributed debugging"
role: reference
subsystem: devx
sources: [tools/os2vm.sh, scripts/qemu-test.sh, tests/remote_e2e, userspace/dsoftbus, source/services/dsoftbusd, tools/pcap_summary.py]
contracts: [RFC-0007, RFC-0010, ADR-0025]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Network and distributed debugging

> **Purpose** — how to prove and debug networking and DSoftBus: the host suites, the single-VM network lanes, and the `tools/os2vm.sh` 2-VM harness with its knobs, artifacts and typed failures.
> **Owns** — operations of `tools/os2vm.sh` and the network gates of `scripts/qemu-test.sh`; addresses and subnets are in `docs/network/address-matrix.md`
> **Contracts** — [RFC-0007](../rfcs/RFC-0007-dsoftbus-os-transport-v1.md) (DSoftBus OS transport), [RFC-0010](../rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md) (cross-VM harness), [ADR-0025](../adr/0025-qemu-smoke-proof-gating.md) (network proofs are opt-in per profile)
> **Invariants** — host first. A transport claim needs marker and wire evidence together. Network gates are selected by profile, never by `REQUIRE_*` in a recipe.
> **Read next** — [e2e.md](e2e.md), [run-logs.md](run-logs.md), [os-markers.md](os-markers.md)

## Proof ladder

| Layer | Proves | Run |
|---|---|---|
| Host unit | wire formats, Noise/session state machines, mux bounds, `test_reject_*` | `just test-dsoftbus-host`, `just test-dsoftbus-mux`, `just test-dsoftbus-quic`, `cargo test -p nexus-net-os` |
| Host E2E | discovery → auth → remote proxy over `FakeNet` | `cargo test -p remote_e2e` |
| QEMU 1-VM | smoltcp interface, DHCP, DSoftBus session in one VM | `just ci-os-dhcp`, `just ci-os-dhcp-strict`, `just ci-os-quic` |
| QEMU 2-VM | cross-VM discovery (UDP), session (TCP), mux, remote packagefs/statefs, perf budgets, soak | `just ci-os-os2vm` |

`just ci-network` runs `ci-os-dhcp`, `ci-os-quic` and `ci-os-os2vm`. It is not part of
`just test-all` or CI.

## Single-VM gates (`scripts/qemu-test.sh`)

Every lane requires `net: smoltcp iface up`. Profiles add:

| Profile | Env it declares | Requires |
|---|---|---|
| `dhcp` | `REQUIRE_QEMU_DHCP=1` | the DHCP path (deterministic fallback allowed) |
| `dhcp-strict` | + `REQUIRE_QEMU_DHCP_STRICT=1` | DHCP bound, ping and DNS proof; DNS fail markers rejected |
| `quic-required` | `REQUIRE_DSOFTBUS=1` | `dsoftbusd: transport selected quic`, `dsoftbusd: auth ok`, `SELFTEST: quic session ok`; `dsoftbusd: transport selected tcp` is forbidden |
| `os2vm` | `REQUIRE_DSOFTBUS=1`, `REQUIRE_DSOFTBUS_REMOTE_PKGFS=1`, `REQUIRE_DSOFTBUS_REMOTE_STATEFS=1` | read by `tools/os2vm.sh` |

The single-VM DSoftBus probes that need a peer (`SELFTEST: dsoftbus os connect FAIL`,
`SELFTEST: dsoftbus ping FAIL`) are tolerated by `config/fail-marker-allow.txt` with their
tracking reference.

## 2-VM harness (`tools/os2vm.sh`)

Recipes: `just ci-os-os2vm` (= `just test-os2vm`: runs the harness, then the `os2vm` chain group
against node A's log), `just os2vm` (harness only), `just os2vm-pcap` /
`just test-dsoftbus-2vm-pcap` (with packet capture). Direct:
`RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci tools/os2vm.sh`.

Phases, in order: `build`, `launch`, `discovery`, `session`, `mux`, `remote`, `perf`, `soak`, `end`.
`RUN_PHASE=<phase>` stops after that phase on success.

### Knobs

| Knob | Values (default) | Effect |
|---|---|---|
| `OS2VM_PROFILE` | `debug` (default), `ci`, `fast-local` | `debug`: full run, PCAP on; `ci`: full run, PCAP auto; `fast-local`: through `session`, skip build, PCAP off. All use typed exits. |
| `OS2VM_SKIP_BUILD` | `0`/`1` | skip the build, check artifacts exist |
| `RUN_TIMEOUT` | duration | global bound |
| `OS2VM_MARKER_TIMEOUT_DISCOVERY` / `_SESSION` / `_MUX` / `_REMOTE` | duration | per-phase wait; falls back to `MARKER_TIMEOUT` / `RUN_TIMEOUT` |
| `OS2VM_PCAP` | `off`, `on`/`1`, `auto` | `auto` keeps captures only on failure |
| `OS2VM_PCAP_BASENAME` | (`packets`) | `<name>-A.pcap`, `<name>-B.pcap` |
| `OS2VM_EXIT_CODE_MODE` | `typed`, `legacy` | `legacy` collapses failures to `1` |
| `OS2VM_BUDGET_ENABLE` | (`1`) | enforce phase budgets |
| `OS2VM_BUDGET_DISCOVERY_MS` / `_SESSION_MS` / `_MUX_MS` / `_REMOTE_MS` / `_TOTAL_MS` | (`15000`/`5000`/`5000`/`60000`/`120000`) | `perf` phase thresholds |
| `OS2VM_SOAK_ENABLE`, `OS2VM_SOAK_DURATION`, `OS2VM_SOAK_ROUNDS` | (`1`, `15s`, `1`) | `soak` phase: liveness and no FAIL/panic markers |
| `OS2VM_SUMMARY_JSON`, `OS2VM_SUMMARY_TXT`, `OS2VM_SUMMARY_STDOUT` | paths, `0`/`1` | summary outputs |
| `OS2VM_ARTIFACT_ROOT` | (`build/logs`) | where run dirs go |
| `OS2VM_RETENTION_*` | `ENABLE` (`1`), `KEEP_SUCCESS` (`4`), `KEEP_FAILURE` (`12`), `MAX_TOTAL_MB` (`3072`), `MAX_AGE_DAYS` (`14`) | run-dir retention |
| `OS2VM_SANDBOX_CACHE_*` | `GC` (`auto`), `DIR`, `MAX_MB`, `TARGET_FREE_MB`, `MIN_AGE_SECS` | build-cache GC during `build` |

### Artifacts

A run writes `build/logs/os2vm--<ts>/`:

| File | Content |
|---|---|
| `uart-A.txt`, `uart-B.txt` | each node's serial output |
| `host-A.txt`, `host-B.txt` | each node's QEMU host stream |
| `<basename>-A.pcap`, `<basename>-B.pcap` | captures, when kept |
| `summary.json`, `summary.txt` | phase durations and budgets, first failure (`errorCode`, `phase`, `node`, `subsystem`, hint), marker hits per node, packet counters |
| `release-evidence.json` | gate snapshot for mux/perf/soak with artifact pointers |
| `hypothesis.json` | triage records ([run-logs.md](run-logs.md)) |
| `result.txt`, `pids.env` | result and node PIDs |

Each node also assembles an evidence bundle ([evidence-bundle.md](evidence-bundle.md)).

### Typed failures

| Code | Exit | Subsystem |
|---|---:|---|
| `OS2VM_E_DISCOVERY_TIMEOUT` | 31 | dsoftbusd/discovery (UDP 37020 path) |
| `OS2VM_E_DISCOVERY_NODE_A_ENDED` / `_B_ENDED` | 32 / 33 | a node ended before discovery |
| `OS2VM_E_SESSION_TIMEOUT` | 41 | dsoftbusd/session |
| `OS2VM_E_SESSION_NODE_A_ENDED` / `_B_ENDED` | 42 / 43 | a node ended before the session |
| `OS2VM_E_SESSION_NO_SYN` | 44 | netstackd/connect: no SYN on the wire |
| `OS2VM_E_SESSION_NO_SYNACK` | 45 | netstackd/accept or network: SYN without SYN-ACK |
| `OS2VM_E_MUX_TIMEOUT` | 46 | dsoftbusd/mux-crossvm |
| `OS2VM_E_MUX_NODE_A_ENDED` / `_B_ENDED` | 47 / 48 | a node ended before mux |
| `OS2VM_E_MUX_NEGATIVE_MARKER` | 49 | dsoftbusd/mux-crossvm |
| `OS2VM_E_REMOTE_RESOLVE_MISSING`, `_QUERY_MISSING` | 51, 52 | remote proxy resolve, query |
| `OS2VM_E_REMOTE_PKGFS_STAT/_OPEN/_READ/_CLOSE_MISSING` | 53–56 | remote packagefs steps |
| `OS2VM_E_REMOTE_PKGFS_FLOW_MISSING` | 57 | final `remote pkgfs read ok` |
| `OS2VM_E_REMOTE_SERVED_MISSING` | 58 | node B never served packagefs |
| `OS2VM_E_REMOTE_STATEFS_FLOW_MISSING` | 60 | remote statefs flow |
| `OS2VM_E_BUILD_ARTIFACT_MISSING` | 61 | build/artifacts |
| `OS2VM_E_HOST_ENOSPC` | 62 | host storage full |
| `OS2VM_E_REMOTE_STATEFS_SERVED_MISSING` | 65 | node B never served statefs |
| `OS2VM_E_REMOTE_NEGATIVE_MARKER` | 66 | selftest-client/remote |
| `OS2VM_E_LAUNCH_NODE_A_FAILED` / `_B_FAILED` | 71 / 72 | qemu/launch |
| `OS2VM_E_PERF_BUDGET_DISCOVERY/_SESSION/_MUX/_REMOTE/_TOTAL` | 81–85 | a `perf` budget exceeded |
| `OS2VM_E_SOAK_NODE_A_ENDED` / `_B_ENDED`, `OS2VM_E_SOAK_FAIL_MARKER` | 86 / 87, 88 | `soak` phase |
| `OS2VM_E_UNEXPECTED` | 99 | unclassified |

Evidence-pipeline failures (`OS2VM_E_EVIDENCE_BIN`, `_DISABLED`, `_NODE_A`, `_NODE_B`) are
recorded with the same mechanism.

## Packet capture

```bash
RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PCAP=on tools/os2vm.sh
```

Filters: `arp`, `udp.port == 37020` (discovery), `tcp.port == 34567 || tcp.port == 34568`
(sessions). Connections to `127/8` or the interface's own address are served inside netstackd
(RFC-0092 §5) and never reach the NIC, so they are invisible to a capture.

| Markers say | Wire shows | Look at |
|---|---|---|
| dial | no SYN | the connect path |
| dial | SYN, no SYN-ACK | peer listen/readiness or the network |
| session up | SYN/SYN-ACK present | transport proof holds |

## Workflow

1. Host suites first (`remote_e2e`, `dsoftbus`, `nexus-net-os`).
2. `tools/os2vm.sh` phase by phase: `RUN_PHASE=discovery`, then `session`, then `remote`.
3. `summary.json` first (classified root cause), then the UART logs, then the PCAP.
4. A distributed mux claim needs the `mux` phase checks on both nodes, not session markers alone.

## Other aids

- **Slot mismatch:** sends succeed but the receiver's queue stays empty and waits time out. Compare
  the slot init assigned (init log) with the constant the service uses; prefer routing-query
  resolution.
- **Init diagnostics:** `INIT_LITE_LOG_TOPICS=general,svc-meta,probe RUN_UNTIL_MARKER=1 just test-os`
  (topics are baked in at build time).
