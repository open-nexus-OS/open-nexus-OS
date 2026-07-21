# RFC-0076: Wall-clock v1 — goldfish rtcd, timed walltime, tz-lite, live clock

- Status: In Progress (all phases proven 2026-07-21; deviation documented)
- Owners: @runtime
- Created: 2026-07-21
- Last Updated: 2026-07-21
- Links:
  - Tasks: `tasks/TASK-0297-time-v1-rtcd-walltime-tz-live-clock.md` (execution + proof),
    `tasks/TASK-0299-time-sync-v1-sntp-seed.md` (NTP follow-up, seed)
  - Related RFCs: `docs/rfcs/RFC-0078-settings-region-keys-watch.md`
    (`time.zone`/`time.format` keys), `docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md`
    (`OP_SURFACE_REGION` carries tz/hour-format to apps)
  - ADRs: `docs/adr/0051-declarative-wire-codec-nexus-wire.md` (frames! codec)

## Status at a Glance

- **Phase 0 (RTC read path)**: ✅ (2026-07-21 — `rtc-goldfish` lib + policy-gated grant to timed; deviation note below: no rtcd service)
- **Phase 1 (timed walltime anchor)**: ✅ (`timed: walltime anchored` + `SELFTEST: walltime rtc ok` in ci-os-smp1)
- **Phase 2 (tz-lite conversion)**: ✅ (5 host goldens incl. EU/US DST boundaries; `SELFTEST: clock tz ok`; settingsd zone-table pin test)
- **Phase 3 (live clock UI + timezone Settings)**: ✅ (`apphost: clock tick applied` live-proven; greeter/shell bind $state.clock/date; Settings tz + 24h/12h chips)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The wall-clock authority model: `timed` is the ONLY wall-clock authority
    (UTC); `rtcd` is its only anchor source in v1.
  - The rtcd wire protocol and `timed`'s `OP_GET_WALLTIME`.
  - The tz-lite zone-table contract (= `time.zone` validator SSOT) and the
    client-side conversion model.
  - The honesty rule: never fake time (`STATUS_UNAVAILABLE`, UI placeholder).
- **This RFC does NOT own**:
  - NTP/anchor refinement (TASK-0299 seed — needs an RFC extension first).
  - The settings keys themselves (RFC-0078) or their propagation to apps
    (`OP_SURFACE_REGION`, RFC-0077).
  - Monotonic timer coalescing (existing timed charter).

### Relationship to tasks (single execution truth)

- TASK-0297 implements and proves every phase.

## Context

The OS has no wall-clock: no RTC driver, `timed` is monotonic-only,
`time-syncd` is a placeholder, and every clock/date in the UI is a static
i18n string. The Settings timezone/hour-format keys (RFC-0078) need real
time behind them. QEMU `virt` ships a goldfish RTC at MMIO `0x101000`
(dtb-verified 2026-07-21: `rtc@101000`, `google,goldfish-rtc`, IRQ 11).

## Goals

- Real UTC wall-time served by the existing time authority.
- Client-side timezone conversion from a curated, testable zone table.
- A live minute-ticking clock in shell + greeter with honest fallback.

## Non-Goals

- No RTC write-back, no alarms/IRQ path (polling read at boot).
- No full IANA tzdb, no leap seconds, no CLDR formatting.
- No kernel changes; no second clock authority — time-syncd may LATER refine
  timed's anchor through a vetted op (TASK-0299), never serve time itself.

## Constraints / invariants (hard requirements)

- **Determinism**: tz-lite conversion is pure const-table math; goldens pin
  fixture epochs incl. DST boundaries.
- **No fake success**: `timed: walltime anchored` / `SELFTEST: walltime rtc
  ok` only after a REAL RTC read; unanchored ⇒ `STATUS_UNAVAILABLE` and the
  UI shows `--:--`.
- **Bounded resources**: fixed frames; one RTC read at anchor time (re-read
  only on explicit re-anchor); clock tick = one wakeup/minute via the
  existing app-host event-loop pacing (no threads, no VSYNC coupling).
- **Security floor**: MMIO USER|RW never exec; rtcd answers only timed
  (`sender_service_id`); no IPC op accepts externally supplied time in v1.

## Proposed design

### Authority model (normative)

```
goldfish RTC (MMIO 0x101000, policy-gated grant device.mmio.rtc) ──read──> timed
timed: walltime_ns = rtc_anchor_epoch_ns + (monotonic_now − monotonic_at_anchor)
clients (app-host via svc.time route, selftest): OP_GET_WALLTIME → UTC epoch ns
tz conversion: client-side via userspace/tz-lite (zone table = SSOT)
region fan-out: windowd watches settingsd (time./locale) → OP_SURFACE_REGION
```

**Implementation deviation (2026-07-21, normative now):** there is NO
separate `rtcd` service. `timed` — the time authority — reads its own anchor
through the `rtc-goldfish` driver library (`source/drivers/rtc/goldfish-rtc`;
the unsafe MMIO reads stay in `drivers/`). Rationale: a 2-register read-only
device does not justify a boot service + wire protocol + init wiring while
init's 128-slot cap table runs at its ceiling (three routes broke during
this task from exactly that pressure). The `nexus-wire/rtcd.rs` protocol from
the seed is NOT built; `OP_READ_TIME` is dropped.

### Wire contract (normative once Phase 1 proofs are green)

- `nexus-wire/src/rtcd.rs` — MAGIC `'R','T'`, VERSION 1, frames! codec:
  `OP_READ_TIME = 1` → reply `status:u8, epoch_ns:u64`
  (`STATUS_OK=0, STATUS_MALFORMED=1, STATUS_UNAVAILABLE=2`).
  rtcd serves only timed's `sender_service_id` (DENIED otherwise → status 3).
- `timed` protocol (existing `'T','M'` family, additive):
  `OP_SLEEP_UNTIL = 3` (existing) … `OP_GET_WALLTIME = 4`:
  request `[T, M, ver, 4, nonce:u32]` → reply
  `[T, M, ver, 4|0x80, status:u8, nonce:u32, epoch_ns:u64]`;
  `STATUS_UNAVAILABLE` while unanchored.

### tz-lite (normative)

- `userspace/libs/tz-lite`: no_std, zero deps. Const zone table (the
  `time.zone` validator SSOT — `settingsd::registry::TIME_ZONES` must match;
  a pin test enforces it): UTC, Europe/Berlin, Europe/London,
  America/New_York, America/Los_Angeles, Asia/Tokyo, Asia/Shanghai,
  Asia/Seoul, Australia/Sydney.
- Each zone: base UTC offset + optional DST rule (EU last-Sun-Mar/Oct 1h,
  US second-Sun-Mar/first-Sun-Nov 1h; AU Oct/Apr southern rule). Civil
  conversion `to_civil(epoch_ns, zone) → {year, month, day, weekday, hour,
  minute}` + `format_hm(24h|12h)`.

### Phases / milestones (contract-level)

- **Phase 0**: rtcd driver + wire (`rtcd: ready` after a real MMIO read).
- **Phase 1**: timed anchor + `OP_GET_WALLTIME`
  (`timed: walltime anchored`, `SELFTEST: walltime rtc ok`).
- **Phase 2**: tz-lite + goldens (`SELFTEST: clock tz ok`).
- **Phase 3**: live clock in shell/greeter + Settings timezone/format pickers.

## Security considerations

- **Threat model**: time spoofing (log skew, cert-adjacent future misuse);
  malformed frames; MMIO mapping abuse.
- **Mitigations**: only rtcd (hardware-backed) anchors timed; rtcd
  identity-gates its caller; codec-enforced bounds; MMIO grant policy-gated
  like every driver (`device.mmio.rtc`).
- **Open risks**: none blocking; NTP refinement contract deferred to the
  TASK-0299 RFC extension.

## Failure model (normative)

- RTC unreadable ⇒ rtcd stays up, answers `STATUS_UNAVAILABLE`; timed stays
  unanchored; `OP_GET_WALLTIME` ⇒ `STATUS_UNAVAILABLE`; UI shows `--:--`;
  the walltime markers never fire (run fails honestly).
- No silent fallback to guessed/bogus epochs anywhere.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p tz-lite -p nexus-wire -p settingsd
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `rtcd: ready` — successful goldfish MMIO time read
- `timed: walltime anchored` — anchor set from rtcd
- `SELFTEST: walltime rtc ok` — epoch > 2020-01-01 and monotonic-consistent
  across two reads
- `SELFTEST: clock tz ok` — fixed epoch converts correctly for two zones

## Alternatives considered

- **Separate `clockd`** — rejected: a second time authority violates the
  one-authority registry; timed already owns time.
- **Timezone conversion inside timed** — rejected: per-client zone state in a
  service vs pure client-side const math; UTC-only keeps timed minimal.
- **VSYNC-coupled clock tick** — rejected: 60 Hz wakeups for a 1/min display.

## Open questions

- None blocking.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: RTC read path (deviation: `rtc-goldfish` lib in timed, no rtcd service) — proof: MMIO grant + anchor
- [x] **Phase 1**: timed anchor + OP_GET_WALLTIME — proof: `timed: walltime anchored`, `SELFTEST: walltime rtc ok` (green 2026-07-21)
- [x] **Phase 2**: tz-lite — proof: `cargo test -p tz-lite` DST goldens + `SELFTEST: clock tz ok` + settingsd pin test (green)
- [x] **Phase 3**: live clock UI + Settings pickers — proof: `apphost: clock tick applied` (visible boot 2026-07-21); tz-switch interactive
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`: foreign rtcd caller, malformed frames).
