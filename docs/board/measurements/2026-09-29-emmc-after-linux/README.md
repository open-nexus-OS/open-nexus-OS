# 2026-09-29 — the eMMC boot right after the stock kernel ran (TASK-0327B P4 H0a)

## Question

An eMMC boot that follows a run of the stock kernel loops in the loader: `bsb ok (slot=a
seq=N)` → `verify FAIL (slot=a nxbd)` → `fallback -> slot=b` → `verify FAIL (slot=b nxbd)` →
`nxboot: PANIC (both slots bad)` → reset → again (measured 2026-09-29, 2 of 2; every boot after
`fastboot reboot` and every warm restart of our own chain verified, 12 of 12). Which check refuses
the slot's descriptor sector, what does the sector read as, does a second read agree, and in which
mode does the loader read?

## Instrument (this package)

- `nxboot: disk sdhci mode=<legacy|hs52|hs400es> bus=<w>` after the card opens.
- `nxboot: verify FAIL (slot=<s> nxbd-<malformed|reserved|crc|bounds|load> head=<16 hex> reread=<same|differs>)`.

## Protocol

1. Boot the stock system from the microSD; wait for adb.
2. Take the microSD out. (The stock system's root file system is on it: it dies at once and
   adb is gone — `adb reboot` is not available here; the reset button is the reboot. The
   state that matters, "the eMMC boot right after the stock kernel ran", is set either way.)
3. Reset: the SPL finds no card and boots the eMMC.
4. Wait a minute; microSD back in, reset; `just board-log` — the loop's loader lines carry the
   instrument's fields.

## Result

- Cycle 1 (12:24): the microSD was pulled BEFORE the stock system had finished booting (no adb
  yet); the reset then booted the eMMC with `dram probe kept (seq=1)` and `verify ok` — no loop.
  Loader lines: `nxboot: disk sdhci mode=hs52 bus=8`. So a stock kernel that dies early leaves the
  card readable; the failing cases had a fully booted stock system that had read the eMMC over adb
  (`just board-log`) and a `dram probe lost` (a longer-running kernel).
- Cycle 2 (12:28): stock fully up (adb; `/sys/kernel/debug/mmc2/ios`: HS400 enhanced strobe,
  200 MHz, bus 8, signal voltage 1.8 V, vdd 3.3 V) → `board-log` (the stock kernel reads the
  eMMC) → microSD out → reset. Result: `dram probe kept (seq=2)`, `disk sdhci mode=hs52 bus=8`,
  `verify ok`, the OS ran (67 844 bytes of OS trace). **No loop, 0 of 2 today.**

## Verdict

The loop does not follow "the stock kernel ran" alone: the same steps that produced it on
2026-09-29 morning produced a clean boot twice in the afternoon. The one difference the traces
show: every failing boot read `dram probe lost` (a reset that cost the DRAM — a longer press,
a power dip), every clean one after the stock kernel read `kept`. The instrument stays in the
loader: the next occurrence names the refused check, the sector's first bytes, whether a second
read agrees, and the mode the loader read with. The card-init fix waits for that trace
(TASK-0327B P4 H0a stays open as "instrumented, not reproduced").
