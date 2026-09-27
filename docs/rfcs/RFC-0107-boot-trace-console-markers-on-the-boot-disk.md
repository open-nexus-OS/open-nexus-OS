# RFC-0107: The boot trace — console markers persisted on the boot disk, read without a serial adapter

- Status: In Progress (Phase 0 ✅ 2026-09-27, Phase 1 ✅ 2026-09-27 — TASK-0327B P0/P1; seeded
  2026-09-27)
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
- **Phase 2 (the kernel's console ring and its read syscall, the OS writer; the whole ladder
  persisted)**: ⬜
- **Phase 3 (a crashed boot's ring rescued from RAM by the next loader — only if the board
  measures DRAM retention across a reset)**: ⬜

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
- **Write authority**: before the OS, only the loader; in the OS, only the one trace writer,
  through the block owner's gate (deny-by-default on the kernel-attributed sender). Phase 2's read
  syscall is gated by a capability init hands to exactly that writer.
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
  loader and skips it, saying so.
- **Phase 2**: the kernel keeps a fixed static ring of every byte it sends to the console —
  including bytes before the console is known — and a read syscall returns the ring's bytes from a
  given offset; the OS writer appends them to its region periodically and at `init: ready`.
- **Phase 3**: the ring lives in a RAM range the tree reserves; the next loader copies a previous
  ring that is still intact (magic, CRC) into its slot before anything else — built only after
  the board shows that DRAM keeps its content across a reset.

### Phases / milestones (contract-level)

- **Phase 1**: the partition and the record codec (`storage::trace`), the loader's capture and
  writes, `nx image trace`, the harness's trace contract (every lane: the loader lines of
  `uart.log` equal the loader regions of the trace, boot by boot).
- **Phase 2**: the kernel ring and syscall, the OS writer, the whole `uart.log` in the trace.
- **Phase 3**: the RAM rescue.

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

- Phase 3: does the board's DRAM keep its content across a reset with the vendor SPL's DDR
  training? Measured before Phase 3 is built.

---

## Implementation Checklist

- [x] Phase 1: `storage::trace`, the layout's `trace` partition, the loader's writer, `nx image
      trace`, `just board-log`, the harness's trace contract (TASK-0327B P1, 2026-09-27).
- [ ] Phase 2: the kernel ring + syscall (ADR-0040 amended), the OS writer, the whole-log contract.
- [ ] Phase 3: measured, then the RAM rescue.
