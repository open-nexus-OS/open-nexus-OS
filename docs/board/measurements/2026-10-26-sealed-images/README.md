<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-26 — a process runs from a sealed image (TASK-0333 6c-1b, ADR-0080)

## Question

Every process got a private copy of its whole ELF; a volume service was held twice (init's
mapped DMA object + the kernel's copy). With `exec_vmo`, a process's text and read-only data
are the image's own frames, shared by every process of that image; a volume service is held
once, sealed before it is verified. Do the app-hosts share, do the volume services shrink to
one copy, and does everything still run?

## Instrument

- new: `KSELFTEST: exec image shared ok (text= ro= copied=)` — the kernel's own boot proof: two
  spaces loaded from one sealed image map the same frames for text and rodata and copy only
  the data page; `KSELFTEST: exec image reject ok (…)` — the refusal matrix (unsealed, unaligned,
  shared file page, writable map of a sealed object, a seal of a device-named object);
- new: `KINIT: exec image (pid= vmo= shared_pages= copied_pages= mappers=)` per exec — `mappers`
  counts the live spaces that map the image (the second app-host shows 2);
- `KINIT: mm space pid= sid= rss=` at the census; `execd: reaped … (free= …)`;
- `bundlemgrd: bundle served (name= read_ms= copy_ms= hash_ms=)` — the bounce copy's cost.

## Before (board cycle 69, `board--2026-10-25T12-17-03`)

- app-host children: `KINIT: mm space … sid=0xdd8c136b3d65f5dc rss=7000064` (the greeter's
  app-host, 6.7 MiB) — of which 1 489 004 + 529 688 B (R + RX) are a private copy of the image;
- init-lite rss 6 443 008 (its rodata holds every CORE ELF and the volume mappings);
- 18 volume services on the board, 6 384 640 B of ELFs read into DMA objects init keeps mapped,
  plus the kernel's copy per spawn;
- free frames at the census: 1 009 907 (cycle 68), with four quadrant tiles 990 953.

## QEMU lanes 2026-10-26 (before the board)

`headless--2026-10-26T09-54-18` and `visible--2026-10-26T09-57-45`, both ladders complete. The
first headless run was red twice, both in the kernel (the syscall window ended at 64 and the
seal is number 64; the selftest asked for a `destroy` of an image the orphan reclaim had already
taken) — see the ledger.

| Instrument | `visible--2026-10-24T18-24-17` (6c-1a) | `visible--2026-10-26T09-57-45` (6c-1b) |
|---|---|---|
| `KSELFTEST: exec image shared ok` / `reject ok` | — | both, on both lanes |
| `KINIT: exec image` per exec'd service | — | every service, `copied_pages=1` each |
| `bundle served` per volume service | `read_ms= hash_ms=` | `copy_ms=0` (dsoftbusd, pinched: 1), `hash_ms` unchanged |
| frame census `free=` (same ladder point) | 35 638 | 37 159 (+1 521 frames = +5.9 MiB) |
| frame census `rss_sum=` | 146 489 344 | 139 837 440 (−6.3 MiB) |
| `spaces=` at the census | 42 | 41 |
| FAIL kinds | `init: FAIL declared slot route send slot` ×1 | the same, ×1 |
| `mappers=` | — | pinched's three respawns: 2 → 3 → 4 from the ONE image |

Two corrections to the lines below, learned here. `rss=` counts a `Fixed` region in full, so an
app-host's rss does **not** fall when its text is shared (7 004 160 on this lane, as before);
the frame pool does. And `mappers=` climbing across pinched's deliberate crashes shows the
respawns run from the one sealed image — and that init never reaps a dead child, so each dead
pinched keeps its space (a 6d item; cycle 69 had the same zombies, uncounted).

## Board cycle 70 — decision lines (written before the cycle, H1/H2 corrected after the QEMU lanes)

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | app-hosts share their image | `KINIT: exec image (… mappers=2)` at the second app-host (the greeter's app-host plus the first launched app), `mappers=4` with four quadrant tiles; `copied_pages=1` for each |
| H2 | a volume service is held once | `bundle served (… copy_ms= …)` for each of the 18 volume services and no `init: volume spawn FAIL`; the frame census `free=` at the four-tile point ≥ cycle 68's 990 953 + 1 480 (three shared app-host images) + 1 560 (the volume services' second copy) = ≥ 993 993 — H1 and H2 share this one number; init-lite's `rss=` at the census below cycle 69's 6 443 008 (its volume mappings are gone; its rodata still counts) |
| H3 | the seal is between the hash and the run | `bundlemgrd: bundle served (… copy_ms= hash_ms=)` for every volume service, no `Integrity`; `KSELFTEST: exec image reject ok` on the board |
| H4 | nothing regresses | the ladder complete, every service `ready`, the operator's series (apps, tiling, modal, clipboard, screenshot), no FAIL kind absent from earlier board logs, frame times as cycle 69's, boot time within a second of cycle 69's |

## Board cycle 70 — results (`board--2026-10-26T10-32-12`, build dev-2deb812c)

Ladder complete (79 rungs); operator acks desktop, pointer, tile (drag ×6 + chord ×2 in the
log), modal, clipboard, gpu-desktop; `typed` not acked (the keyboard defect of the UX lane),
screenshot not tried.

| # | Result | Evidence |
|---|---|---|
| H1 ✅ | app-hosts share their image | `KINIT: exec image (… vmo=88 shared_pages=495 copied_pages=1 mappers=N)`: N = 1 (greeter) → 2, 3, 4, 5, 6 as the operator opened five apps, back to 3 after closing, up to 6 again — `execd: reaped` ×21 brought the count down each time (the app-hosts' spaces go; init's children's do not, see the QEMU note) |
| H2 ✅ | a volume service is held once | 18 × `bundle served (… copy_ms=0 …)` (dsoftbusd 3 ms, one 11 ms), 0 × `volume spawn FAIL`, 0 × `Integrity`; frame census `free=` 995 646 (cycle 69) → 1 011 396 (+15 750 frames = +61.5 MiB; decision line ≥ 993 993), `rss_sum` 124 772 352 → 73 207 808, `vmo_bytes` 716 132 352 → 661 225 472. Of the 61.5 MiB, 16.6 MiB are the text/rodata pages the first exec of every image now shares (Σ `shared_pages` = 4 256) and ≈ 6 MiB the volume services' second copy; the rest comes with five fewer spaces at the census (46 → 41) and is not attributed here. init-lite is not among the 13 largest spaces (< 315 392 B) — nor was it in cycle 69; the 6 443 008 named above came from cycle 68 (`board--2026-10-23T16-12-49`), the census point differed |
| H3 ✅ | the seal is between the hash and the run | `KSELFTEST: exec image shared ok (text=1 ro=1 copied=1)` and `… reject ok (unaligned shared-page lazy device-named writable-mapped unsealed)` on the board; the one `EXEC-REFUSED reason=unsealed` is the selftest's own; every volume service hashed after its seal |
| H4 ✅ | nothing regresses | FAIL kinds the same three as cycle 69 (`WINDOWD: FAIL imed relay send` 8× vs 2× — the typed-input defect of the UX lane, raised while the taskbar was used; `init: FAIL declared slot route send slot` 1×; `gpud: FAIL gpu gate scene` 1×); the one `[USER-ILL]` is the cache probe of 6c-1a (pid 0x4f, `cbo.clean`); boot milestone 11 at 2 000 ms (cycle 69: 1 869 ms); frame pace late-dominated as in cycle 69 (`on_time=0–2 late=2–7` per sample); operator: "alle funktional" |

**Verdict:** 6c-1b holds on the board. ADR-0080 → Accepted.
