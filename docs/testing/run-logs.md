# build/logs — test run logs and hypothesis grid (agent-readable)

Since 2026-05-31 all test logs live under `build/logs/`. Run directories are named
`<profile>--<timestamp>` (e.g. `headless--2026-07-17T16-05-14`, `manual--2026-07-17T16-05-14`,
`os2vm--2026-07-17T16-05-14` for the 2-VM harness).

```
build/logs/
  README.md              ← this file (hypothesis grid reference)
  latest → {run_dir}/    ← symlink to most recent run — CAN BE STALE; prefer the newest <profile>--<timestamp> dir by mtime
  {run_dir}/
    hypothesis.json       ← structured check grid (one JSON line per hypothesis)
    uart.log              ← guest serial output
    qemu.stderr           ← QEMU's own error stream
    build.stderr          ← compiler warnings/errors (persisted)
```

The old flat files (`uart.log`, `qemu.log` in repo root) and `.cursor/debug*.log` are obsolete.
Use the newest run's `hypothesis.json` as the first triage stop after any QEMU run.
`just logs-gc [keep]` prunes old runs (keeps the newest 5 per profile by default).

## Hypothesis grid legend

Every `hypothesis.json` entry has a `hypothesisId` field. Use this grid to decode failures:

| ID | Location | Meaning | Triaged by |
|----|----------|---------|------------|
| **A** | `qemu-test.sh:pre-run` / `:exit` | QEMU smoke start / exit summary (exit_code, saw_init_start, dhcp_bound) | Always check first |
| **B** | `qemu-test.sh:diag-kpgf-signature` | Exec KPGF syscall signature (a7, a0, a2, is_as_map_signature) | Kernel crash triage |
| **C** | `qemu-test.sh:diag-timed-order` | timed markers before crash (timed_before_kpgf) | Kernel crash triage |
| **D** | `qemu-test.sh:diag-exec-progress` | Exec phase progression around crash (crash_before_child_hello) | Kernel crash triage |
| **E** | `qemu-test.sh:diag-address-range` | Fault address relative to syscall range (fault_inside_a0_a0_plus_a2) | MMU / syscall triage |
| **F** | `qemu-test.sh:diag-exec-routing` | Exec routing readiness before crash (routing_ready_before_crash) | IPC routing triage |
| **H1** | `qemu-test.sh:effective-config` / `run-qemu-rv64.sh:build-paths` | Effective flags/env before run (service_list, run_until_marker, cargo_target_dir) | Build/env misconfig |
| **H2** | `qemu-test.sh:artifact-state` | Kernel/init-lite ELF presence + size + mtime BEFORE QEMU launch | Build failure detection |
| **H3** | `qemu-test.sh:marker-progression` | UART marker counts post-run (init: start, init: ready, last_uart_line) | Guest progress check |
| **H4** | `run-qemu-rv64.sh:build-errors` | Cargo build errors per service label (exit_code, errors: [error[E0599], …]) | **Compiler error triage** |
| **H4b** | `run-qemu-rv64.sh:build-warnings` | Cargo build warnings per service label (count, warnings: [warning[…], …]) | **Warning hygiene** |
| **H5** | `qemu-test.sh:pm-cli-select` | Proof-manifest CLI resolution (path, source) | Tooling misconfig |
| **H6–H13** | `qemu-test.sh:diag-metrics-*` | metricsd/logd/sink diagnostics (rejects, counters, emission, append) | Observability triage |
| **H15** | `qemu-test.sh:build-failure-summary` | Post-run: if kernel/init still missing, collects H4 error entries for human-readable triage (kernel_missing, init_missing, h4_error_entries, h4_sample) | **Build failure summary** |
| **J** | `qemu-test.sh:diag-runqemu-result` | QEMU exit code + log artifact presence (qemu_status, uart_exists) | QEMU launch failure |
| **K** | `qemu-test.sh:diag-qemu-launch-errors` | QEMU launch error signatures (lock conflict, binary missing) | QEMU env triage |
| **DNS** | `qemu-test.sh:diag-net-dns-proof` | DHCP/DNS proof markers (dhcp_bound, dns_ok, dns_fail) | Network triage |
| **N1–N9** | `qemu-test.sh:diag-selftest-*` / `:diag-statefs-*` / `:diag-policyd-*` / `:diag-rngd-*` / `:diag-execd-*` / `:diag-netstackd-*` | Selftest checkpoint progression, statefs deny pressure, policyd MMIO, rngd policy, execd deny class, netstackd MMIO | Service-specific triage |

### Common triage flows

**Build failure (kernel/init missing)**:
1. Check **A** exit_code ≠ 0
2. Check **H2** artifact-state: kernel_exists=false, init_exists=false
3. Check **H15** build-failure-summary for aggregated H4 entries
4. Drill into **H4** build-errors for exact `error[E…]` lines per service
5. Check **H4b** build-warnings for hygiene issues that may escalate

**QEMU launched but no UART output**:
1. Check **J** qemu_status ≠ 0, uart_exists=false
2. Check **K** qemu launch errors (lock conflict, missing binary)
3. Check **H2** artifact-state if not already triaged above

**Guest boots but markers missing**:
1. Check **H3** count_init_start, last_uart_line
2. Check **A** saw_init_start
3. Check phase-specific hypotheses (DNS, N1-N9, B-F)
