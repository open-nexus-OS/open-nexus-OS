# RFC-0083: Settings distribution v2 — single authority, versioned snapshots, repaint not remount

- Status: Complete (2026-07-27 — all phases boot-proven, driven-tap interactive proof in TASK-0307 §P6; follow-ups recorded there)
- Owners: @runtime
- Created: 2026-07-27
- Last Updated: 2026-07-27
- Links:
  - Tasks: `tasks/TASK-0307-settings-distribution-v2.md` (execution + proof)
  - ADRs: `docs/adr/0053-settingsd-single-settings-authority.md`
  - Related RFCs: `docs/rfcs/RFC-0078-settings-region-keys-watch.md` (amended by this RFC),
    `docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md`,
    `docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md`

## Status at a Glance

- **Phase 1 (settingsd reactive: reply-before-persist, burst, heal)**: ✅
- **Phase 2 (OP_SURFACE_SETTINGS wire + app-host reemit receive)**: ✅
- **Phase 3 (windowd: snapshot fold + push_presentation; watch-as-WAKE-source
  DEFERRED with measured kernel evidence — side channel + bounded idle tick
  shipped instead, see TASK-0307 P3)**: ✅
- **Phase 4 (authority flip: all settings writes → settingsd)**: ✅
- **Phase 5 (retirement + deletion of legacy ops + CONTROL writes)**: ✅
- **Phase 6 (boot proof + docs sweep)**: ✅ (driven-tap interactive proof,
  TASK-0307 P6)

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The settings authority model: settingsd owns ALL settings state (live + persisted),
    including `ui.theme.mode`, `ui.theme.accent`, `ui.shell.mode`.
  - The distribution contract: writes flow app → settingsd; notifications flow
    settingsd → subscribers (windowd) → surfaces. windowd never writes settings.
  - The wire contract `OP_SURFACE_SETTINGS = 26` (versioned presentation snapshot)
    and the retirement (reserved-forever) of `OP_SURFACE_THEME = 16`,
    `OP_SURFACE_PROFILE = 17`, `OP_SURFACE_REGION = 23`, and the surface-control
    writes `CONTROL_THEME`, `CONTROL_THEME_ACCENT`, `CONTROL_SHELL_PROFILE`.
  - The watch amendment to RFC-0078: registration burst + server-side resync heal
    + the "burst per prefix ≤ subscriber queue depth" constraint.
  - The receiver semantics: settings changes are **repaints (reemit), never remounts**;
    `@effect on Load` does NOT re-run on a profile switch — programs that need
    profile-dependent data declare `ProfileEvent::Changed`.
- **This RFC does NOT own**:
  - statefsd internals (per-write policyd round-trip, virtio probe, journal batching)
    — documented follow-up, separate track.
  - Per-key write ACLs in settingsd (`sender_service_id`-based) — documented follow-up.
  - Window controls (`CONTROL_WIN_MINIMIZE/CLOSE/MODE`) — genuine windowing, stays
    on the windowd surface channel.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0307-settings-distribution-v2.md` defines stop conditions and proof
  commands per phase; this RFC records the contracts those phases implement.

## Context

One accent-color tap in the settings app (boot `manual--2026-07-27T09-02-33`) caused:
three app-hosts tearing down and remounting their entire app (re-running initial
effects, re-fetching `session.users` / `bundlemgr.enumerate`), a 36-event input
backpressure burst, a settingsd persist timeout (`persist=fail`), and a lost ack.
The user experience: "changed the color and nothing works any more". The same defect
class previously hit locale, theme-mode and shell-mode switching.

The root causes are architectural, all verified at file:line in the task ledger:

1. **Split authority.** `settings.set` of `ui.theme.mode`/`ui.theme.accent`/
   `ui.shell.mode` goes to WINDOWD as a surface-control frame; every other key goes
   to settingsd. `settings.get` ALWAYS goes to settingsd — GET and SET of the same
   key take different paths and can disagree.
2. **Yield-spins instead of kernel parks.** app-host spins up to 2 s per settings
   call inside the DSL dispatch; windowd spins up to 500 ms per persist on the
   COMPOSITOR thread; settingsd spins up to 500 ms per statefsd round-trip.
3. **Reply gated behind persist.** settingsd answers a SET only after the full
   statefsd blob PUT (which itself does a synchronous policyd round-trip per write).
4. **Lossy pushes with no detection.** Theme/profile pushes are fire-and-forget
   NONBLOCK with the result discarded; the watch layer's resync flag exists but the
   only subscriber ignores it; there is no generation concept anywhere, so a missed
   delta is permanent silent drift.
5. **Remount instead of repaint.** A theme byte change tears down the whole DSL app,
   although colors are resolved at emit time from the `Tokens` trait and
   `reemit` + `relayout_retained` demonstrably suffices (the locale and size-class
   paths already do exactly this).
6. **Watch events do not wake an idle windowd** (they land on a side channel pumped
   once per frame) and **boot restore is a poll** (3 blocking GETs + 24×250 ms probe
   cadence that keeps the 120 Hz pacer armed).

## Goals

- One authority, one direction of flow, one delivery semantic for all settings.
- A settings change is a **bounded repaint**: no remount, no effect re-run, no
  service re-fetch, no input backpressure.
- Reactive end to end: kernel-parked waits, watch events as wake sources, zero
  yield-spin loops, zero polling where a wake source exists.
- Self-healing delivery: a lost notification converges at the next push (versioned
  idempotent snapshots), never silent permanent drift.
- Persistence is asynchronous and coalesced; its failure is visible (`persist ok|fail`
  marker on actual completion — no-fake-green) and never blocks a client.

## Non-Goals

- Hardening statefsd internals (follow-up track).
- Per-key write ACLs (follow-up; the authority flip already improves on today's
  "any surface client can flip the global theme with zero permission").
- Multi-seat / per-user settings.

## Design

### Authority and flow

```
app (DSL svc.settings.set) ──► settingsd  (validate → apply live → reply OK
                                           → notify watchers → async persist)
                                   │ watch events ('S','T' OP_EVENT)
                                   ▼
                               windowd     (fold into PresentationState,
                                            apply own chrome: wallpaper/profile)
                                   │ OP_SURFACE_SETTINGS = 26 (gen, snapshot)
                                   ▼
                               app-hosts   (gen-idempotent apply: reemit, never remount)
```

### Wire: `OP_SURFACE_SETTINGS = 26` (`'I','N'` envelope)

```
[hdr:4][gen:u32 LE][theme:u8 = pack_theme(mode, accent)][profile:u8][hour_fmt:u8]
[locale_len:u8][locale ≤16][tz_len:u8][tz ≤32][keymap_len:u8][keymap ≤8]
```

Max frame 70 bytes. `theme` reuses the existing packed byte (low nibble mode, high
nibble accent index). Decode is fail-closed (bounds + UTF-8); future fields append
as optional tails (the keymap-tail precedent). The generation is **windowd-local**,
monotonic wrapping u32 — kernel IPC is FIFO per channel, so ordering is structural;
gen serves as a short-circuit and dedupe token only. The settingsd `OP_EVENT` frame
stays **byte-identical** (three deployed decoders; a gen there buys nothing).

Ops 16 (`OP_SURFACE_THEME`), 17 (`OP_SURFACE_PROFILE`) and 23 (`OP_SURFACE_REGION`)
are retired after the migration and their numbers reserved forever. The control
values `CONTROL_THEME`/`CONTROL_THEME_ACCENT`/`CONTROL_SHELL_PROFILE` are likewise
retired (window controls stay).

### Watch amendment (RFC-0078)

- **Registration burst**: on `OP_WATCH`, settingsd immediately sends the current
  value of every matching key to THAT watcher, resync-flagged. This is the boot
  restore path (replaces windowd's ThemeProbe polling) and fixes the latent inputd
  bug where a persisted keymap was ignored at boot.
- **Server-side resync heal**: the next change reaching a resync-flagged watcher
  re-sends ALL matching current values, not just the changed key. Healing by
  re-WATCH is forbidden — every registration moves a fresh cap and would leak
  watcher slots (`MAX_WATCHERS = 8`, dedupe is by channel number).
- **Constraint**: a registration burst per prefix must fit the subscriber's queue
  depth (8 today); subscribers stagger multi-prefix registration (one WATCH per
  frame) and drain ≥ queue-depth events per pump.

### Delivery classes (amends the TASK-0306 taxonomy)

Settings pushes are **retained latest-wins state**, not transient intent:
first attempt NONBLOCK; on failure set a per-channel `needs_push` flag and retry
next frame; bounded failure counter reclaims dead channels. The compositor thread
never parks for a settings push. The invariant is "a settings push is never LOST",
not "a settings push blocks". (Taps remain `Critical` — user intent with no
recovery path. Hover remains `Coalescing`.)

### Receiver semantics (app-host)

- gen equal → skip. Otherwise field-compare and apply:
  - **theme/accent** → tokens swap + `reemit` + `relayout_retained` + `anim_sync`.
  - **profile** → same reemit recipe (`if device.*` arms re-select structure —
    the size-class precedent), then re-derive/announce the window intent, then
    dispatch `ProfileEvent::Changed(tag)` into programs that declare it
    (the `KeymapEvent::Changed` precedent). **`@effect on Load` does not re-run.**
  - **locale** → existing `apply_region` recipe; **hour/tz** → clock re-tick.
- Store state survives by construction (no remount). The remount paths for theme
  and profile are deleted.

### windowd

- Watch events arrive on windowd's OWN server endpoint (registration passes a
  `cap_clone` of its send half), so a settings event IS a wake — no polling, no
  idle-miss. The `'S','T'` magic is disjoint from the `'I','N'` surface envelope.
- windowd folds settings into one `PresentationState {gen, theme, profile, hour,
  locale, tz, keymap}` and applies its own chrome from the same events (wallpaper
  swap, plane-0 rewrite, `OP_WALLPAPER_DIRTY` — unchanged).
- The ThemeProbe (3 blocking GETs + 24×250 ms cadence) is deleted; initial values
  arrive via the registration burst; code defaults hold until then.

## Security

- Today any surface client can flip the global theme with zero permission (every
  windowed app holds the windowd request slot). After the flip, settings writes
  require a granted settingsd route (`nexus.permission.SETTINGS`, pack-time ceiling
  widened from `settings` to `settings | shell` for the control center).
- Per-key write ACLs keyed on `sender_service_id` are the recorded follow-up.
- Identity remains kernel-provided; payloads are bounded before parsing (all new
  decode paths fail closed with reject-matrix tests).

## Compatibility / migration

Receiver-first, dual everything, retire last:

1. app-host learns OP 26 while legacy arms stay (P2).
2. windowd dual-emits OP 26 + legacy 16/17/23; CONTROL arms stay as dual authority
   (all applies are idempotent-guarded, so the settingsd echo cannot loop) (P3).
3. The write path flips to settingsd (P4) — requires the pack-ceiling widen first,
   or the control center goes dead.
4. Legacy emission, decode arms, CONTROL writes, ThemeProbe remnants and the
   0x40/0x41 watch-slot provisioning are deleted (P5).

Markers: only `SELFTEST: settings watch ok` and `SELFTEST: i18n switch ok` are
gate-asserted; the selftest's event drain is burst-tolerant (verified). The
`settingsd: set … persist=` marker splits: `set key=…` immediately,
`persist ok|fail` only on actual statefsd completion.

## Open questions

- None blocking. Follow-ups recorded under Non-Goals.
