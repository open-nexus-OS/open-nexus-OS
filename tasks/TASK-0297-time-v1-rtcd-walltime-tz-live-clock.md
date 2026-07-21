---
title: TASK-0297 Time v1 (OS/QEMU): goldfish rtcd + timed walltime + tz-lite + live clock + timezone Settings
status: Draft
owner: @runtime
created: 2026-07-21
depends-on:
  - TASK-0298
follow-up-tasks:
  - TASK-0299 (SNTP time-syncd, seed)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract seed: docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md (seeded by this task)
  - Settings spine (time.zone/time.format keys): tasks/TASK-0298-settings-spine-watch-region-keys.md
  - Region push (consumer): tasks/TASK-0241-i18n-v2-os-runtime-locale-switch-region-push-settings.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The OS has no wall-clock at all: no RTC driver, `timed` is a monotonic
timer-coalescing authority only, `time-syncd`/`userspace/time_sync` are echo
placeholders, and every clock/date in the UI (shell, greeter) is a static
i18n string ("09:41", "Sunday, July 13"). The Settings "General management"
plan needs a real timezone setting, which needs real time.

Architecture (RFC-0076): a tiny `rtcd` driver reads the QEMU goldfish RTC
once (epoch ns); `timed` — already the time authority — anchors
`walltime = rtc_anchor + monotonic_delta` and serves it as UTC;
timezone conversion is a client-side no_std library (`tz-lite`) with a
curated IANA subset incl. DST rules; the shell clock ticks via app-host's
existing recv-timeout pacing (minute boundary), no VSYNC coupling.

## Goal

1. **`source/drivers/rtc/goldfish-rtc` (`rtcd`)**: MMIO map via the
   virtio-rng driver pattern; read TIME_LOW/TIME_HIGH → epoch ns; serve
   `frames!` protocol `nexus-wire/src/rtcd.rs` (MAGIC `'R','T'`,
   `OP_READ_TIME=1` → `status, epoch_ns:u64`). Polling read, no IRQ in v1.
   **Verify base/IRQ against the QEMU dtb before coding** (virt typical:
   0x101000).
2. **timed**: `OP_GET_WALLTIME=4` → `status, epoch_ns` — anchored at boot
   from rtcd (sole client); `STATUS_UNAVAILABLE` when unanchored (honest —
   never fake time).
3. **`userspace/libs/tz-lite`** (new, no_std, zero deps): curated zone table
   (UTC, Europe/Berlin, Europe/London, America/New_York, America/Los_Angeles,
   Asia/Tokyo, Asia/Shanghai, Asia/Seoul, Australia/Sydney, …) with DST rules
   as const data; `to_civil(epoch_ns, zone) → {y,m,d,weekday,h,min}` +
   12/24h formatting helper; host-tested against fixture epochs (incl. DST
   boundary cases). The zone table is the **validator SSOT** for the
   `time.zone` settings key.
4. **Live clock**: app-host `svc.time.now()` effect + minute-boundary
   `Wait::Timeout` tick dispatching a `ClockTick` DSL event; desktop-shell +
   greeter bind clock/date to state (static strings removed); honors
   `time.zone` + `time.format` from `OP_SURFACE_REGION`.
5. **Settings**: General management → timezone picker (tz-lite table) +
   24h/12h toggle.

## Non-Goals

- No NTP/network time (TASK-0299 seed); no RTC write-back; no alarms/IRQ.
- No full IANA tzdb or leap seconds; no CLDR date formatting (weekday/month
  names via app i18n catalogs).
- No kernel changes.

## Constraints / invariants (hard requirements)

- timed serves **UTC only**; conversion is client-side (tz-lite) — one time
  authority, zero timezone state in services.
- Never fake time: unanchored walltime → `STATUS_UNAVAILABLE`, UI shows
  "--:--", markers simply don't fire (run fails honestly).
- Clock tick = one wakeup/minute via existing event-loop pacing — no new
  threads, no per-frame work, no VSYNC coupling.
- MMIO mapping USER|RW, never exec; bounded reply parsing everywhere.

## Security considerations

- rtcd answers only timed (`sender_service_id` gate; `test_reject_*`).
- Walltime is low-sensitivity but spoofable time skews logs: only rtcd
  (hardware) can anchor timed; no IPC op accepts an externally supplied time
  in v1 (NTP will go through its own vetted path in TASK-0299).

## Contract sources (single source of truth)

- **Wire + semantics**: RFC-0076 + nexus-wire goldens (rtcd, timed op).
- **Zone table**: `userspace/libs/tz-lite` const table (= settings validator).
- **QEMU marker contract**: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`.

## Stop conditions (Definition of Done)

- **Proof (host)**: tz-lite conversion goldens incl. DST boundaries; codec
  goldens + reject matrices; 12/24h formatting tests.
- **Proof (QEMU)**:
  - `rtcd: ready` (after a successful MMIO read)
  - `timed: walltime anchored`
  - `SELFTEST: walltime rtc ok` (epoch > 2020-01-01 and consistent with
    monotonic delta across two reads)
  - `SELFTEST: clock tz ok` (fixed epoch converts correctly for two zones)
- **Proof (interactive)**: `just start` — live clock in shell + greeter;
  changing timezone in Settings shifts it immediately.
- **Gates**: `just check`, `just test-all` green; RFC-0076 checklist ticked.

## Touched paths (allowlist)

- `source/drivers/rtc/` (new: goldfish-rtc, src/ + tests/)
- `source/services/timed/` (walltime op + anchor)
- `userspace/libs/tz-lite/` (new)
- `source/services/app-host/` (time effect + tick), `userspace/apps/desktop-shell/`,
  `userspace/apps/greeter/`, `userspace/apps/settings/` (pickers)
- `source/libs/nexus-wire/src/rtcd.rs` (new) — **approval zone**
- Boot service list (Makefile/scripts), `scripts/qemu-test.sh`,
  `tools/nx/chains/markers.txt` — **approval zone**
- `docs/rfcs/RFC-0076-*.md` (new seed) — **approval zone**
- `docs/architecture/**` (time authority note), `CHANGELOG.md`

## Plan (small PRs)

1. RFC-0076 seed; dtb verification; rtcd driver + wire + host tests.
2. timed walltime op + anchor + markers + selftest.
3. tz-lite + goldens.
4. app-host tick + shell/greeter live clock + Settings pickers + selftest + docs.

## Acceptance criteria (behavioral)

- Booted OS shows the real (host) date/time live in shell + greeter, ticking
  at minute boundaries; timezone + 12/24h changes apply instantly and persist.
- Without an RTC (unanchored), the UI shows a placeholder and no walltime
  marker fires — no fake time anywhere.
