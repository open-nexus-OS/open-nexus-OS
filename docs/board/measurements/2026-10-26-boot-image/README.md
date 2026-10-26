<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-26 — the boot image is a container: kernel + bootfs (TASK-0333 6c-2, ADR-0081)

## Question

Every core program's bytes are in memory three to four times (the kernel's rodata holds init,
init's rodata holds every core ELF, execd's rodata holds app-host; the kernel copies init at boot,
init copies every core ELF into a sealed object at boot). With the bootfs beside the kernel,
adopted as sealed objects and handed to init as capabilities, do those copies disappear, does the
boot get shorter, and does everything still run?

## Before (board cycle 70, `board--2026-10-26T10-32-12`)

- `KINIT: mm frames (… excluded=4671 …)` — the kernel image (10.2 MB of file + 8.8 MB of `.bss`
  and stack) and the tree; 9.7 MB of the file is init-lite's ELF;
- `KSELFTEST: segment copied (pages=…)` — the kernel's page copy of init at boot; init-lite's
  space at the census is not among the 13 largest (< 315 392 B) but its image frames are its
  private copy of 9.7 MB;
- init's `sealed_image` of every core ELF: ≈ 8 MB of `vmo_write` at boot; execd's image
  (6 841 360 B of file, 1 670 shared pages = 6.5 MB) carries app-host's and recv-wake's bytes;
  app-host's sealed object (495 pages) is a copy of them;
- frame census `free=1011396 … spaces=41 rss_sum=73207808`;
- boot: `KINIT: milestone 11 at 2000 ms`; first `init: start keystored` after the init load.
- Built ELFs today: kernel 10 213 496 (flat 10 204 008), init-lite 9 704 576, execd 6 841 360,
  app-host 2 028 544; all built services page-rounded 15 437 824 B (of which init-lite's and
  execd's embedded bytes ≈ 16 MB are the nesting).

## Instrument

- new: `nxboot: bootfs (at=0x… len=…)` — the loader placed the second item;
  `KSELFTEST: bootfs ok (files= bytes=)` — the kernel adopted every file as a sealed object;
  `KINIT: exec image (pid=1 …)` — init through the one loader;
- `KINIT: mm frames (… excluded= …)` and the frame census `free=`; `KINIT: mm space` for
  init-lite and execd; `KINIT: exec image` for execd (`shared_pages`) and app-host;
- `KINIT: milestone 11 at … ms`.

## Decision lines (written before the cycle)

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | the copies are gone | `excluded=` grows by the bootfs (≈ 2 700 frames) minus init's 9.7 MB (≈ 2 370) — roughly unchanged; the frame census `free=` ≥ cycle 70's 1 011 396 + 5 000 (init's private copy 2 370 + the core ELFs' sealed copies ≈ 2 000 + execd's app-host bytes 1 560 + app-host's copy 495, less the bootfs's own 2 700 that moved into `excluded`); execd's `KINIT: exec image (… shared_pages= …)` ≤ 120 (was 1 670) |
| H2 | the boot copies nothing it does not need | no `KSELFTEST: segment copied`; no `vmo_write` burst at init's start; `KINIT: milestone 11` ≤ cycle 70's 2 000 ms; `init: start keystored` within the same milestone as before or earlier |
| H3 | the trust is one digest | host: a flipped bootfs byte behind a valid NXBD is `Reason::Digest` in the loader's flow test; QEMU: the OTA backstop lane (`tamper`) still rejects; `KSELFTEST: bootfs ok` on QEMU and the board; the `test_reject_*` of the two codecs green |
| H4 | nothing regresses | the ladder complete, every service `ready`, the operator's series (apps, tiling, modal, clipboard, screenshot), no FAIL kind absent from cycle 70's log, frame pace as cycle 70's, `mappers=` of app-host's image follows the apps as in cycle 70 |

## Results

### QEMU headless (`headless--2026-10-26T16-16-36` vs cycle 70's `headless--2026-10-26T09-54-18`)

- `nxboot: bootfs (at=0x80e00000 len=9228288)` — the bootfs at the first 2 MiB boundary past the
  kernel's footprint (`len=0x8f7c00` → 0x80400000 + 0x8f7c00 ≈ 0x80cf7c00 → 0x80e00000);
  `KSELFTEST: bootfs ok (files=18 bytes=9169560)` before the banner; `KINIT: exec image (pid=29
  vmo=2 shared_pages=65 copied_pages=1)` — init through the one loader, 65 pages of its 267 KB
  shared, one data page copied; no `segment copied` line (3 before).
- Built: kernel ELF 508 896 (was 10 213 496), init-lite 267 256 (9 704 576), execd 4 793 360
  (6 841 360); payload 9 728 512 = kernel 499 336 + bootfs 9 228 288 (18 files).
- **H1** ✅ (QEMU): `excluded=` 4 669 → 4 552 (the bootfs's 2 253 frames in, the kernel's 9.7 MB
  of init out); frame census `free=` 38 238 → **43 399 (+5 161 frames = +20.2 MiB)**; `allocs=`
  3 929 → 1 516. execd's image `shared_pages=` 1 670 → **1 170**, not the ≤ 120 the decision line
  said: the 500 pages that left are app-host's and the probe's bytes; what stays is the baked text
  atlas execd embeds to fill the shared RO VMO (RFC-0080, ≈ 4.5 MB) — a program's data, not a
  program. Follow-up (not 6c-2): the atlas as a bootfs file of its own.
- **H2** ✅ (QEMU): no `segment copied`, no `vmo_write` burst; `KINIT: milestone 11` 990 → 952 ms;
  `init: start keystored` at line 234 (was 248).
- **H3** ✅ (host): `test_reject_a_tamper_inside_the_bootfs_as_a_digest_failure`,
  `test_reject_a_container_the_window_cannot_place`, `test_reject_a_header_that_is_not_a_container`
  green beside the 19 existing flow tests; the two codecs' `test_reject_*` green (nexus-bootfs 7,
  nx payload 4). QEMU: `ota-tamper--2026-10-26T16-23-15` — a flipped byte behind a valid NXBD is
  `nxboot: verify FAIL (slot=b digest)` with the container, the fallback to slot a boots with
  `nxboot: bootfs (…)`; `ota-fallback--2026-10-26T16-50-46` — the trial slot verifies as a
  container (`verify ok (slot=b build=otaB64f3`), parks twice, tries 2→1→0, the exhaustion flip
  back to slot a, `SELFTEST: ota fallback ok`. Found on the way: the fixtures' os-B was "the
  running image + a 512-byte trailer" — bytes past the container (`verify FAIL (slot=b
  payload-size)`); now every disk assembler checks the slot image is a container whose length the
  header covers, and os-B is the container re-composed with the trailer on the kernel item.
- **H4** (QEMU): the ladder complete (`SELFTEST: Completed`, chain markers 13/13, trace contract
  loader 12 lines + OS 79 694 bytes byte-equal); every FAIL kind present in cycle 70's headless log
  and none new (`init: FAIL declared slot route send slot=8` 1/1, `dsoftbus os connect FAIL` 1/1,
  `execd: exit pid=80 reason=error code=42` 1/1, `route-deny bundlemgrd -> execd` 1/1,
  `apphost: ux launch FAIL (first_frame_ms=…)` 1/1 — all pre-existing); app-host's image
  `shared_pages=495 mappers=1` from the handed-over object (`vmo=18`, an adopted bootfs object).

### Board cycle 71 (`board--2026-10-26T17-05-01`, build dev-64f3fda1, vs cycle 70)

Ladder complete (79 rungs, `board-test --log` judge); operator: "alle funktional".

| # | Verdict | Evidence |
|---|---|---|
| H1 | ✅ (QEMU) · board consistent, census not comparable | `nxboot: bootfs (at=0xe00000 len=9228288)`, `KSELFTEST: bootfs ok (files=18 bytes=9169560)`, init `KINIT: exec image (pid=29 vmo=2 shared_pages=65 copied_pages=1)`; kernel image `len=` 0x1239c00 → **0x8f7c00** (−9.7 MB of init), `excluded=` 4 671 → 4 554, `allocs=` 8 130 → 6 909; execd's image 1 670 → 1 170 shared pages (the baked atlas stays, see QEMU); app-host's image is the handed-over bootfs object `vmo=18` (was a copy, `vmo=88`), `mappers=` 1 → 6 as the operator opened apps. The census `free=` 1 011 396 → 1 012 851 (+1 455 frames) was taken with 44 spaces instead of 41 and `rss_sum` 73.2 → 96.2 MB (+23 MB of the operator's open apps), so the ≥ +5 000 line is not read from it; QEMU's census at the same operator-free point gave +5 161 |
| H2 | ✅ | no `KSELFTEST: segment copied` (3 before); `KINIT: milestone 11` 2 000 → **1 743 ms**; `init: start keystored` at line 203 (219) |
| H3 | ✅ | `KSELFTEST: bootfs ok` on the board; the loader's digest covers the bootfs (QEMU `ota-tamper`); host `test_reject_*` (loader 3 new, codecs 11) |
| H4 | ✅ | FAIL kinds the same six as cycle 70 (`WINDOWD: FAIL imed relay send` 14× vs 8× — the typed-input defect of the UX lane, grows with typing; `init: FAIL declared slot route send slot` 1×; `gpud: FAIL gpu gate scene` 1×; `route-deny bundlemgrd -> execd` 1×; `[USER-ILL]` 1× = the cache probe; `[USER-PF]` 216 = 216); `SELFTEST: … ok` 235 (234), 36 services `ready` (36); frame pace `on_time=0 late=2–4` as cycle 70's, one sample `on_time=3`; acts in the log: `wm tile` 6 + `wm chord` 2, modal 13, clipboard 25, screenshot 11, 10 app launches, 17 reaped |

**Verdict:** 6c-2 holds on the board. ADR-0081 → Accepted.
