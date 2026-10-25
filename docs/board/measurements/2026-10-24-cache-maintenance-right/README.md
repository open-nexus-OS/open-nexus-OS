<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-24 — user cache maintenance is a device's right (TASK-0333 6c-1a, ADR-0079)

## Question

Every task could run `cbo.clean`/`cbo.flush` on every hart (RFC-0098 C4 as built): a
Flush+Reload primitive against every page two tasks share (the glyph atlas today, code pages
after 6c). After 6c-1a only a process holding a non-coherent device's capability is granted it,
at its first `cbo`. Do exactly the board's DMA drivers get it, does everything they drive still
work, and is everyone else refused?

## Instrument

- new: `KINIT: cache maintenance granted (pid=)` — once per process, at its first `cbo`;
- new: `KINIT: cache maintenance refused (pid= inst=)` + `[USER-ILL] sepc= inst= pid=` — a `cbo`
  (or any illegal instruction) from a process without the right; the process dies with
  `reason=fault`;
- new: `child: cache start` / `SELFTEST: cache maintenance refused ok` — the selftest's child
  without a device capability;
- `SELFTEST: dma buffer ok (device=… block=… runs=…)` — now with the device's own coherence;
- the ladder, the operator's series, `gpud: gpu frame us`.

## Before

- Tree (2026-10-23): one user-mode `cbo` site (`nexus_abi::cache_clean`/`cache_flush`), reached by
  sdhci (in blkd), xhcid, gpud (display controller + GPU memory) and the selftest's DMA probe —
  each from its own device capability's coherence. QEMU's virtio devices are coherent.
- A user-mode illegal instruction panicked the kernel (`panic!("ILLEGAL")` in the trap handler);
  6c-1a kills the process instead.

## Board cycle 69 — decision lines (written before the cycle)

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | exactly the drivers of non-coherent devices are granted | one `cache maintenance granted` line per process, and only for blkd, xhcid and gpud (pids resolved from the boot's spawn lines); never a second line for the same pid |
| H2 | what they drive still works | boot from the eMMC, mouse and keyboard events (`xhcid: irq hz=`), the desktop on the display, `SELFTEST: dma buffer ok (device=…)` |
| H3 | everyone else is refused, the kernel survives | `SELFTEST: cache maintenance refused ok` on QEMU and the board; no `cache maintenance refused` for any service |
| H4 | nothing regresses | ladder complete, no FAIL kind absent from earlier board logs, frame times as cycle 68's |

## Board cycle 69 — results

`board--2026-10-25T12-17-03` (build dev-c51e5c0d). QEMU `headless--2026-10-24T18-17-47` and
`visible--2026-10-24T18-24-17` green: no grant (no non-coherent device), the child refused
(`KINIT: cache maintenance refused (pid=78 inst=0x0014200f)`, `[USER-ILL] sepc=0x1020`,
`execd: exit pid=78 reason=fault`), `SELFTEST: dma buffer ok (device=coherent block=64 runs=1)`.

| # | Result |
|---|---|
| H1 | ✅ three grants, one each: pid 43 = blkd (right after `blkd: backend ok (kind=spacemit,k1-sdhci …)`), pid 53 = xhcid (between the dwc3 reset and `xhcid: controller ok`), pid 55 = gpud (sid `0xd05fd724dfcfac3`); no second line for any pid |
| H2 | ✅ `blkd: gpt ok (parts=7)`, `xhcid: controller ok`, the desktop, mouse and the series; `SELFTEST: dma buffer skipped (no device granted)` as in cycle 68 (the board's harness gets no device window) |
| H3 | ✅ `SELFTEST: cache maintenance refused ok` on the board; the only refusal is the selftest's child; the kernel survived it |
| H4 | ✅ ladder 79/79, the only FAIL kind new against cycle 68 is `apphost: ux launch FAIL` (in 11 earlier board logs); gpud frame average median 31 ms with four quadrant tiles (cycle 68 with the same scene: 64 ms and a stall); no compositor stall (longest `moved=0` run 2, recovered) |

Operator acks: desktop, pointer, modal, clipboard, screenshot, gpu-desktop. `tile` not acked (the
drag only — no Super+Ctrl+arrow tried), `typed` (the keyboard defect).
