<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-23 — the OS image is built with its own flags (TASK-0333 6c-0)

## Question

`.cargo/config.toml` names the OS target's flags (`nexus_env="os"`, `+zbb`, `share-generics`,
`relocation-model=static`). Every recipe that built or checked the OS image set `RUSTFLAGS`
(`scripts/build.sh` `RUSTFLAGS_OS` since 2025-10, the justfile's `os_rustflags`), and cargo takes
the first flag source it finds — so the shipped image never had `+zbb` or `share-generics`, while
`just build-kernel` (no `RUSTFLAGS`) did: two kernels from one tree. Does the image built with the
configured flags boot and run as before, and what do the flags change?

## Instrument

- the ELFs: size per `PT_LOAD`, the count of Zbb instructions (`llvm-objdump -d --mattr=+zbb`);
- the board's own lines: `apphost: ux launch … (first_frame_ms= mount_ms= vmo_ms= render_ms=)`
  (CPU-bound: the DSL's first layout and raster), `gpud: gpu frame us (avg=)`, the ladder.

## Before (measured 2026-10-23)

- Probe build of app-host and windowd with the configured flags (same features as `build.sh`):
  app-host 2 018 056 → 2 022 120 B (+0.2 %), windowd 3 832 496 → 3 826 760 B (−0.15 %); Zbb
  instructions 0 → 1 199 (app-host), 0 → 1 076 (windowd). `relocation-model=static` is the
  target's default — no change.
- Both targets have Zbb: the board's X60 lists `zba zbb zbc zbs` (stock system, `/proc/cpuinfo`,
  read only); QEMU runs `-cpu max`.
- Board cycle 67 (`board--2026-10-23T16-12-49`): launches `render_ms` 535 / 210 / 164 / 354 / 633
  / 917, `first_frame_ms` 688 / 212 / 239 / 558 / 730 / 1052; gpud's frame average median 24.5 ms
  (GPU-wait bound — not a CPU signal).

## Board cycle 68 — decision lines (written before the cycle)

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the image built with the configured flags runs as before | ladder complete, no FAIL kind absent from earlier board logs, the operator's series as cycle 67 |
| H2 | the flags reach every OS binary | Zbb instructions > 0 in the kernel, init-lite and every service ELF of the bundle |
| H3 | size is neutral | the OS ELFs' total within ±1 % of cycle 67's build |
| H4 | the CPU-bound launch work is not slower | `render_ms` of the same apps within cycle 67's spread or below; a launch slower by > 10 % on the same app is investigated, not shipped |

## Board cycle 68 — results

`board--2026-10-23T17-18-42` (build dev-a0d5fb43). QEMU `headless--2026-10-23T16-59-39` and
`visible--2026-10-23T17-03-39` green, no FAIL kind new against the run before.

| # | Result |
|---|---|
| H1 | ⚠ ladder complete, every process alive to the end, no fault beyond the selftests' own; the operator: "very fast" — and **the whole system hung** once: after four quadrant tiles (a scene no board cycle had tested) and the control center opened as a modal, the compositor drew one frame a second and the pointer froze, the GPU overloaded before it (frame average 64 ms, 31 of 32 dropped). Filed as its own defect, TASK-0345; nothing ties it to the flags (they change instruction selection only), nothing clears them either — the A/B is TASK-0345's question 3 |
| H2 | ✅ Zbb instructions in 40 of 40 shipped ELFs (kernel, init-lite, nxboot, every service); 0 before. The one ELF without them, `build/services/virtioblkd/`, is a stale artifact of 2026-09-25 (the service is retired, nothing builds or ships it) |
| H3 | ✅ the ELFs' total 38 767 200 → 38 612 360 B (−0.40 %): kernel −64 KB, init-lite −61 KB; nxboot's ELF +52 KB, its loaded bytes 149 896 → 156 216 (footprint 442 368 → 450 560) |
| H4 | inconclusive — `render_ms` greeter 535→584, desktop-shell 210→307, calculator 164→137, chat 354→455, settings + stash 1 550→1 347; across the eight cycles before, the same app's launch already spans ×2 (desktop-shell 191–387, chat 354–1 138) and `mount_ms`/`vmo_ms` (paths the flags cannot touch) moved with it. **The decision line was wrong**: one launch per app per cycle cannot resolve a 10 % codegen effect; a CPU-bound gate needs a repeated, isolated workload (a re-raster loop measured per run), not a launch |

Operator acks: desktop, pointer, modal, clipboard. `tile` not acked (four tiles by drag, no
`windowd: wm chord`), `typed` / `gpu-desktop` (the keyboard defect), `screenshot` (none taken).
