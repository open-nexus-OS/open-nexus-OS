---
schema: 1
title: "Run logs and triage"
role: reference
subsystem: devx
sources: [scripts/qemu-test.sh, scripts/qemu-launcher.sh, scripts/build.sh, scripts/hypothesis-log.sh, tools/os2vm.sh, tools/uart-filter.py]
contracts: [RFC-0107, ADR-0063]
verified: 2026-11-24
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

**A lock taken against the order (`smp8--*`, `NEURON_BOOT_FEATURES=lock_rank_check`):** grep
`PANIC` — `KPANIC: lock <class> 0x… taken under <class> 0x…` names the two locks,
`ranked lock held past an IPC syscall` the body, `frame owner` a frame saved twice;
`KINIT: lock class <class> (takes= waited= max_hold_us= max_wait_us=)`; the memory lines:
[../kernel/memory-locks.md](../kernel/memory-locks.md#proof-and-numbers).

**Input lane stuck:** `H4` records from the injector name the scenario step and the missing
marker.

## A slow frame's timeline

A frame that missed its budget is not read from window averages: each process of the frame chain
prints its own slow frames once, when idle (`apphost: hops one (s=…)`, `windowd: hops one (f=…
from=…)`, `gpud: hops one (f=…)` — board only, QEMU runs another GPU path), with a histogram per
window (`<svc>: hops hist (…)`) and the record's cost once (`<svc>: hops cost (…)`; RFC-0108 §hops,
TASK-0373). Stitch them:

```bash
cargo run -q -p nx -- hops build/logs/<run>/uart.log            # per frame: hops, the longest, the gap
cargo run -q -p nx -- hops build/logs/<run>/uart.log --frame 8607
cargo run -q -p nx -- hops build/logs/<run>/uart.log --check    # exit 3: stamps disagree, a line cut
```

Each frame shows every process's hops (`a>b=µs`), its waits (`w=count/µs`), yields, bytes copied
and counts; `gap` is the time no hop names — runnable and not run, IPC between processes, or a
stamp missing. The summary gives p50/p90 per hop for bare, mid and loaded scenes. Unjoined lines
are normal: each process prints only the frames it judged slow.

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
| `QEMU_MEM_PREALLOC=0` | leave the machine's memory lazy on the host (default: preallocated — a first touch that faults on the HOST stalls a hart inside the kernel lock for tens of ms) |
| `tools/uart-filter.py --strip-escape --grep <s> uart.log` | decode `E`-prefixed probe lines, filter lines |
| `just check-markers <uart.log>` | run the chain-marker contract against any log |

| Symptom in `uart.log` | Means |
|---|---|
| no `map kernel segments ok` / `AS: post-satp OK` | kernel mapping or the SATP switch failed; illegal-instruction traps print `sepc/scause/stval` + bytes |
| `!init-lite entry` without `!init-lite ready` | init stopped during service spawn; look for `fatal err=` |
| `init: up <svc>` without `<svc>: ready` | the service started but never served; a hard failure |
| `nxboot: PANIC` | the boot chain died (reset loop) |
| `KPGF hart=0x<n> sepc= stval= scause= ra= sp= a7= a0= a1= a2= sstatus= satp= pid=`, then `PANIC: <file>:<line hex>: <text>`, `PANIC hart=0x<n> ra=`, `PANIC: end` | a kernel page fault (`KEXC`: another exception, `KILL`: illegal instruction): `sepc` where, `stval` the address, `a7` the syscall, `satp` the space. One hart speaks; `PANIC: end` closes. No fault line: a `panic!`. Board: reset without a power cut, the next boot keeps the words (`nxboot: rescue ok (seq=<dead boot>`, `trace-all.log`). Proof: `just ci-os-kfault` |
| `KSELFTEST: kernel fault fixture armed (seq=1 in_ms=)` | the fixture (`selftest_kernel_fault`) faults on a trace's first boot; an ordinary OS build before `just board-image` |
| `KINIT: ipc call rt by hart (fastest_us=[..] mean_us=[..] n=[..])` | `ipc_call` round trips by calling hart: TASK-0330 P2's 64 us budget per hart |
| `KINIT: bkl long wait <us> cpuN for=<id> deferred=<n> passed_over=<n>` | a hart waited above 10 ms for the kernel lock: what it wanted it for (`diag/lock_share` id: a syscall number, 80 timer, 81 IPI, 82 external, 83 fault, 84 idle), how many turns it stood back for CPU 0, how often the lock changed hands meanwhile — `passed_over=0` means ONE long holder |
| `KINIT: bkl long hold <us> cpuN by=<id>` | that holder: any hold above 10 ms, also a fault, the timer or the scheduling loop (the ecall budget sees syscalls only) |
| `KINIT: bkl stall <ms>ms holder=cpuN by=<id> idle=0x<mask> act=[…]` | a hold named while it lasts: a hart that waited 50 ms for the kernel lock prints the holder and the first eight CPUs' activity tags, once per 50 ms. A tag is hex, low byte the class, above it the detail: `1` in a trap (syscall number or cause), `2`/`3` a create's zero or copy, `4` idle, `6` spinning for a lock (detail: its address without the top byte), `7` waiting for shootdown acks (epoch), `8` waiting for the kernel lock (what for), `9` the idle loop under the lock (1 taken, 2 picked, 3 dispatch, 4 deadline walk, 5 events, 6 witness, 7 a stashed line, 8 its own deadline), `a` arming the timer, `b` an unmap (1 clear, 2 forget, 3 reclaim). Read the holder's tag first: what it waits for, or the stage it stands in; then the tag of the hart it waits on |
| `KINIT: fault long step=<fill\|pressure\|exit> us= pid= va=` | which step of a page fault held the lock above 10 ms; `fill` on a multi-hart QEMU lane with `QEMU_MEM_PREALLOC=0` is the host's page fault ([measurement](../board/measurements/2026-11-11-n-harts/README.md)) |
| `KINIT: boot stack (used= left= of=)`, then a panic `boot stack: <n> bytes left` | the boot hart entered its first idle turn with less than 32 KiB of its stack: a frame in `kmain` grew (a large value built on the stack — find it with `llvm-objdump -d` on an unstripped kernel, the functions whose prologue subtracts a `lui` constant). Without the check the symptom was a `KPGF` with `stval=0x10` right after `KINIT: cpuN sched loop`: the kernel's statics below the stack overwritten |
| `virtio-blk: device failed (needs reset)` | the device reported failure; a slow device only delays requests |
| `windowd: gpud reply foreign frame …` / `windowd: FAIL gpud reply ate client frame …` | something drained client-surface traffic off windowd's own inbox (`0x49` = `b'I'`, the client envelope's first byte); the desktop then shows at the probe size without input — seen only at 4 harts + virgl (`just start`) |

A black screen on a visible lane is our compositor/gpud path, never the host display stack.
QEMU lanes and a board image build share `build/`; do not run them in parallel.
