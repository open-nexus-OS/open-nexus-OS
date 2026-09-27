---
title: TASK-0327B Board proof lane: the marker ladder from the boot trace on the disk (RFC-0107) or the debug UART, `board-*` profiles, opt-in in `test-all`
status: In Progress (P0 done 2026-09-27 — recut: no USB-UART adapter is at the desk, so the ladder's channel is the boot trace on the boot disk (RFC-0107 seeded), the serial log where an adapter exists; P1 done 2026-09-27 — the loader's trace: kept on the disk in every lane, read back with `nx image trace` and, on the board, `just board-log`; next the board attempt with TASK-0260B's FIT; seeded 2026-09-21 at Block 0 P0)
owner: @devx @runtime
created: 2026-09-21
depends-on:
  - tasks/TASK-0327-board-developer-tooling-flash-serial-one-command.md
follow-up-tasks: []
links:
  - Execution order (Block 0 T3 / Block 1 B1.6): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - The harness this mirrors: scripts/qemu-test.sh, docs/testing/README.md, docs/testing/proof-manifest.md
  - Profiles SSOT: source/apps/selftest-client/proof-manifest/profiles/harness.toml
  - Contract: docs/rfcs/RFC-0107-boot-trace-console-markers-on-the-boot-disk.md (the boot trace)
---

## RECUT 2026-09-27 — P0: the ladder's channel is the boot trace on the disk (RFC-0107)

No USB-UART adapter is at the desk, and the board's first boots are exactly the ones that stop
early. Measured (a survey of the tree, 2026-09-27): the kernel keeps no copy of its console
output and has no syscall to read one (ADR-0040 ruled out a replay buffer); logd sees structured
logs only, never the markers; nxboot writes straight to the UART and holds no buffer, but writes
the boot disk already (the BSB); nothing in the tree persists console text. The boot disk is the
one medium every stage from the loader on can write and the host can read afterwards — TASK-0260
P2 read the eMMC back over adb.

### End state (binding)

- The console text of each boot is kept on the boot disk (RFC-0107): a `trace` partition of eight
  1 MiB slots, one per boot, the loader's region then the OS region, header last, fail-closed
  readers.
- The loader captures everything it prints and writes it at its milestones (the disk found, the
  image verified, the jump); it hands its slot to the OS in `/chosen/nexus,trace`.
- The kernel keeps a fixed ring of every console byte and a capability-gated read syscall; one OS
  writer appends the ring to the slot through the block owner's gate (Phase 2).
- `nx image trace` reads a trace from an image or a partition dump; `just board-log` pulls the
  board's trace from its stock system over adb into `build/logs/board--<ts>/uart.log`, and the
  proof manifest judges it like a QEMU `uart.log`.
- `just board-test PROFILE=…` (this ledger's original goal) runs on either channel: the trace
  after a boot, or the serial log where an adapter exists.

### Packages

- **P0 — the recut and RFC-0107 ✅ 2026-09-27.**
- **P1 — the loader's trace ✅ 2026-09-27.**
  - `storage::trace`, the record codec shared by the loader, the OS writer (P2) and the readers:
    - the header counts only with its magic, version, CRC and in-bounds fields, and only in its
      own slot;
    - the slot choice is the highest valid seq + 1, round a ring of eight;
    - the writer clears a taken-over slot's header first, then writes its text, then the header
      last; the region is cut at 64 KiB and the overflow flagged.
  - The layout gains the `trace` partition (8 MiB, `NEXUS-TRACE-v1`), after `data`.
  - nxboot captures every byte it prints, from its first line (64 KiB static; what does not fit
    is flagged).
    - It opens the trace once the boot disk is known and prints `nxboot: trace slot=<n>
      seq=<m>`, or `nxboot: trace none (<why>)`.
    - It writes at three points: the disk found, just before the jump, and on every terminal
      failure (the PANIC line included).
    - It hands the slot to the OS in `/chosen/nexus,trace` (`"<slot's first LBA> <seq>"`).
    - A failed write prints `nxboot: trace FAIL (write)`, and the boot goes on.
  - `nx image trace --image <disk | partition dump> [--all | --since <seq>] [--loader] [--out F]
    [--json]`.
  - `just board-log` (`scripts/board-log.sh`):
    - finds the partition over adb by its GPT name, as the stock kernel parsed it, and checks its
      size;
    - pulls it binary-safe (`adb exec-out dd`) and reads it with `nx`;
    - writes `build/logs/board--<ts>/uart.log` (the latest boot), `trace-all.log` and
      `trace.part`, and points `latest-board` at them;
    - exits 3 when no stock system is on adb, 4 when there is no trace partition, 5 when no boot
      was kept.
  - The harness's trace contract (`scripts/qemu-test.sh`, every lane):
    - The run's boots are those from the run's first `nxboot: trace … seq=N` on; a kept disk also
      holds earlier launches' boots.
    - Their loader regions must hold exactly the UART's `nxboot:` lines.
    - `trace-loader.txt` lands in the log directory.
    - A direct-kernel boot is skipped, and the skip is printed.
  - **Proof — host:**
    - `storage` trace: 6 tests. Two use a fault-injecting disk: a takeover torn before its header
      shows no mixed record, and a milestone torn in its text keeps the last complete one.
    - `nx` `image_trace_cli`: 4 tests.
    - 10 mutations each killed: CRC, bounds, ring, takeover clear, header order, slot field,
      sort, `--loader`, `--since`, GPT lookup.
  - **Proof — the harness contract, run standalone** against the smp1 lane's log, 5 cases:
    - the lane's own trace: PASS;
    - the trace missing the jump line: FAIL, with the diff;
    - no trace line in the UART: FAIL;
    - a kept disk holding an earlier launch's boot: PASS from seq 2 (the `--all` comparison it
      replaced FAILs there);
    - a direct kernel: SKIP.
  - **Proof — QEMU:** `ci-os-smp1` `[PASS] trace contract: the loader's 5 lines of 1 boot(s) …`,
    the trace byte-equal to the UART's loader lines. `just test-all` EXIT=0: 12 QEMU runs, each `[PASS] trace contract`, 21 boots kept, among them the reset lane's 3 boots in one run and ota-fallback's 4 (23 loader lines, the fallback decision included); the sdhci lane wrote its trace through the SDHCI core in PIO.
  - **Proof — board:** `just board-log` against the stock system over adb. The lookup lists our
    nine partitions by their GPT names. The disk written in TASK-0260 P2 predates the partition,
    so the exit is 4 with the command that fixes it.
  - **Proof — `board-log` against a stand-in stock system** (an `adb` that serves ota-fallback's
    disk as the eMMC, CRLF included):
    - `uart.log` is the disk's latest boot and `trace-all.log` its four boots, both byte for byte;
    - a zeroed trace (a freshly flashed disk) exits 5;
    - no stock system exits 3.
- **P2 — the OS trace.** The kernel ring + read syscall (ADR-0040 amended), the OS writer and its
  gate, the whole-log contract in every lane.
- **P3 — the RAM rescue**, only if the board keeps DRAM across a reset (measured first).
- **P4 — the board ladder**, with TASK-0260B's FIT: the first eMMC boot attempt read back with
  `just board-log`, `markers/board.toml`, the `board-*` profiles, `just board-test`.

## Origin

B part of `TASK-0327`: the tooling lets a developer flash and watch the board; this ledger makes a
board boot a PROOF — the same marker manifest, read over the debug UART instead of QEMU's stdout.

## Context

`scripts/qemu-test.sh` boots a profile, records `uart.log`, and `nexus-proof-manifest verify-uart`
judges the marker ladder (expectations suppress surprise, REQUIREs live in the harness). A board log is
the same text on a serial port; nothing in the harness reads one today, and no profile names a board.

## Goal

`just board-test PROFILE=board-headless|board-visible`: flash (`TASK-0327`'s recipe) → reset (SBI
SRST from our OS when it is up; a manual power cycle otherwise, with the lane waiting on the serial
banner) → capture the serial log into `build/logs/board--<ts>/uart.log` → `verify-uart` with the board
profile → the QEMU-only proofs replaced by their board equivalents (the pixel proof by the display
controller's marker + `board-visual: desktop`; input by `board-visual: typed`). Profiles
`board-headless`/`board-visible` `extends` the QEMU ones (env: no QEMU args; `runner =
scripts/board-test.sh`). In `just test-all` only under `NEXUS_BOARD=1` — opt-in, never silent, and the
report says "board lane skipped" otherwise.

## Non-Goals

Automated power control (no relay), a second board, the 2-VM network lane (`TASK-0331`).

## Constraints / invariants

- One marker manifest for QEMU and board; a `board-visual:` marker is declared like any other and
  written only by `just board-ack` from a human at the monitor.
- Wait loops self-terminate (timeout cap + failure signatures: `PANIC`, `alloc_error`, `FAIL` gate).
- No fake success: a lane that never saw the serial banner fails, it does not time out green.

**Block 1 markers from M1 (TASK-0286, added 2026-09-24).** The board ladder carries the memory
markers QEMU already requires: `KINIT: mm frames (banks=2 …)` (two banks around the 2 GiB hole),
`KSELFTEST: vmo runs ok (runs=… deny=4)`, `KSELFTEST: vmo reach ok (…)` (TASK-0246 P1: the
window is the top of the first bank — on the board that is bank 0 below 2 GiB),
`KINIT: user cache maintenance zicbom block=64`,
`SELFTEST: dma buffer ok (device=… block=64 runs=…)` and `KSELFTEST: mm frames (…)` with
`exhausted=0`. YELLOW: user-mode `cbo.*` needs `menvcfg.CBCFE`/`CBIE` from the firmware — the
vendor OpenSBI is 1.0; if the `dma buffer` line traps, the FIT's own OpenSBI (≥ 1.3, TASK-0260B)
is the fix, not a kernel workaround.

## Definition of Done

`[PASS] board-headless` and `[PASS] board-visible` on the desk board with Block 1's markers; the
profiles registered in `harness.toml`; `docs/testing/README.md` documents the lane and the opt-in.
