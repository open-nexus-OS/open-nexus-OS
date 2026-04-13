# Network & Distributed Debugging (SSOT)

This document is the single source of truth (SSOT) for networking and distributed debugging in Open Nexus OS.
It covers host-first validation, QEMU proof knobs, `tools/os2vm.sh` operations, packet capture, and typed failure triage.

Address/subnet/profile values are maintained in `docs/architecture/network-address-matrix.md`.

## Scope

- Networking proof flow for host + OS runs.
- Distributed proof flow for DSoftBus discovery/session/remote proxy.
- Canonical interpretation of `os2vm` markers, artifacts, and error classification.

For general testing philosophy and non-network domains, see `docs/testing/index.md`.

## Core Principles

- Host-first, OS-last: prove protocol and state-machine logic on host before QEMU.
- Deterministic first: bounded loops, stable markers, explicit phase budgets.
- No fake green: success must prove real behavior, not only API return status.
- Correlated evidence: marker state and on-wire packet evidence should agree.

## Layered Proof Ladder

### Host

- Wire-format encode/decode and golden vectors.
- Noise/session state-machine stepping with host backends.
- Negative tests (`test_reject_*`) for malformed frames, oversized input, identity mismatch.
- Network logic tests in `nexus-net-os`: `cargo test -p nexus-net-os`.

### QEMU 1-VM

- Deterministic smoke and marker ladder via `scripts/qemu-test.sh`.
- Optional strict networking requirements via smoke proof knobs (DHCP/DSoftBus).

### QEMU 2-VM (`os2vm`)

- Cross-VM discovery over real UDP path.
- Cross-VM authenticated session over TCP.
- Remote proxy proof (`remote resolve`, `remote query`, `remote pkgfs stat/open/read/close`, `remote packagefs served`).

## Commands

- Host E2E: `just test-e2e`
- QEMU smoke: `RUN_UNTIL_MARKER=1 just test-os`
- QEMU 2-VM: `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`
- QEMU 2-VM + forced packet capture: `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=debug OS2VM_PCAP=on tools/os2vm.sh`

Convenience targets:

- `just test-dsoftbus-2vm` / `just os2vm`
- `just test-dsoftbus-2vm-pcap` / `just os2vm-pcap`

### TASK-0020 mux marker status

For `TASK-0020` runs, keep marker interpretation explicit:

- **Single-VM proven** (green with `REQUIRE_DSOFTBUS=1`): `dsoftbusd: auth ok`, `dsoftbusd: os session ok`, `dsoftbus:mux session up`, `dsoftbus:mux data ok`, `SELFTEST: mux pri control ok`, `SELFTEST: mux bulk ok`, `SELFTEST: mux backpressure ok`, `SELFTEST: dsoftbus os connect ok`, `SELFTEST: dsoftbus ping ok`.
- **2-VM proven** (green with `RUN_OS2VM=1`): `dsoftbus:mux crossvm session up`, `dsoftbus:mux crossvm data ok`, `SELFTEST: mux crossvm pri control ok`, `SELFTEST: mux crossvm bulk ok`, `SELFTEST: mux crossvm backpressure ok` (checked on both nodes by `tools/os2vm.sh` phase `mux`).
- **2-VM performance-gate proven**: deterministic runtime budgets are enforced in `tools/os2vm.sh` phase `perf` (discovery/session/mux/remote/total thresholds in summary JSON).
- **2-VM hardening soak proven**: bounded stability window is enforced in `tools/os2vm.sh` phase `soak` (node liveness + fail/panic marker absence, configurable soak rounds).

Do not infer distributed mux closure from transport/session markers alone; use the dedicated `phase: mux` ladder checks.

## QEMU Networking Proof Knobs

Single-VM smoke (`scripts/qemu-test.sh`) supports explicit proof gating:

- Default smoke: requires `net: smoltcp iface up ...`
- `REQUIRE_QEMU_DHCP=1`: requests DHCP proof path.
- `REQUIRE_QEMU_DHCP_STRICT=1` with `REQUIRE_QEMU_DHCP=1`: requires deterministic DHCP bound.
- `REQUIRE_DSOFTBUS=1`: requires DSoftBus transport markers in smoke.
- `REQUIRE_DSOFTBUS_REMOTE_PKGFS=1`: enforces remote packagefs markers when cross-VM session markers are present; in single-VM profile it logs an explicit skip warning.

2-VM harness (`tools/os2vm.sh`) supports deterministic performance budgets:

- `OS2VM_BUDGET_ENABLE=1|0`: enable/disable budget enforcement (default `1`)
- `OS2VM_BUDGET_DISCOVERY_MS`, `OS2VM_BUDGET_SESSION_MS`, `OS2VM_BUDGET_MUX_MS`, `OS2VM_BUDGET_REMOTE_MS`, `OS2VM_BUDGET_TOTAL_MS`

2-VM harness (`tools/os2vm.sh`) supports bounded hardening soak checks:

- `OS2VM_SOAK_ENABLE=1|0`: enable/disable soak hardening gate (default `1`)
- `OS2VM_SOAK_DURATION=<seconds|Ns|Nm|Nh>`: soak window duration (default `15s`)
- `OS2VM_SOAK_ROUNDS=<N>`: number of deterministic soak rounds (default `1`)

Release-ready evidence artifact:

- `artifacts/os2vm/runs/<runId>/release-evidence.json` (machine-readable gate snapshot for mux/perf/soak + artifact pointers)

See `docs/adr/0025-qemu-smoke-proof-gating.md` for policy rationale.

## `os2vm` Debug Controls

`tools/os2vm.sh` now supports phase-gated debugging and typed triage.

### Phase & Build Controls

- `OS2VM_PROFILE=debug|ci|fast-local`
  - `debug`: full ladder, typed exits, PCAP on.
  - `ci`: full ladder, typed exits, PCAP auto.
  - `fast-local`: run through session, skip build, PCAP off.
- `RUN_PHASE=build|launch|discovery|session|remote|end`
  - Runs only through the selected phase and exits early on success.
- `OS2VM_SKIP_BUILD=1`
  - Skips compile and validates required artifacts before launch.

### Timeout Controls

- `RUN_TIMEOUT=<duration>` global hard runtime bound (e.g. `180s`).
- `OS2VM_MARKER_TIMEOUT_DISCOVERY=<duration>`
- `OS2VM_MARKER_TIMEOUT_SESSION=<duration>`
- `OS2VM_MARKER_TIMEOUT_REMOTE=<duration>`

If a phase timeout is missing, `MARKER_TIMEOUT` (or `RUN_TIMEOUT`) is used as fallback.

### Packet Capture Controls

- `OS2VM_PCAP=off|on|auto`
  - `off`: no packet capture.
  - `on`: always capture and keep PCAP artifacts.
  - `auto`: capture during run, keep on failure, drop on success.
- `OS2VM_PCAP_BASENAME=<name>`
  - Produces run-scoped files like `<name>-A.pcap` and `<name>-B.pcap`.

### Exit & Summary Controls

- `OS2VM_EXIT_CODE_MODE=legacy|typed`
  - `legacy`: non-zero failures collapse to `1`.
  - `typed`: uses rule-matrix-specific exit codes.
- `OS2VM_SUMMARY_JSON=<path>`
- `OS2VM_SUMMARY_TXT=<path>`
- `OS2VM_SUMMARY_STDOUT=1|0`

### Artifact & Retention Controls

- `OS2VM_ARTIFACT_ROOT=<path>` (default: `artifacts/os2vm`)
- `OS2VM_RETENTION_ENABLE=1|0`
- `OS2VM_RETENTION_KEEP_SUCCESS=<n>`
- `OS2VM_RETENTION_KEEP_FAILURE=<n>`
- `OS2VM_RETENTION_MAX_TOTAL_MB=<n>`
- `OS2VM_RETENTION_MAX_AGE_DAYS=<n>`
- `OS2VM_SANDBOX_CACHE_GC=auto|on|off`
- `OS2VM_SANDBOX_CACHE_DIR=<path>` (default: `/tmp/cursor-sandbox-cache`)
- `OS2VM_SANDBOX_CACHE_MAX_MB=<n>`
- `OS2VM_SANDBOX_CACHE_TARGET_FREE_MB=<n>`
- `OS2VM_SANDBOX_CACHE_MIN_AGE_SECS=<n>`

Default behavior: `os2vm` runs sandbox-cache GC automatically during `phase_build` (mode `auto`) and prunes stale entries when cache pressure or low `/tmp` free space is detected.

## `os2vm` Artifacts (Evidence Bundle)

Per run:

- Run directory: `artifacts/os2vm/runs/<runId>/`
- UART logs: `uart-A.txt`, `uart-B.txt`
- Host/QEMU streams: `host-A.txt`, `host-B.txt`
- Optional PCAP: `<basename>-A.pcap`, `<basename>-B.pcap`
- Structured run summaries: `summary.json`, `summary.txt`
- Metadata: `result.txt` and `pids.env`

Summary contains:

- Phase durations and configured budgets
- First failure classification (`errorCode`, `phase`, `node`, `subsystem`)
- Missing marker and next-step hint
- Marker line hits for both nodes
- Packet counters (ARP/UDP/TCP/SYN/SYN-ACK/RST) when PCAP is available

## Typed Error Rule Matrix

`os2vm` classifies failures with stable error codes.

| Error code | Typed exit | Phase | Typical subsystem | Primary meaning |
| --- | ---: | --- | --- | --- |
| `OS2VM_E_DISCOVERY_TIMEOUT` | 31 | discovery | dsoftbusd/discovery | Discovery marker not reached in time |
| `OS2VM_E_DISCOVERY_NODE_A_ENDED` | 32 | discovery | selftest-client/node-a | Node A hit terminal marker before discovery success |
| `OS2VM_E_DISCOVERY_NODE_B_ENDED` | 33 | discovery | selftest-client/node-b | Node B hit terminal marker before discovery success |
| `OS2VM_E_SESSION_TIMEOUT` | 41 | session | dsoftbusd/session | Session marker missing within budget |
| `OS2VM_E_SESSION_NODE_A_ENDED` | 42 | session | selftest-client/node-a | Node A ended before session success |
| `OS2VM_E_SESSION_NODE_B_ENDED` | 43 | session | selftest-client/node-b | Node B ended before session success |
| `OS2VM_E_SESSION_NO_SYN` | 44 | session | netstackd/connect | No TCP SYN observed in packet evidence |
| `OS2VM_E_SESSION_NO_SYNACK` | 45 | session | netstackd/accept-or-network | SYN observed, but no SYN-ACK evidence |
| `OS2VM_E_REMOTE_RESOLVE_MISSING` | 51 | remote | remote-proxy-resolve | `remote resolve ok` marker missing |
| `OS2VM_E_REMOTE_QUERY_MISSING` | 52 | remote | remote-proxy-query | `remote query ok` marker missing |
| `OS2VM_E_REMOTE_PKGFS_STAT_MISSING` | 53 | remote | remote-proxy-packagefs-stat | `remote pkgfs stat ok` marker missing |
| `OS2VM_E_REMOTE_PKGFS_OPEN_MISSING` | 54 | remote | remote-proxy-packagefs-open | `remote pkgfs open ok` marker missing |
| `OS2VM_E_REMOTE_PKGFS_READ_MISSING` | 55 | remote | remote-proxy-packagefs-read | `remote pkgfs read step ok` marker missing |
| `OS2VM_E_REMOTE_PKGFS_CLOSE_MISSING` | 56 | remote | remote-proxy-packagefs-close | `remote pkgfs close ok` marker missing |
| `OS2VM_E_REMOTE_PKGFS_FLOW_MISSING` | 57 | remote | remote-proxy-packagefs | final `remote pkgfs read ok` marker missing |
| `OS2VM_E_REMOTE_SERVED_MISSING` | 58 | remote | remote-proxy-node-b | `remote packagefs served` marker missing |
| `OS2VM_E_BUILD_ARTIFACT_MISSING` | 61 | build | build/artifacts | Required binaries not present for run |
| `OS2VM_E_UNEXPECTED` | 99 | any | unknown | Unclassified shell/runtime failure |

## Packet Capture & Correlation

Packet capture is the fastest way to confirm actual network behavior:

```bash
RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PCAP=on tools/os2vm.sh
```

Inspect in Wireshark/tshark with filters:

- `arp`
- `udp.port == 37020`
- `tcp.port == 34567 || tcp.port == 34568`

Correlation rule of thumb:

- Marker says dial but packet evidence has no SYN -> connect path issue.
- SYN exists but no SYN-ACK -> peer listen/readiness or network path issue.
- Session marker present and packet evidence present -> transport proof is strong.

## Debugging Workflow (Recommended)

1. Run host-first tests (`remote_e2e`, `nexus-net-os`) to narrow logic issues.
2. Run `os2vm` phase-gated:
   - `RUN_PHASE=discovery`
   - `RUN_PHASE=session`
   - `RUN_PHASE=remote`
3. Inspect `artifacts/os2vm/runs/<runId>/summary.json` first (root-cause candidate).
4. Use UART logs for service-level details.
5. Use PCAP to confirm on-wire behavior and reconcile with markers.

## Avoiding Fake Success

- Treat marker-only success as insufficient for transport-sensitive paths.
- Prefer proof pairs:
  - Marker + packet evidence.
  - Internal state + externally observable event.
- Keep “state entered” markers distinct from “wire observed” conclusions.

## Remote E2E (Host) Context

`tests/remote_e2e` remains the fastest deterministic path for discovery/auth/proxy logic.
Use it to validate protocol and routing behavior before moving to `os2vm` runtime evidence.

## IPC Slot Mismatch Runbook

A common deterministic failure mode is slot mismatch between init-lite allocation and hardcoded service slot constants.

Symptoms:

- send path appears successful, receive queue stays empty
- repeated timeouts without explicit transport failure

Triage:

1. log actual slot assignment from init-lite
2. compare with service constants
3. align constants or use routing-query based resolution where available

## Init-lite Topic Logging for Networking Triage

Use `INIT_LITE_LOG_TOPICS` to opt into richer init-lite diagnostics during network/distributed debugging:

- `general`
- `svc-meta`
- `probe`

Example:

```bash
INIT_LITE_LOG_TOPICS=general,svc-meta,probe RUN_UNTIL_MARKER=1 just test-os
```
