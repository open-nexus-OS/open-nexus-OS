# Tools Overview

This folder contains operational tooling for Open Nexus OS development, diagnostics, and proof harnesses.

## OS2VM Harness Quick Guide

`tools/os2vm.sh` is the canonical 2-VM proof harness for cross-VM DSoftBus behavior.
It now supports phase-gated execution, typed error classification, structured summaries, and packet-capture modes.

### Base command

```bash
RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
```

### Built-in profiles

`tools/os2vm.sh` now supports profile defaults via `OS2VM_PROFILE`:

- `debug` (default): full run, typed exits, PCAP on
- `ci`: full run, typed exits, PCAP auto
- `fast-local`: stop at session, skip build, no PCAP

### Common profiles

- Quick triage (fast rerun, no rebuild):

```bash
RUN_OS2VM=1 OS2VM_PROFILE=fast-local RUN_TIMEOUT=120s tools/os2vm.sh
```

- Packet deep dive (keep full PCAP evidence):

```bash
RUN_OS2VM=1 OS2VM_PROFILE=debug RUN_PHASE=session OS2VM_PCAP=on OS2VM_PCAP_BASENAME=deep-dive RUN_TIMEOUT=180s tools/os2vm.sh
```

- Remote flow validation (resolve/query/pkgfs path):

```bash
RUN_OS2VM=1 OS2VM_PROFILE=ci RUN_PHASE=remote OS2VM_SKIP_BUILD=1 OS2VM_PCAP=auto RUN_TIMEOUT=180s tools/os2vm.sh
```

- Full end-to-end verification:

```bash
RUN_OS2VM=1 OS2VM_PROFILE=ci RUN_PHASE=end RUN_TIMEOUT=180s tools/os2vm.sh
```

### Important flags

- `OS2VM_PROFILE=debug|ci|fast-local`
- `RUN_PHASE=build|launch|discovery|session|remote|end`
- `OS2VM_SKIP_BUILD=1` (validate artifacts, skip compile)
- `OS2VM_MARKER_TIMEOUT_DISCOVERY|SESSION|REMOTE`
- `OS2VM_PCAP=off|on|auto`
- `OS2VM_PCAP_BASENAME=<name>`
- `OS2VM_EXIT_CODE_MODE=legacy|typed`
- `OS2VM_SUMMARY_JSON=<path>`
- `OS2VM_SUMMARY_TXT=<path>`
- `OS2VM_SUMMARY_STDOUT=1|0`
- `OS2VM_ARTIFACT_ROOT=<path>` (default: `artifacts/os2vm`)
- `OS2VM_RETENTION_ENABLE=1|0`
- `OS2VM_RETENTION_KEEP_SUCCESS=<n>`
- `OS2VM_RETENTION_KEEP_FAILURE=<n>`
- `OS2VM_RETENTION_MAX_TOTAL_MB=<n>`
- `OS2VM_RETENTION_MAX_AGE_DAYS=<n>`
- `OS2VM_SANDBOX_CACHE_GC=auto|on|off`
- `OS2VM_SANDBOX_CACHE_DIR=<path>` (default: `/tmp/cursor-sandbox-cache`)
- `OS2VM_SANDBOX_CACHE_MAX_MB=<n>` (auto-gc threshold)
- `OS2VM_SANDBOX_CACHE_TARGET_FREE_MB=<n>` (auto-gc free-space target)
- `OS2VM_SANDBOX_CACHE_MIN_AGE_SECS=<n>` (auto-gc protects fresh entries)
- `NETDEV_A` / `NETDEV_B` (override default multicast netdev)

### Outputs and evidence

Per run, the harness emits:

- run-scoped directory: `artifacts/os2vm/runs/<runId>/`
  - `uart-A.txt`, `uart-B.txt`
  - `host-A.txt`, `host-B.txt`
  - `blk-A.img`, `blk-B.img`
  - PCAP files when enabled (`<basename>-A.pcap`, `<basename>-B.pcap`)
  - summaries: `summary.json`, `summary.txt`
  - `result.txt` and `pids.env` metadata

The JSON summary includes:

- phase durations
- marker line hits
- first failure classification (`OS2VM_E_*`)
- subsystem/node hints
- packet counters (ARP/UDP/TCP/SYN/SYN-ACK/RST) when available

### Typed error mode

With `OS2VM_EXIT_CODE_MODE=typed`, failures return phase-specific non-zero exit codes.
This is recommended for CI pipelines and automated triage.

### Operational notes

- Run QEMU proofs sequentially (do not run multiple smoke/harness jobs in parallel).
- Default `TMPDIR` is run-scoped (`<runDir>/tmp`) to avoid `/tmp` saturation.
- Retention/GC runs after each harness run; keep counts and size/age budgets are configurable.
- Sandbox cache GC runs automatically in `phase_build` (mode `auto`) and trims stale `/tmp/cursor-sandbox-cache` entries when cache size is too large or `/tmp` free space is too low.
- For network/distributed debugging policy and rule matrix details, see:
  - `docs/testing/network-distributed-debugging.md`
