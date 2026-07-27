---
title: TASK-0307 Settings distribution v2 — single authority, versioned snapshots, repaint not remount
status: In Progress (2026-07-27) — P0-P5 done (legacy paths deleted; P6 proof next)
owner: @runtime @ui
created: 2026-07-27
links:
  - RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md
  - ADR: docs/adr/0053-settingsd-single-settings-authority.md
  - Amends: docs/rfcs/RFC-0078-settings-region-keys-watch.md
  - Evidence: build/logs/manual--2026-07-27T09-02-33/uart.log
  - Predecessor: tasks/TASK-0306-ui-responsiveness-input-integrity-smp-rendezvous.md
---

## Context

One accent tap in the settings app produced (all counted in the evidence log):
3 full app remounts (`re-theme old app dropped` ×3, initial effects re-run,
`session.users`/`bundlemgr.enumerate` re-fetched), `inputd: push backpressure`
n=1..36, `settingsd: statefs reply timeout` → `persist=fail`, one
`WINDOWD: FAIL app event send`. User-visible: "changed the color and nothing
works any more". Locale, theme-mode and shell-mode switching hit the same
class before. The causes are architectural; the full defect table with
file:line lives in RFC-0083 §Context.

## Goal

A settings change is a bounded repaint delivered exactly-once-or-healed:
no remount, no effect re-run, no input backpressure, no compositor blocking,
no yield-spins, persistence async + coalesced + honestly reported.

## Non-Goals

statefsd internals; per-key write ACLs; `_timeout_ms` wiring of DSL svc calls
(all recorded follow-ups).

## Constraints / invariants

- Every phase lands alone green: `just check` + `just diag` + `just test-host`,
  and the boot lanes for P3+ (`just ci-os-headless`).
- Receiver before sender; dual-emit + dual-authority during migration; retire last.
- Marker truth (no-fake-green): `settingsd: persist ok` only on a real statefsd ACK.
- Gate-pinned markers that must stay green: `SELFTEST: settings watch ok`,
  `SELFTEST: i18n switch ok`.
- Structure ratchet: new code goes into new modules (`settingsd/src/persist.rs`,
  `app-host/src/presentation.rs`, `windowd/runtime/presentation.rs`) — the three
  at-cap files (`app-host/main.rs`, `effect_host.rs`, `windowd/runtime/mod.rs`)
  must not grow.

## Touched paths (per phase)

- P1: `source/services/settingsd/src/{os_lite,watch,statefs_client,lib}.rs` + NEW `persist.rs`
- P2: `source/libs/nexus-display-proto/src/surface_text.rs` (approval zone),
  `source/services/app-host/src/main.rs` + NEW `src/presentation.rs`, `src/probe/{clock,interaction}.rs`,
  `userspace/dsl/**` only if ProfileEvent needs registry plumbing (KeymapEvent precedent says no)
- P3: `source/services/windowd/src/compositor/runtime/{region,shell,app_window}.rs`
  + NEW `runtime/presentation.rs`, `compositor/mod.rs`, `src/{client_surface,settings_client}.rs`
- P4: `source/services/app-host/src/{effect_host,effect_ime}.rs`,
  `tools/nxb-pack/src/main.rs` + `tests/payload_kind.rs`,
  `userspace/apps/desktop-shell/manifest.toml`
- P5: deletions across windowd/app-host + `source/init/nexus-init/src/bootstrap/route_provision.rs`
- P6: `scripts/qemu-test.sh` (only if new assertions), docs sweep

## Plan / stop conditions

1. **P0 — contracts.** RFC-0083 + ADR-0053 + this ledger + indexes. DONE when
   indexed and consistent.
2. **P1 — settingsd reactive.** Reply-before-persist; persist state machine
   (NONBLOCK PUT, in-flight flag, SF-magic drain at loop top, `Wait::Timeout`
   only while pending, coalescing, bounded backoff); registration burst
   (resync-flagged current values); server-side resync heal. STOP: host tests
   prove (a) SET during persist-in-flight is answered immediately, (b) N rapid
   SETs coalesce to ≤2 PUTs, (c) burst delivers current values on register,
   (d) a dropped watcher send heals on the next change. Markers split.
3. **P2 — wire + receive.** OP_SURFACE_SETTINGS=26 codec + reject matrix;
   app-host `presentation.rs` applies gen-idempotently: theme/accent/profile →
   reemit (NOT remount; profile also re-announces window intent + dispatches
   `ProfileEvent::Changed`), locale → apply_region, hour/tz → re-tick. STOP:
   dsl_apps_conformance proves accent snapshot changes scene colors with store
   state + View identity preserved; profile snapshot changes structure WITHOUT
   re-running initial effects.
4. **P3 — windowd.** Watch events on own server endpoint ('S','T' arm =
   wake source); subscribe `time.`/`ui.`/`input.keymap` staggered; drain ≥8;
   PresentationState + `push_presentation()` (NONBLOCK + per-channel needs_push
   + frame retry + desktop dedupe + failure reclaim); dual-emit; ThemeProbe
   deleted; pinned delivery test rewritten to "never lost, never blocking".
   STOP: ci-os-headless green; interactive boot shows theme+locale switch with
   zero `FAIL … push` and no probe markers.
5. **P4 — authority flip.** nxb-pack ceiling `settings|shell` + shell manifest
   cap FIRST; app-host routes all settings keys to settingsd; call_reply spin →
   kernel-parked deadline send/recv (250 ms). STOP: control-center toggles work
   in the interactive boot; no windowd CONTROL writes remain in app-host.
6. **P5 — retirement.** Legacy ops/arms/probes/slots deleted; ops 16/17/23
   documented reserved. STOP: grep-clean tree, all gates green.
7. **P6 — boot proof.** Interactive script: rapid accent/theme/locale/profile
   switching. STOP: `apphost: presentation gen=N applied` present; negative
   grep `re-theme old app dropped` + `profile old app dropped` after switches;
   no `inputd: push backpressure` burst; gated SELFTEST markers green;
   `settingsd: persist ok` arrives async. Before/after measurement on the same
   script recorded here.

## Evidence

### P5 — retirement + cleanup (2026-07-27)

Deleted, not just dormant (net -725 lines (120+ / 845-)): windowd's CONTROL_THEME/ACCENT/
SHELL_PROFILE arms (old senders get the unknown-control report),
`push_app_theme`/`push_app_profile`/`send_attach_pushes`/
`push_region_to_surfaces`/`push_settings_frame`/`region_frame`;
`settings_client` reduced to the cached route lookup the watch subscription
needs (its header rewritten to match); `set_shell_profile_wire` lost its
`persist` flag (apply-only forever). app-host: BOTH remount arms deleted —
`mount_restoring` survives only for the initial mount; the boot wait and
every race-stash decode OP 26 only (buffers 64→96, a 70-byte snapshot no
longer truncates into an undecodable frame); `presentation_control` is
window-controls-only. Wire: codecs for 16/17/23 deleted, the op numbers and
control values 0/1/3 documented reserved-forever and PINNED in a test;
`pack_theme`/`THEME_*`/`PROFILE_*`/`REGION_*` live on as snapshot vocabulary.

Deliberately NOT retired: the 0x40/0x41 watch side channel — P3's measured
kernel finding made it the shipping mechanism (the plan's retirement bullet
assumed in-band delivery worked).

Boot (14-25-37): snapshot-only end to end — `APPHOST: settings received`
(boot wait consumed OP 26), ZERO legacy `theme/profile received`, 5 gen
markers, locale applied, all bursts `sync 5/5|2/2|1/1`, ladder green
(chronic `qos FAIL` only). Tree grep-clean of every deleted symbol.

### P4 — authority flip (2026-07-27)

Landed in order: nxb-pack SETTINGS ceiling widened to `settings | shell`
(+ pinned in `payload_kind.rs`: a shell MAY hold SETTINGS, a plain app still
may NOT); `nexus.permission.SETTINGS` added to the desktop-shell manifest
(measured in boot: `abilitymgr: caps ok app=desktop-shell (n=5)`); app-host
routes EVERY settings key to settingsd — only `window.control` still goes to
the compositor; `call_reply`'s 2 s NONBLOCK+yield spin became a KERNEL-PARKED
send+recv on one 250 ms absolute deadline (settingsd replies in µs since P1),
and `send_fire_and_forget`'s spin got the same treatment. windowd's CONTROL
arms stay as dual authority until P5 (idempotent-guarded, loop-safe).

Boot (14-01-10): green — ladder runs, only the chronic `qos FAIL`, snapshot
pipeline alive (6 gen markers), ZERO remount markers. Honest limit: the
headless script parks at the greeter (no login), so the live control-center
toggle-through-settingsd proof is P6's interactive script; the mechanism is
the settings app's already-proven settingsd route, now granted to the shell
by the same table.

En route: a botched python edit spliced a function into the file header of
`effect_host.rs`; caught by inspection before any gate ran, repaired, and
the edit re-done against unique anchors.

### P3 — windowd: subscribe, fold, deliver (2026-07-27)

Landed: `presentation_state.rs` (pure per-channel due/retry/reclaim core,
host-tested — the `Delivery` precedent) + `runtime/presentation.rs`
(snapshot fold + `pump_presentation`: one NONBLOCK attempt per due channel
per frame, desktop-alias dedupe, rebind hooks in the channel attach + both
desktop binds); windowd subscribes `time.`/`ui.`/`input.keymap` (staged, one
per frame — `ui.` covers theme/accent/shell-mode, so the registration burst
IS the boot restore); drained events go through ONE apply path
(`handle_settings_event`: theme → `set_theme_mode` incl. wallpaper swap,
accent, shell profile, region); dual-emit OP 26 + legacy; **ThemeProbe
deleted** (3 blocking GETs + 24×250 ms cadence + the handoff GET — the
pacer disarms earlier); `settings_client` GET helpers + `toggle_theme` +
`mark_theme_user_set` deleted; pinned delivery test rewritten (settings =
retained latest-wins, never lost, never blocking; `Critical` stays for taps
and the transitional legacy pushes).

**Two boot-gate catches en route, both fixed before landing:**

1. The attach-time OP 26 snapshot was consumed pre-mount by
   `wait_for_boot_pushes` and DROPPED while windowd recorded it delivered —
   zero `presentation gen=` markers. The boot wait now decodes OP 26 (it
   carries theme+profile+region in one) and seeds `last_settings_gen`.
2. **Watch-as-wake-source is NOT shippable with today's kernel caps — the
   plan's finding-3 fix is deferred with evidence.** Cloning windowd's own
   server slots for the watch push caps: settingsd's sends fail
   `PermissionDenied` (init-granted service-pair caps are process-pinned).
   Re-provisioning slot 0x41 as a SEND half of windowd's request endpoint:
   registration + burst delivery WORK (`sync 5/5`), but the system then
   wedges deterministically — the Idle-class selftest ladder starves and
   never runs (boots 11-31-56, 13-32-13; bisected green by reverting only
   the provisioning hunk, 13-38-26). Mechanism unexplained = KERNEL
   FOLLOW-UP recorded here and in route_provision.rs. Shipped instead: the
   dedicated side channel, drained per frame through the same apply path,
   plus a bounded 500 ms idle tick (2 wakes/s) so an idle compositor applies
   a settings change within half a second.

Final boot (13-42-57): ladder runs, only the chronic `qos FAIL`; all bursts
delivered (`sync 5/5|2/2|1/1`); **6 × `apphost: presentation gen=N applied`**
(gen 5→8 tracking the selftest keymap flips) — the versioned snapshot
pipeline is live end to end. Gates green.

### P2 — wire + app-host receive (2026-07-27)

`OP_SURFACE_SETTINGS = 26` landed in its own module
(`nexus-display-proto/src/surface_settings.rs`, the structure gate refused to
let `surface_text.rs` grow past 600) with roundtrip + reject matrix
(truncation anywhere, lying length, trailing garbage, wrong op — all fail
closed). app-host receive: `probe/presentation.rs` applies gen-idempotently —
region via the existing RFC-0076/0077 recipe, **theme/accent and profile via
reemit**; `ProfileEvent::Changed` is the data-reload hook (KeymapEvent
precedent, no runtime plumbing needed). Legacy arms 16/17/23 untouched
(windowd still emits them until P3); `absorb_snapshot` keeps the remount
re-apply state coherent during the migration window.

Plan correction, verified at the source: window intent needs NO re-announce
on a profile flip — it is a program constant (`root.get_window()`), not a
profile-dependent value.

Host proofs (`tests/dsl_apps_conformance/tests/shell_presentation.rs`,
against the REAL compiled shell):
- token swap recolors the scene, the OPEN Control Center survives, and the
  effect host records ZERO service calls;
- profile reemit reproduces a fresh desktop mount's structure **byte-exact**
  (box-rect fingerprint) with the open panel carried across, zero effects,
  and the round trip restores the tablet structure. The compiler's
  platform-override merge (`project.rs`: `ui/platform/<p>` pages become
  `device.profile == <p>` arms) is what makes this work — the first test
  draft compared i18n-empty TEXTS and false-negatived; box fingerprints are
  the honest discriminator.

Structure ratchet forced two real splits: the codec file, and `DslApp` out of
`main.rs` into `probe/state.rs` (state vs loop logic; main.rs 1290 → 1207,
baseline ratcheted DOWN). Gates green; regression boot equivalent to the P1
reference boot (1 mount, shell on, only the chronic `qos FAIL`).

### P1 — settingsd reactive (2026-07-27)

Landed: reply-before-persist; `persist.rs` state machine (one PUT in flight,
coalescing, bounded backoff, **500 ms spacing floor between PUTs**);
registration burst; server-side resync heal; statefs client rebuilt (cached
routes, NONBLOCK PUT + reply drain, boot GET kernel-parked instead of
yield-spinning); marker split (`set key=…` immediate, `persist ok|fail` on
real outcome); bounded delivery diagnostics (`watch registered (sync D/M)`,
`event send FAIL chan= err=`).

**The spacing floor was found by the boot gate, not designed in.** First P1
boot: `SELFTEST: i18n switch FAIL` plus a policyd-flake denial of settingsd's
own prefs PUT. Three boots without the floor: i18n FAIL once, `device key
persist FAIL` + `keystored capmove FAIL` twice — none of these appear in any
pre-P1 boot. Mechanism: the OLD code's 500 ms in-handler stall was an
accidental THROTTLE; removing it let the boot keymap selftest turn five SETs
in 160 ms into five back-to-back full-blob PUTs against a statefsd that pays
a policyd round-trip + journal write per request — starving the keystored
probes' bounded waits. The floor restores the spacing WITHOUT the stall
(clients still get µs replies) and collapses the burst to 3 PUTs. Two boots
with the floor: only the chronic `qos FAIL` (present in every boot for days),
every burst `sync 1/1|2/2`, `persist ok` throughout, i18n/watch/device-key/
keystored green.

Host proof: 22 settingsd tests (coalescing incl. the floor, timeout/backoff/
recovery, burst, heal, reclaim). Gates: `just check` + `just diag` +
`just test-host` green. Also fixed en route: the nexus-wire
`STATUS_PERSIST_FAIL` doc claimed rollback semantics SET never had.

### P0 — contracts (2026-07-27)

RFC-0083 seeded + indexed; ADR-0053 seeded + indexed; this ledger. The defect
table (7 entries, file:line) lives in the RFC; the validation pass that shaped
the phasing found five design holes that are now contract constraints:
receiver-before-sender, dual-authority window, watch-as-wake-source (idle
windowd never drained the side channel), pack-ceiling widen for the shell
(it has NO settings route today — theme writes only worked through the
windowd slot every app holds), and heal-server-side-only (re-WATCH leaks
watcher slots).
