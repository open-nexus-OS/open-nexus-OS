---
title: TASK-0327B Board proof lane: the marker ladder from the boot trace on the disk (RFC-0107) or the debug UART, `board-*` profiles, opt-in in `test-all`
status: In Progress (P0 done 2026-09-27 — recut: no USB-UART adapter is at the desk, so the ladder's channel is the boot trace on the boot disk (RFC-0107 seeded), the serial log where an adapter exists; P1 done 2026-09-27 — the loader's trace: kept on the disk in every lane, read back with `nx image trace` and, on the board, `just board-log` (the board's first eMMC boot read with it, TASK-0260B P2); P2 done 2026-09-28 — the OS trace; P3 done 2026-09-28 — the RAM rescue, proven on QEMU, and the board measured: its reset scrubs DRAM, so the board's early kernel needs the UART (P4's serial ladder) or a LED ladder; seeded 2026-09-21 at Block 0 P0)
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
  - **Proof — on hardware, 2026-09-27 (TASK-0260B P2):** the board's first eMMC boot read back
    with `just board-log`, with no serial adapter attached.
    - The first attempts kept no boot, which correctly placed the stop before the loader. It
      was the firmware: our tree's CLINT.
    - Once the tree was fixed, the trace held the loader's six lines, from
      `nxboot: platform=bananapi,bpi-f3 tree=0x268000 …` to `nxboot: jump slot=a base=0x400000`.
  - **Proof — `board-log` against a stand-in stock system** (an `adb` that serves ota-fallback's
    disk as the eMMC, CRLF included):
    - `uart.log` is the disk's latest boot and `trace-all.log` its four boots, both byte for byte;
    - a zeroed trace (a freshly flashed disk) exits 5;
    - no stock system exits 3.
- **P2 — the OS trace** (2026-09-28; recut from "ring + read syscall + a gated writer" while
  building — the facts: `console_write_byte` is the one hook every console path passes; the
  kernel already exposes static memory read-only through a `VmoRo` init pins by name; `blkd`
  is the first process that can write the disk, and `init: ready` prints before it runs).
  - `nexus-console-ring` (new lib, `no_std`, zero deps): the ring's one layout — a header page
    (`NXCRING1`, version, data size, the head) and 128 KiB of data — and the reader, which
    counts what the writer overwrote before a read and drops a prefix overwritten during the
    copy. 4 host tests.
  - The kernel (`hal/console_ring.rs`): static, page-aligned pages in `.bss`; `init()` writes
    the header after `zero_bss`; `console_write_byte` records then emits under one
    `SpinIrqLock` (a bounded wait, so a stopped holder never silences the console); the ring
    goes to init read-only in slot 3 (`INIT_CONSOLE_RING_SLOT`), like the tree.
  - The topology: `NamedSlot::ConsoleRing` and blkd's `CONSOLE_RING` + `TRACE_TIMER` slots;
    `test_reject_a_second_console_ring_reader` holds blkd as the one reader. init pins the ring
    (`init: console ring grant ok svc=blkd`).
  - `storage::trace::OsTrace`: appends to this boot's slot only — the loader's handoff `lba`
    must be the place the ring rule gives `seq` in the partition, and the header there that
    boot's record; text first, header last; no per-call allocation (a partial last sector kept
    in the writer); overflow flagged. 4 host tests, two with a fault-injecting disk.
  - `blkd` keeps the region itself (`trace_os.rs`): the `trace` partition gets NO selector (no
    client can name it, `parts=7` stays), the serve loop runs on a waitset over the server
    endpoint and a periodic 250 ms kernel timer on the declared pair (RFC-0093 §7), a tick keeps
    at most 64 KiB in 4 KiB appends; `blkd: trace os ok (slot=<n> seq=<m>)` on the first write,
    `blkd: trace os none (<why>)` once at start.
  - `nx image trace --seq <n> --os`; the harness's OS contract (`tools/trace_os_contract.py`):
    each boot's OS region is the UART's text after its jump line byte for byte; a boot that
    stopped before the block owner ran keeps none (a fallback lane's trial boots — allowed for
    every boot but the run's last, whose region must reach `stage: platform`).
  - RFC-0107 §Write authority / Phase 2 and ADR-0040 amended.
  - **Proof — host:** `nexus-console-ring` 4 tests (every byte once across the wrap; bytes lost before a read counted; a prefix overwritten during the copy dropped; another ring's header refused); `storage` trace 10 (the loader's 6, the OS writer's 4: sectors + a reopen, other boots' slots and a stray record, text past the region and text the ring lost, a torn append); `nexus-service-topology` 12 (blkd the one ring reader); `nx` image_trace_cli 4 (`--seq`, `--os`). Kernel cross-build and `build-os-workspace` with 0 warnings.
  - **Proof — QEMU:** `ci-os-smp1`: `init: console ring grant ok svc=blkd`, `blkd: trace os ok (slot=0 seq=1)`, `[PASS] trace contract (os): 64485 bytes … byte-equal to the UART` (through `stage: platform` to the UI selftests). `just test-all`: EXIT=0 in 52 min — 12 QEMU runs, every loader AND OS trace contract green, 21 boots kept, 912 240 bytes of kernel console text byte-equal to the UART (the reset lane's 3 boots, ota-fallback's 4 with its two trial boots that stop before the block owner and keep none); 3614 host tests; 0 errors..
  - **Proof — mutations:** 8 host mutations each killed — the reader's lost count, its during-copy skip, its version check; the writer's LBA check, its seq/slot check, its overflow flag, its region bound; a second ring reader. The harness OS contract against the smp1 lane's disk and log: PASS; a flipped byte → refused at byte 100 with both texts; an emptied OS region → "keeps no OS text"; a region cut before `stage: platform` → refused.
- **P3 — the RAM rescue** (2026-09-28; recut: the rescue IS the measurement — the stock
  system's kernel forbids RAM reads, so the only reader that runs before the next kernel is the
  loader). Trigger: the first P2 board boot kept the loader's six lines and NO OS text — the
  kernel started and nothing reached the block owner's writer; whether the kernel stopped early
  or the owner's first ADMA2 write failed is exactly what the ring in RAM answers.
  - `nexus-console-ring`: the header carries the boot's trace `seq` (offset 24, stamped by the
    kernel from `/chosen/nexus,trace`); `find(window, seq)` scans page starts for an intact
    header with that number; `Reader::from(position)`; `RingBytes` reads a ring in memory.
  - nxboot: after its trace is open and BEFORE the image is loaded over the window, `rescue`
    finds the previous boot's ring, opens that boot's slot (`LoaderTrace::previous`,
    `OsTrace::open` — the slot must be that boot's record) and appends what the ring holds past
    the OS text already kept (`OsTrace::rescue`, flag `OS_RESCUED`; a lapped gap sets overflow).
    `nxboot: rescue ok (seq=<n>[ unstamped] bytes=<b> lost=<l>)` / `rescue none (first boot |
    no ring in ram | ring of seq=<m> in ram, not seq=<n> | slot of seq=<n> not its record |
    unstamped ring, seq=<n> has text)` / `rescue FAIL (write)`. An unstamped ring (the kernel
    stopped before it read the tree) is taken as the previous boot's — every kernel start
    zeroes `.bss`, so a ring in the window is the most recent run's — when that boot kept no
    OS text yet; the first board cycle showed the distinction was needed (the loader could not
    tell a scrubbed DRAM from an early-dead kernel).
  - `nx image trace --json` shows `os_rescued`; `just board-log` says "rescued from RAM by the
    next loader".
  - `nx image flash-plan` records the trace partition and `board-flash.sh --verify` reads it as
    zero: it is written by every boot, and a verify after a boot lied about `nxdisk1`.
  - Found on the way: `just board-image` right after `just build-os-workspace` shipped a stale
    kernel — the OS build never produced the flat `neuron-boot.bin`; only the QEMU launcher did,
    at a lane's start, so the board's image was as fresh as the last lane, not the last build
    (the build id did not change after a kernel edit). `scripts/build.sh` now emits the flat
    image from the ELF it just built; the launcher's step stays as the freshness guard.
  - **Proof — host:** `nexus-console-ring` 6 tests (`find_ring`/`find`, `Reader::from`, a lapped ring's gap, another boot's, an unstamped and a missing ring); `storage` trace 11 (the rescue of the tail the owner never wrote, a flagged gap, no previous boot on the first); `nx` image_flash_cli 3 (the plan's trace entry) + image_trace_cli 4; nxboot cross-build and `build-os-workspace` with 0 warnings.
  - **Proof — mutations:** 7 host mutations each killed — `find` ignoring the stamp, accepting seq 0 (found survivable, a test added), scanning every byte, `Reader::from` at 0, a rescue that does not mark, `previous()` naming the wrong slot.
  - **Proof — QEMU:** `ci-os-reset`: boot 2 `nxboot: rescue ok (seq=1 bytes=643 lost=0)`, boot 3 `rescue ok (seq=2 bytes=245 lost=0)`, the OS contract byte-equal with the rescued tails (116 828 bytes of 3 boots); `ci-os-smp1`: `rescue none (first boot)`. `just test-all`: EXIT=0 in 54 min — 12 QEMU runs, every loader and OS trace contract green, 21 boots, 929 531 bytes of kernel console text byte-equal to the UART, 9 rescues across the reset and OTA lanes (the reset lane's 643 and 245 bytes the same as in the single run; ota-fallback's two early-dead trial kernels rescued whole, 5944 bytes each, and their text byte-equal too); 3617 host tests; 0 errors..
  - **Proof — board (the measurement):** three flash cycles on the desk board, each read back exact — with `--verify` reading the trace partition as zero after boots (the fix proven) — and read with `just board-log` without a serial adapter: the first cycle's loader could not tell a scrubbed DRAM from an early-dead kernel (`no ring of seq=N in ram`), the second reported `no ring in ram` for stamped and unstamped rings alike, and the third, with the loader's own probe page, `dram probe lost` on every reset — the board's reset path scrubs DRAM (`docs/board/measurements/2026-09-28-dram-retention/`). Every board boot's loader ran to the verified jump; none kept OS text.
  - **Verdict:** Phase 3 works where DRAM survives a reset (QEMU's reset lane; proven) and cannot on this board. What is left for the board's early kernel without an adapter is the user LED (`sys-led`, GPIO 96 of `k1x-gpio` at 0xd4019000, measured) — a ladder of a few bits — or the debug UART, whose ladder (`just board-serial`, P4) is built.
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
