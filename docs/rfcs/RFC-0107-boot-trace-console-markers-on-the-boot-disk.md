# RFC-0107: The boot trace — console markers persisted on the boot disk, read without a serial adapter

- Status: In Progress (Phase 0 + 1 ✅ 2026-09-27, Phase 2 ✅ 2026-09-28 — TASK-0327B P0–P2; Phase 3
  waits for the board's DRAM measurement; seeded 2026-09-27)
- Owners: @runtime @devx
- Created: 2026-09-27
- Last Updated: 2026-09-27
- Links:
  - Tasks: `tasks/TASK-0327B-board-proof-lane-serial-ladder-profiles.md` (execution + proof)
  - ADRs: `docs/adr/0040-*` (logging policy — amended in Phase 2: the kernel keeps a fixed console
    ring), `docs/adr/0059-*` (nxboot and its handoff), `docs/adr/0066-*` (the board's boot chain)
  - Related RFCs: `docs/rfcs/RFC-0089-*` (§2 the disk layout the `trace` partition joins),
    `docs/rfcs/RFC-0098-*` (C8 proof markers), `docs/rfcs/RFC-0011-*` (logd's structured journal —
    a different thing), `docs/rfcs/RFC-0031-*` (crash dumps — a different thing)

## Status at a Glance

- **Phase 0 (the contract)**: ✅ 2026-09-27 — TASK-0327B P0
- **Phase 1 (the partition, the record, the loader writes, the host reads; QEMU proof)**: ✅
  2026-09-27 — TASK-0327B P1 (the trace contract in every lane; `just board-log` against the
  board's stock system)
- **Phase 2 (the kernel's console ring, read-only to the block owner, which keeps it; the
  whole ladder persisted)**: ✅ 2026-09-28 — TASK-0327B P2 (no read syscall: the ring is
  exposed like the tree; every lane's contract compares each boot's OS text with the UART)
- **Phase 3 (a crashed boot's ring rescued from RAM by the next loader)**: ✅ 2026-09-28 built and proven on QEMU's reset lane — TASK-0327B P3; **measured on the board: the reset lets DRAM decay** (`nxboot: dram probe lost` three cycles, then one `kept` with a rescued, bit-flipped kernel console — `docs/board/measurements/2026-09-28-dram-retention/`), so the rescue was a lucky witness there — until the ring was written back per byte (`cbo.flush`, 2026-09-28): since then every board rescue read `lost=0` and ended at the kernel's last byte, and the rescue carried the diagnosis of TASK-0260B P3 (a warm reset keeps DRAM on this board; a cold one does not)

Definition: "Complete" = the contract below is implemented and its proof gates are green on
QEMU and on the board.

## Scope boundaries (anti-drift)

- **This RFC owns**: the `trace` partition's format (slots, header, regions), who writes which
  region, how the loader hands its slot to the OS, the reader contract, and — Phase 2 — the
  kernel console ring and the syscall that reads it.
- **This RFC does NOT own**: logd's structured journal and its evidence spill (RFC-0011,
  RFC-0087), crash dumps (RFC-0031), the serial capture path (TASK-0327/0327B keep it: a board
  with a USB-UART adapter is judged on its serial log exactly as before), the marker vocabulary
  (the proof manifest and RFC-0098 C8).

## Context

On QEMU the proof medium is the console text (`uart.log`): the marker ladder is judged on it.
The board's ladder (TASK-0327B) assumed a USB-UART adapter on the debug UART; none is present,
and a board boot that stops early leaves nothing a host can read. The boot disk is the one medium
every stage from the loader on can write and the host can read afterwards: TASK-0260 P2 wrote
the eMMC and read it back byte for byte from the stock system over adb. So the console text of
each boot is kept on the boot disk, and the host reads it back — the same text, a different
channel.

## Goals

- The console text of each boot — the loader's, then the kernel's and every service's — kept on
  the boot disk, one record per boot, the last eight boots at a time.
- Read on the host from an image (`nx image trace`) or from the board's stock system
  (`just board-log`), and judged by the proof manifest like a `uart.log`.
- The same code path on QEMU and the board, proven on QEMU first: a lane's trace equals its
  `uart.log`.

## Non-Goals

- A structured log, a query API, rotation policies beyond eight boot slots.
- Remote retrieval while the OS runs (the trace is read after the boot, from another system or
  an image).
- Replacing the serial capture where an adapter exists.

## Constraints / invariants (hard requirements)

- **Determinism**: the trace holds exactly the bytes the console received, in order; the QEMU
  proof compares them with `uart.log`.
- **No fake success**: a writer marks its region complete only after its last bytes are on the
  disk; a reader shows an incomplete region as incomplete, never as a finished boot.
- **Bounded resources**: fixed slots and regions; a writer that reaches its region's end stops and
  sets the region's overflow flag; the loader's capture buffer and the kernel ring are static.
- **Fail-closed reading**: a slot counts only with its magic, its version, a matching header CRC
  and lengths inside their regions; anything else is shown as absent or damaged, never guessed.
- **Write authority**: before the OS, only the loader; in the OS, only the block owner itself
  (amended 2026-09-28, Phase 2: the owner keeps the OS region, so the `trace` partition has no
  selector and no client can name it — nothing to gate). The kernel's console ring reaches
  exactly one reader: the topology declares its slot for the block owner alone, held by a
  `test_reject_*`.
- **No new secret exposure**: secrets never reach the console (the standing rule); the trace adds
  no reader beyond physical access to the disk.

## Proposed design

### Contract / interface (normative)

- **Partition**: `trace`, type `NEXUS-TRACE-v1`, 8 MiB, appended after `data` in the layout SSOT
  (RFC-0089 §2). Eight slots of 1 MiB.
- **Slot**: sector 0 is the header; the loader's region is the next 64 KiB; the OS region is the
  rest of the slot (1 MiB − 512 B − 64 KiB).
- **Header** (512 bytes, little-endian): `magic` = `NXTRACE1` (8 bytes); `version` u32 = 1;
  `slot` u32; `seq` u64 (the boot's sequence number, from 1); `loader_len` u32 and `os_len` u32
  (bytes used in each region); `flags` u32 (bit 0 loader complete, bit 1 loader overflow, bit 2 OS
  complete, bit 3 OS overflow); `crc32` (IEEE) of bytes 0..40 at 40; the rest zero.
- **Slot choice** (the loader, once it knows the boot disk): read the eight headers (a header
  counts only in its own slot); `seq` = the highest valid `seq` + 1 (1 on an empty partition);
  `slot` = (`seq` − 1) mod 8. Taking a slot over clears its old header first; after that the slot
  is written text first, header last. The loader captures every byte it prints from its first
  line (a static 64 KiB buffer; what does not fit sets the overflow flag) and writes its region
  and then the header at its milestones: the disk found, just before the jump (complete), and on
  every terminal failure (complete, the `PANIC` line included). Its console says which slot it
  took (`nxboot: trace slot=<n> seq=<m>`), or why there is none (`nxboot: trace none (<why>)`);
  a failed write prints `nxboot: trace FAIL (write)` and the boot goes on.
- **Handoff**: the loader writes `/chosen/nexus,trace` = `"<slot's first LBA on the disk> <seq>"`;
  the OS writer (Phase 2) appends into that slot's OS region, checks the header's `seq`, and writes
  the header last each time.
- **Readers**: `nx image trace --image <disk or partition dump>` prints the latest boot's text
  (the loader's region, then the OS region), `--all` every valid slot by `seq`, `--since <seq>`
  one run's boots, `--loader` the loader's regions only, `--out` the bytes exactly, `--json` each
  boot's `seq`, slot, lengths and flags. `just board-log` finds the partition on the board's eMMC
  by its GPT name as the stock system's kernel parsed it, checks its size, pulls it
  (`adb exec-out dd`, binary-safe) and writes the latest boot's text as
  `build/logs/board--<ts>/uart.log` (every kept boot: `trace-all.log`), where the proof manifest
  judges it; it exits 4 when the eMMC has no trace partition and 5 when the trace keeps no boot
  (since the disk was written, no boot reached the loader's disk step: the chain stopped before
  nxboot, or nxboot found no boot disk — before that step only the console witnesses a boot,
  which is what Phase 3 addresses).
- **The harness's contract** (every QEMU lane): the run's boots are the trace's boots from the
  run's first `nxboot: trace … seq=N` on (a kept disk also holds the boots of earlier launches);
  their loader regions hold exactly the UART's `nxboot:` lines. A direct-kernel boot has no
  loader and skips it, saying so. Phase 2 adds: each boot's OS region is the UART's text after
  that boot's jump line, byte for byte (a prefix); a boot that stopped before the block owner
  ran keeps none, which is allowed for every boot but the run's last, whose region must reach
  `stage: platform` (`tools/trace_os_contract.py`).
- **Readers, Phase 2**: `nx image trace --seq <n>` one boot, `--os` the OS text only.
- **Phase 2** (amended 2026-09-28 from the design below; TASK-0327B P2): the kernel keeps a
  fixed static ring of every byte it sends to the console — including bytes before the console
  is known — in whole pages of its own image (`nexus-console-ring` is the one layout: a header
  page with magic `NXCRING1`, version, the data size and the head, then 128 KiB of data; the head
  counts every byte ever written). Every console path passes one hook, which records the byte
  and emits it under one lock, so the ring holds the bytes in the order the UART receives them.
  - **No read syscall**: the kernel exposes the ring read-only, the way it exposes the tree —
    a `VmoRo` in init's slot 3 (`INIT_CONSOLE_RING_SLOT`), which init pins into the block owner's
    declared `ConsoleRing` slot and nowhere else.
  - **The block owner is the OS writer**: it holds the disk, so it is the first process that can
    write it, and a diagnostics channel depends on as little as possible. Paced by a periodic
    kernel timer on a declared notify endpoint (RFC-0093 §7, 250 ms) beside its server endpoint,
    it appends what the ring holds to this boot's slot (`/chosen/nexus,trace`), text first,
    header last, never taking a slot over: the slot must already be this boot's record, at the
    place the ring rule gives its number. What the ring overwrote before it was read sets the OS
    overflow flag. Its console says `blkd: trace os ok (slot=<n> seq=<m>)` on its first write, or
    `blkd: trace os none (<why>)` once at start.
  - The trace lags the console by at most one period; a boot that stops in the kernel before the
    block owner runs leaves no OS text (Phase 3's case).
- **Phase 3** (amended 2026-09-28, TASK-0327B P3): the ring is where the kernel image put it —
  static pages in the image's `.bss`, inside the kernel window — and the kernel stamps the
  header with the boot's trace sequence number (`/chosen/nexus,trace`). On a real cache the
  ring is write-back memory and a reset drops dirty lines, so a rescued ring ended lines
  before the kernel did (measured 2026-09-28: two complete rings, `lost=0`, cut at the same
  byte while the LED showed the kernel further on); the kernel therefore writes every
  completed cache block, every line end and the header back with Zicbom `cbo.flush` (the
  block size from the tree; QEMU's tree names Zicbom too, a tree without it keeps the ring
  as it was). The rescue reads what the kernel wrote up to its last line, not up to its last
  eviction. The next loader, once
  it knows the disk and BEFORE it loads its image over the window, scans the window's page
  starts for a ring whose header is intact and stamped with the previous boot's number. What it
  finds past the OS text the block owner had kept goes into that boot's OS region, marked
  `OS_RESCUED` (bit 4); a gap the ring had overwritten sets the overflow flag too. Every kernel
  start zeroes its `.bss`, so a ring in the window is the most recent kernel run's: one stamped
  with the previous boot's number is that boot's, and an unstamped one (the kernel stopped
  before it read the tree — the case the rescue exists for) is still that run's, taken as long
  as nothing was kept for that boot yet. Its console says `nxboot: rescue ok (seq=<n>[ unstamped]
  bytes=<b> lost=<l>)`, or `nxboot: rescue none (<why>)`: `first boot`, `no ring in ram` (the
  DRAM did not keep it), `ring of seq=<m> in ram, not seq=<n>`, `slot of seq=<n> not its
  record`, `unstamped ring, seq=<n> has text`. The rescue IS the
  DRAM measurement: the stock system's kernel forbids RAM reads (`/dev/mem` is strict), so the
  only reader that runs before the next kernel is the loader. On QEMU a fresh launch has no ring
  (zeroed RAM) and the reset lane's later boots find their predecessors'; on the board the first
  rescue says whether DDR training keeps the content.
  - Not a boot dependency: a failed rescue write prints `nxboot: rescue FAIL (write)` and the
    boot goes on.

### Phases / milestones (contract-level)

- **Phase 1**: the partition and the record codec (`storage::trace`), the loader's capture and
  writes, `nx image trace`, the harness's trace contract (every lane: the loader lines of
  `uart.log` equal the loader regions of the trace, boot by boot).
- **Phase 2**: the kernel ring, read-only to the block owner, which keeps it; the whole `uart.log`
  in the trace.
- **Phase 3**: the RAM rescue by the next loader — the measurement of DRAM retention included.

## Security considerations

- **Threat model**: a writer other than the loader or the trace writer corrupting or forging a
  record; a crafted partition making the host reader misbehave; a crafted record making the
  loader choose a wrong slot.
- **Mitigations**: the block owner's gate (one writer; `test_reject_*`); the reader's bounds and
  CRC checks before any byte is used; the loader treats any invalid header as absent (it only
  picks a slot, never trusts a record's text).
- **Open risks**: the trace is plain text on the disk — whoever holds the disk reads it, as they
  read the UART.

## Failure model (normative)

- The disk has no `trace` partition: the loader says so on the console and boots on.
- A write fails: the loader says so and boots on; the trace is diagnostics, never a boot
  dependency.
- A boot stops between two milestones: its slot shows the loader's last complete milestone.
  The loader's waits are bounded (a stop is a `PANIC` + reset, kept in the trace); a Rust panic
  in the loader itself (a bug) prints its `PANIC` line on the UART only, and the slot keeps the
  last milestone.

## Proof / validation strategy (required)

### Proof (Host)

- `storage::trace`: round trip; the slot choice (empty, wrap after eight, a damaged header);
  refusals of a bad CRC, out-of-bounds lengths and slots, a valid header in another slot's place.
- The loader's writer against a disk whose writes fail at a chosen point: a takeover torn before
  its header shows no mixed record; a milestone torn in its text keeps the last complete one.
- `nx image trace` against an image built from the layout and against a dump of its partition:
  the latest boot, `--all`, `--since`, `--loader`, the ring of eight, a damaged header and a
  forged length dropped, the refusals.

### Proof (OS/QEMU)

- The harness's trace contract in every lane (Phase 1: the loader's lines; Phase 2: the whole log).

### Proof (board)

- `just board-log` after an eMMC boot attempt shows how far the boot came.
  - Done 2026-09-27 (TASK-0260B P2, `docs/board/measurements/2026-09-27-first-emmc-boot/`).
  - The first attempts kept no boot: the chain stopped before the loader, in the firmware.
  - Once the board tree was fixed, the trace held the loader's six lines from the board, from
    `nxboot: platform=bananapi,bpi-f3 tree=0x268000 …` to
    `nxboot: jump slot=a base=0x400000`.
  - That was the loader's first proof on hardware, and no serial adapter was attached.

## Alternatives considered

- **A serial adapter**: the standing path where one exists; not a dependency.
- **The display**: needs the display chain (B1.7) that the trace is meant to help bring up.
- **Blink codes on an LED**: a few bits, no text.
- **logd's journal**: structured logs only, never the markers, and only once the OS runs.

## Open questions

- ~~Phase 3: does the board's DRAM keep its content across a reset with the vendor SPL's DDR
  training?~~ Measured 2026-09-28: not reliably — the content decays over the reset; one quick reset kept it with bit errors (`docs/board/measurements/2026-09-28-dram-retention/`).

---

## Implementation Checklist

- [x] Phase 1: `storage::trace`, the layout's `trace` partition, the loader's writer, `nx image
      trace`, `just board-log`, the harness's trace contract (TASK-0327B P1, 2026-09-27).
- [x] Phase 2: the kernel ring, exposed read-only to the block owner (ADR-0040 amended), which
      keeps it in the trace; the OS-text contract in every lane (TASK-0327B P2, 2026-09-28).
- [x] Phase 3: the RAM rescue, which measures (TASK-0327B P3, 2026-09-28); the board's verdict recorded in the ledger.
