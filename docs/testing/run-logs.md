---
schema: 1
title: "Run logs and triage"
role: reference
subsystem: devx
sources: [scripts/qemu-test.sh, scripts/qemu-launcher.sh, scripts/build.sh, scripts/hypothesis-log.sh, tools/os2vm.sh, tools/uart-filter.py]
contracts: [RFC-0107, ADR-0063]
verified: 2026-11-04
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Run logs and triage

> **Purpose** — what a run directory under `build/logs/` holds, how to decode `hypothesis.json`, and the first moves when a lane fails.
> **Owns** — the log layout written by `scripts/qemu-test.sh`, `scripts/qemu-launcher.sh`, `scripts/build.sh`, `tools/os2vm.sh`
> **Contracts** — [RFC-0107](../rfcs/RFC-0107-boot-trace-console-markers-on-the-boot-disk.md) (boot trace on the boot disk), [ADR-0063](../adr/0063-proof-lane-resource-envelope-and-authority.md) (lane envelope)
> **Invariants** — read the newest `<profile>--<timestamp>` directory, not a possibly stale `latest`. A missing marker is a regression until proven otherwise. No keystrokes in any log.
> **Read next** — `.claude/skills/boot-proof/SKILL.md`, [os-markers.md](os-markers.md), [network-distributed-debugging.md](network-distributed-debugging.md)

## Layout

```
build/logs/
  latest -> <profile>--<ts>/   may be stale; prefer the newest dir by name/mtime
  <profile>--<ts>/             one QEMU lane (headless, smp1, visible, manual, …)
    uart.log                   guest serial output
    hypothesis.json            NDJSON triage records (one object per line)
    qemu.stderr                QEMU's own error stream
    trace-loader.txt           the loader lines the boot disk kept (RFC-0107)
    trace-os-<seq>.txt         each boot's kernel console text as the block owner kept it
    (pixel-proof PNG/JSON on visible lanes)
  os2vm--<ts>/                 2-VM run: uart-A.txt, uart-B.txt, host-A/B.txt, summary.json, …
  board--<ts>/                 a board boot (`just board-serial` or `just board-log`):
    uart.log, trace-all.log    latest boot / every kept boot
  latest-board -> board--<ts>/
```

`just logs-gc [keep]` keeps the newest 5 runs per profile prefix by default. Logs are trimmed
after the run (`UART_LOG_MAX`, `QEMU_LOG_MAX`).

**Trace contract** (RFC-0107, every lane): the boots the run kept on the disk, from its first
`nxboot: trace slot=<n> seq=<m>` on, hold exactly the UART's `nxboot:` lines, and each boot's OS
region is the UART text after its jump line, byte for byte. `[error] trace contract` names the
difference. Inspect the disk with `nx image trace --image build/nexus.img --all` (one boot:
`--seq <n> --os`).

## `hypothesis.json`

Each line: `{"runId", "hypothesisId", "location", "message", "data", "timestamp"}`. The
`location` names the emitting script and check; IDs are grouped by what they answer.

| ID | Location(s) | Answers | Use for |
|---|---|---|---|
| `A` | `qemu-test.sh:pre-run`, `:post-run`, `:exit` | start/exit summary (exit code, `saw_init_start`, DHCP bound) | always first |
| `B` | `qemu-test.sh:diag-kpgf-signature` | the syscall signature around a kernel page fault | kernel crash |
| `C` | `qemu-test.sh:diag-timed-order` | `timed` markers before the crash | kernel crash |
| `D` | `qemu-test.sh:diag-exec-progress` | exec progress around the crash | kernel crash |
| `E` | `qemu-test.sh:diag-address-range` | fault address vs the syscall's buffer | MMU / syscall |
| `F` | `qemu-test.sh:diag-exec-routing` | routing readiness before the crash | IPC routing |
| `H1` | `qemu-test.sh:effective-config`, `:verify-uart-pre/-post`; `qemu-launcher.sh:resolved-config`; `build.sh:build-paths`; `os2vm.sh:session_wait_timeout` | effective env/flags/paths; profile vs harts; verify-uart result | env or profile misconfig |
| `H2` | `qemu-test.sh:artifact-state`; `os2vm.sh:launch`; `justfile:test-dsoftbus-*` | kernel/init ELF presence, size, mtime before launch; 2-VM launch | build output missing |
| `H3` | `qemu-test.sh:marker-progression`; `os2vm.sh:discovery_wait_*`, `session_wait_*` | marker counts and last UART line; 2-VM waits | guest progress |
| `H4` | `build.sh:build-errors`; `tools/qmp_input.py`, `tools/qmp_visible_input_inject.py`; `os2vm.sh:session_wait_done`, `remote_markers_done` | cargo errors per service; injector and scenario steps (`scenario step ok`, `[scenario] FAIL …`); 2-VM phase results | compiler errors; input lanes |
| `H4b` | `build.sh:build-warnings` | warnings per service (the warning gate under `NEXUS_WARN_GATE=1`) | warning hygiene |
| `H5` | `qemu-test.sh:pm-cli-select`; `justfile:test-all:start/end` | which `nexus-proof-manifest` binary was used; `test-all` bounds | tooling |
| `H6` | `qemu-test.sh:diag-metrics-selftest-order` | metrics selftest marker order and misses | observability |
| `H7` | `qemu-test.sh:diag-metrics-emission-counts` | metricsd emission counts in the UART | observability |
| `H8` | `qemu-test.sh:diag-logd-metrics-append` | logd's append statuses for metricsd | observability |
| `H9` | `qemu-test.sh:diag-sink-logd-send-path` | the `nexus-log` sink-logd send path | observability |
| `H10` | `qemu-test.sh:diag-logd-scope-metricsd` | append frames logd saw with metricsd scope | observability |
| `H11` | `qemu-test.sh:diag-sink-selftest-slot-mode` | selftest sink-logd slot resolution | observability |
| `H12` | `qemu-test.sh:diag-sink-selftest-send` | selftest sink-logd send outcome | observability |
| `H13` | `qemu-test.sh:diag-sink-selftest-probe-marker` | selftest sink-logd probe marker | observability |
| `H14` | `qemu-launcher.sh:host-resources-pre` | host resource snapshot before QEMU starts | host pressure |
| `H15` | `qemu-test.sh:build-failure-summary` | kernel/init still missing after the run, with H4 samples | build failure |
| `J` | `qemu-test.sh:diag-runqemu-result`; `qemu-launcher.sh:qemu-result` | QEMU exit code, UART present | QEMU launch |
| `K` | `qemu-test.sh:diag-qemu-launch-errors` | launch error signatures (lock conflict, missing binary) | QEMU env |
| `R` | `qemu-test.sh:envelope`, `:signal`, resource checks | lane `memory.peak`, envelope exceeded/unenforced, external signal | lane killed |
| `DNS`, `DNS_GATE` | `qemu-test.sh:diag-net-dns-proof`, `:dhcp-gate` | DHCP/DNS proof markers and gate state | network |
| `N1`–`N9` | `qemu-test.sh:diag-selftest-*`, `diag-statefs-deny-pressure`, `diag-netstackd-mmio-failure`, `diag-execd-deny-class`, `diag-policyd-*`, `diag-rngd-policy-subject` | selftest failure locus and checkpoints, statefs deny pressure, netstackd MMIO, execd deny class, policyd MMIO/RNG, rngd subject | service-specific |

Writers: `agent_debug_log` in `scripts/qemu-test.sh` and `tools/os2vm.sh`, `debug_log` in
`scripts/qemu-launcher.sh` and `scripts/build.sh`, `scripts/hypothesis-log.sh` from recipes,
`append_debug_log` in the Python injector.

## Triage flows

**Build failure (kernel/init missing):** `A` exit code ≠ 0 → `H2` `kernel_exists=false` →
`H15` summary → `H4` exact `error[E…]` per service → `H4b` warnings.

**QEMU started, no UART:** `J` (`qemu_status`, `uart_exists=false`) → `K` (lock conflict, missing
binary) → `qemu.stderr`. A stale QEMU holding the image lock: `pgrep -af qemu-system-riscv64`.

**Boots, markers missing:** `H3` (`last_uart_line`) → the harness line
`first_failed_phase=… missing_marker='…'` → grep the marker in `uart.log` → the phase's
hypotheses (`DNS`, `N*`, `B`–`F`). Rerun a slice with `RUN_PHASE=<phase>`.

**Lane killed or cut short:** `R` records (`memory.peak`, `hit_max`, signal) and
`qemu-test: lane terminated externally (signal N)` in the output.

**Input lane stuck:** `H4` records from the injector name the scenario step and the missing
marker.

## Troubleshooting

| Knob / tool | Effect |
|---|---|
| `RUN_TIMEOUT=<dur>` | hard bound (`scripts/qemu-test.sh` default `300s`; `ci-os-*` recipes set their own) |
| `RUN_UNTIL_MARKER=1` | stop shortly after the ladder's end marker (default in `qemu-test.sh`) |
| `RUN_PHASE=<phase>` | stop after a phase; on failure print a bounded excerpt of that phase |
| `DEBUG_QEMU=1` | start QEMU halted with a gdb stub on `tcp:localhost:1234` |
| `NEURON_BOOT_FEATURES=trap_symbols,trap_ring` | `sepc` as `name+offset`; last trap frames kept |
| `INIT_LITE_LOG_TOPICS=general,svc-meta,probe` | extra init diagnostics (build-time, baked via `option_env!`) |
| `INIT_LITE_WATCHDOG_TICKS=<n>` | init panics after n cooperative yields (catch a silent spin) |
| `UART_LOG_MAX`, `QEMU_LOG_MAX` | raise the post-run trim caps |
| `tools/uart-filter.py --strip-escape --grep <s> uart.log` | decode `E`-prefixed probe lines, filter lines |
| `just check-markers <uart.log>` | run the chain-marker contract against any log |

| Symptom in `uart.log` | Means |
|---|---|
| no `map kernel segments ok` / `AS: post-satp OK` | kernel mapping or the SATP switch failed; illegal-instruction traps print `sepc/scause/stval` + bytes |
| `!init-lite entry` without `!init-lite ready` | init stopped during service spawn; look for `fatal err=` |
| `init: up <svc>` without `<svc>: ready` | the service started but never served; a hard failure |
| `nxboot: PANIC` | the boot chain died (reset loop) |
| `virtio-blk: device failed (needs reset)` | the device reported failure; a slow device only delays requests |
| `windowd: gpud reply foreign frame …` / `windowd: FAIL gpud reply ate client frame …` | something drained client-surface traffic off windowd's own inbox (`0x49` = `b'I'`, the client envelope's first byte); the desktop then shows at the probe size without input — seen only at 4 harts + virgl (`just start`) |

A black screen on a visible lane is our compositor/gpud path, never the host display stack.
QEMU lanes and a board image build share `build/`; do not run them in parallel.
