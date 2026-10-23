<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-23 — the arena reserved, the splash given back, the atlas counted, the layout split (TASK-0333 6b-2, Block 4 layer 6)

## Question

Since 6b-1 a page costs a frame only when it is first touched — except where a device needs the
memory before anyone touches it. gpud's layout block (13 planes, 107.8 MB at 1080p) is one
contiguous, eager block because the display controller scans three of its planes; the other ten
— the wallpaper, the retained plane and an eight-plane atlas — are read only by the GPU, which
needs no contiguity. Beside it the board keeps the boot splash (8.3 MB) for gpud's life. And
app-host's frame arena is a 1 MiB static whose 256 KiB halves the chat outgrows on every frame,
tolerated on the board as `SELFTEST: frame arena spill FAIL`.

Three questions, two steps:

1. (6b-2a) Does an arena reserved for the largest frame end the spill without costing the apps
   committed memory?
2. (6b-2a) Does the splash come back once the scan has left it?
3. (6b-2a → 6b-2b) How many of the atlas's 8 640 rows does a session ever reach — what would a
   lazy atlas hold committed against today's 66.4 MB?

## Instrument

- `apphost: frame arena (… peak= of= spill=)` and `SELFTEST: frame arena spill FAIL` — the arena's
  high-water across both regions, its size, whether a frame spilled;
- `KINIT: mm space pid= sid= rss= dma= reserved=` — committed and reserved bytes per space
  (`sid=0x0d05fd724dfcfac3` gpud; the greeter's app-host is the space 6b-1's folder named with
  `rss=7000064 reserved=5029888`);
- `KSELFTEST: mm frames (… free= …)` — frames free at the boot's census (on the board it follows
  the reveal and the first ring flip);
- new: `gpud: splash released (bytes=)` — once, at the first ring flip taken after the reveal;
- new: `windowd: atlas rows (peak= used= of=)` — the deepest atlas row any surface ever reached
  (a free never lowers it) and the rows in use, printed on each new high;
- host: `tests/dsl_apps_conformance/tests/frame_arena_budget.rs` — what one frame of each real
  app allocates per region, now also at the maximized window (1920×1024: the display less the
  desktop taskbar).

## Before (board cycle 64, `build/logs/board--2026-10-23T09-48-24`, build `dev-747a`)

- gpud `rss=123879424 dma=116137984`: the layout block 107 827 200 + the splash 8 294 400 + the
  cursor 16 384.
- `KSELFTEST: mm frames (… free=991583 …)` at the census (after `gpud: dc reveal flip ok` and
  `gpud: dc flip ok (slot=0 …)`).
- The chat: `apphost: frame arena (… peak=524288 of=1048576 spill=1 …)` — both regions' halves
  full, `SELFTEST: frame arena spill FAIL` twelve times (tolerated).

## Host (2026-10-23)

| App | Window | Scene (emit) | Layout |
|---|---|---|---|
| chat | 1440×814, 960×620, maximized 1920×1024 | 293 848 B | 500 552 B |
| settings | all three | 0 B (no services on the host) | 237 200 B |
| stash | 1440×814 / 960×620 / maximized | 90 048 B | 207 640 / 206 048 / 207 640 B |
| desktop-shell | 1920×1080, 1280×800 | 58 192 B | 101 200 B |

The chat costs the same at every size: it lays out its whole first page (60 rows), whatever the
window shows. The largest phase is 500 552 B — under half of a 1 MiB half. The recorded budgets
are now held against `nexus_service_entry::FRAME_ARENA_HALF` when the test file builds.

## QEMU (2026-10-23, `headless--2026-10-23T10-11-24`, `visible--2026-10-23T10-16-30`)

- `apphost: frame arena (layouts=12 … peak=74784 of=4194304 spill=0 …)` — the greeter.
- The greeter's app-host: `rss=7000064` before and after; `reserved` 5 029 888 → 8 175 616 B —
  exactly 3 MiB more reserved, nothing more committed.
- `windowd: atlas rows (peak=800 used=534 of=8640)` at 1280×800 — 9 % of the atlas ever reached.
- The splash path is the board display's only: QEMU's 2D splash resource is freed at attach.

## Board cycle 65 — decision lines (written before the cycle)

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the arena holds the largest frame | no `SELFTEST: frame arena spill FAIL` over the chat series, maximize included; the chat's `peak=` under 2 × 1 MiB |
| H2 | the splash comes back | `gpud: splash released (bytes=8294400)` once, after `gpud: dc reveal flip ok`; gpud `dma=` 8 294 400 lower, `free=` ≈ 2 025 frames higher at the census |
| H3 | the atlas is mostly never touched | `windowd: atlas rows (peak=…)` over the series: a lazy atlas ≈ peak × 7 680 B committed against 66.4 MB eager |
| H4 | nothing regresses | no `KPGF`, no app crash, the fleet `ready`, the chat types |

## Board cycle 65 — results (`build/logs/board--2026-10-23T10-29-54`, build `dev-91d2`)

The operator's series: greeter, calculator, chat (typing, scrolling), settings, stash, the chat
again (the clipboard restored an entry, tiled left, light mode, a window screenshot), the chat a
third time — **maximized twice** (`windowd: wm tile (zone=fill id=app0)` at lines 3702 and 3776),
tiled left and right, returned — each app closed with `code=0`. `[PASS] board-visible: 76 rungs,
8 operator marker(s), no FAIL marker, manifest clean`, the acks set after the operator confirmed
each point and its act line was found in the log.

| | Cycle 64 | Cycle 65 |
|---|---|---|
| `SELFTEST: frame arena spill FAIL` | 6 (tolerated) | **0** |
| the chat's `apphost: frame arena (… peak= of=)` | 524 288 of 1 048 576, `spill=1` | **789 164 of 4 194 304, `spill=0`** — the same after both maximizes |
| the chat's base heap (`base=`) | 674 096 | 504 048–525 488 (its layout no longer spills there) |
| `gpud: splash released (bytes=)` | — | **8 294 400**, line 918: after `dc reveal flip ok` (677) and the first ring flip (913) |
| gpud `rss=` / `dma=` at the census | 123 879 424 / 116 137 984 | **115 585 024 / 107 843 584** (−8 294 400 each) |
| `KSELFTEST: mm frames (… free=)` at the census | 991 583 | **993 609** (+2 026) |
| system `dma_bytes=` / `vmos=` | 116 199 424 / 124 | 107 905 024 / 123 |
| `windowd: atlas rows (peak=)` | — | 1 096 → 1 366 → 2 417 → 3 389 → 3 668 → **3 676 of 8 640** (`used=` at most 2 907) |
| faults | `fault-probe`, `demo.fault` (the ladder's own) | the same two, nothing else; no `alloc-fail`, no app crash |

Free frames at each app's reap move by 9–20 frames an app (990 563 → 990 525 over four apps;
cycle 64 the same: 988 539 → 988 504), with one exception: across the second chat session (the clipboard restore, a tile, light mode, the
screenshot) `vmo_bytes` grows by 5 345 280 B with the object count unchanged (133) — one object
replaced by a larger one, 1 444 frames — and stays flat over the third session (989 081 →
989 062). Not this step's: no 6b-2a change creates an object mid-session (screencapd's frame VMO
is made at its start, its PNG VMO destroyed after each save; the wallpaper swap rewrites plane
0). Its owner is 6b-3's to name (a heap and space line per service).

## Verdict

- **H1 confirmed.** No spill over the whole series, the maximized chat included; the chat's frame
  holds 789 164 B of the 4 MiB arena (both regions' halves together), as the host predicted
  (293 848 + 500 552 B). The gate is red on a spilling capture again
  (`gate-red-cycle64.txt`: cycle 64's capture judged with the entry gone).
- **H2 confirmed.** The splash is given back once, at the first ring flip after the reveal:
  gpud −8 294 400 B resident and DMA, +2 026 frames free at the census.
- **H3 answered.** A session that opens every app, tiles, maximizes and takes a screenshot reaches
  3 676 of the atlas's 8 640 rows: a lazy atlas holds ≈ 28.2 MB committed (3 676 × 7 680 B)
  where the eager one holds 66.4 MB — 38 MB, 57 % of the atlas, never touched. With a decommit on
  free the committed rows would follow `used=` (at most 2 907) instead; that is 6e's purgeable
  half. 6b-2b's split is sized from this: the three ring slots contiguous (24.9 MB), the rest
  page-backed and committed by what windowd writes.
- **H4 confirmed.** No new fault, no app crash, the fleet ready, the chat typed.

## 6b-2b — one pool with the GPU (ADR-0078)

The operator's direction (2026-10-23): no fixed GPU memory, a share of the machine like the
desktop systems with unified memory, the whole RAM usable. The board's layout block becomes the
scan-out ring (contiguous, 24.9 MB) and the surface object (lazy, made for the GPU, an eighth of
RAM in display planes — 64 on the board, 530.8 MB reserved — entered into the GPU's tables row
range by row range as frames sample it); the GPU's ceiling is two thirds of RAM (2.85 GB).

QEMU (2026-10-23, `headless--2026-10-23T11-59-25`, `visible--2026-10-23T12-06-05`):
`KSELFTEST: vmo lazy device ok (pages=4 named=3 runs=3 deny=3)`; the virtio display keeps the
fixed layout (`windowd: atlas rows (peak=800 used=534 of=8640)`); the FAIL kinds of the previous
visible run, nothing new.

Board cycle 66 — decision lines (baseline cycle 65, `board--2026-10-23T10-29-54`: gpud
`rss=115585024 dma=107843584`, `free=993609` at the census):

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the display needs only the ring contiguous | `gpud: layout split ok (ring=24883200 …)`; gpud `dma=24899584` (the ring + the cursor) |
| H2 | the surfaces cost what is drawn | gpud `rss=` ≈ 50 MB at the census (the ring, the wallpaper, the atlas rows drawn by then, its own 7.8 MB); `free=` ≈ +15 000 frames |
| H3 | the capacity follows the machine | `gpud: gpu memory (ram=4275466240 ceiling=2850310826 surfaces=530841600 scanout=24883200)`; `windowd: atlas rows (… of=63720)`; `held=` ≈ 580 MB of the ceiling |
| H4 | nothing regresses | the cycle-65 series (maximize, tiling, a screenshot whose freeze commits the retained plane): no GPU fault, no `FAIL dc present`, no `FAIL gpu memory`, frame times as cycle 65 |

### Board cycle 66 — results (`build/logs/board--2026-10-23T12-22-07`, build `dev-163f`)

The operator's series as cycle 65 with more windows open at once (the atlas reached 5 833 rows);
`[PASS] board-visible: 78 rungs, 8 operator marker(s), no FAIL marker`, the acks set after the
operator confirmed each point and its act line was found.

| | Cycle 65 | Cycle 66 |
|---|---|---|
| gpud `rss=` / `dma=` at the census | 115 585 024 / 107 843 584 | **49 373 184 / 24 899 584** |
| gpud `reserved=` (lazy, not yet touched) | 147 456 | 514 260 992 (the surfaces not drawn) |
| `free=` at the census | 993 609 | **1 006 774** (+13 165 frames, +51.4 MiB) |
| system `committed=` / `rss_sum=` / `dma_bytes=` | 177.7 / 146.6 / 107.9 MB | 121.7 / 88.3 / 25.0 MB |
| `gpud: gpu memory (…)` | — | `ram=4275462144 ceiling=2850308096 surfaces=530841600 scanout=24883200` |
| `gpud: layout split ok (…)` | — | `ring=24883200 surfaces=530841600 entered=8294400 held=586657792 ceiling=2850308096` (the first frame: the wallpaper alone) |
| `windowd: atlas rows (…)` | `of=8640`, peak 3 676 | **`of=63720`**, peak 5 833 |
| GPU faults | the boot rung's known `context lost` | the same, nothing in the session |
| a steady frame (5–6 draws, ~0.9 MP) | 10.5–11.8 ms, `clean=` 2.2–3.1 ms | 10.6–11.7 ms, `clean=` 2.5–2.9 ms |
| the heaviest window | 114 draws, 7.2 passes, 34.1 ms | 189 draws, 7.4 passes, 74.7 ms (`clean=` up to 10 ms) |

### Verdict (6b-2b)

- **H1 confirmed.** The display's memory committed up front is the ring: gpud `dma=24899584`
  (the ring and the cursor), from 107.8 MB.
- **H2 confirmed.** gpud holds what is drawn: 49.4 MB resident at the census (the ring, the
  wallpaper, the atlas rows drawn by then — 16.6 MB of the surfaces — and its own); 13 165 frames
  more free (the census came later in the boot than cycle 65's, with more drawn: the estimate
  was ≈ 15 000).
- **H3 confirmed.** The ceiling is two thirds of the RAM read at boot; the surfaces 64 planes;
  windowd's atlas 63 720 rows; the GPU held 586.7 MB against 2.85 GB.
- **H4 — the split regresses nothing it touches; one finding beside it.** No GPU fault, no
  `FAIL dc present`, no `FAIL gpu memory`; a steady frame costs what it did. The heavier windows
  carried more windows (189 draws). A band's first entry (its pages named, entered, and faulted
  into gpud's CPU view for the clean) lands in `clean=` and is not measured apart — 6b-3's P0
  splits it out. **Finding:** imed stopped draining after a focus change (line 2521: `WINDOWD:
  FAIL imed relay send` ×33, `inputd: imed send FAIL` ×22, no `apphost: text commit applied`) —
  typed text reached no field. imed blocks in a clock-less call per focus change (the
  `ime.personalization` read from settingsd, the store's flush to statefsd); both services lived
  on. The split touches neither path; the first such stall in fourteen cycles (two transient
  refusals on 2026-10-22). Routed to layer 8's closing with the input lane's other finding (typed
  text shown only after a select-all), the operator's call.
