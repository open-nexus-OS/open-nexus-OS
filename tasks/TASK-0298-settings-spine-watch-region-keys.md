---
title: TASK-0298 Settings spine: region/keymap/time keys + OP_WATCH push propagation + General-management pickers
status: Draft
owner: @runtime
created: 2026-07-21
depends-on: []
follow-up-tasks:
  - TASK-0297 (time keys consumer), TASK-0241 (locale consumer), TASK-0147/0204 (ime keys)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract seed: docs/rfcs/RFC-0078-settings-region-keys-watch.md (seeded by this task)
  - Registry baseline (Done via TASK-0072): source/services/settingsd/src/registry.rs
  - Settings v2 ledger reconciliation: tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Keymap tables (consumer target): tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

settingsd already has the typed registry, validation, prefs-blob persistence
and OP_GET/OP_SET wire (landed with TASK-0072 Phase 8; the old TASK-0225 plan
predates this). Two gaps block the "General management" goals:

1. **Missing keys**: only 5 exist (theme/accent/shell/font/`ui.locale`);
   keyboard layout, country, timezone, hour format have no registry rows.
   The legacy host-only `InputSettingsSnapshot` keymap concept never reached
   the live registry.
2. **No change propagation**: consumers re-read on their own cadence; theme
   only works because windowd is its apply authority. Keymap (inputd) and
   region data (windowd relay) need a push primitive.

## Goal

1. **New SPECS rows** (all TYPE_TEXT, validated, persisted via the existing
   prefs blob):
   - `region.country` — default `DE`, validator: exactly 2 ASCII uppercase
   - `input.keymap` — default `us`, validator: `us|de|jp|kr|zh`
   - `time.zone` — default `Europe/Berlin`, validator: membership in the
     tz-lite zone table (TASK-0297; until tz-lite lands, a const mirror list
     with a test pinning it to tz-lite)
   - `time.format` — default `24h`, validator: `24h|12h`
   - `ime.personalization` — default `on`, validator: `on|off` (TASK-0204)
2. **`OP_WATCH=3`** in the settingsd wire + service: request carries a key
   prefix (str8, 1–64 B); settingsd pushes `OP_EVENT=4` frames
   (`flags, key, value`) on every applied change matching the prefix.
   Bounded: ≤ 8 subscribers, per-subscriber queue with drop-oldest +
   `resync` flag (bit0) so a starved watcher re-reads via OP_GET.
3. **inputd consumer**: watch `input.` → `Keymap::set_layout_name` (exists)
   applies live; emits `inputd: keymap set <name>` marker on change.
4. **Settings app**: General management pane becomes real — country picker +
   keyboard-layout picker (accent-chip row pattern from the personal pane),
   wired via `svc.settings.set`. (Language/timezone pickers follow in
   TASK-0241/0297 on this spine.)

## Non-Goals

- No windowd region relay / `OP_SURFACE_REGION` (TASK-0241).
- No new settings storage format; no configd/2PC integration; no deep links
  (TASK-0226 unchanged).
- No per-key ACLs yet — writes remain UI-path-only as today; policyd-gated
  writes are an RFC-0078 documented follow-up.

## Constraints / invariants (hard requirements)

- Additive wire change: VERSION stays 1, unknown ops still reject; existing
  OP_GET/OP_SET goldens byte-identical.
- Watch is bounded everywhere: subscriber cap, prefix length, queue depth,
  frame sizes; drop-oldest + resync — a slow watcher can never wedge settingsd.
- Validators fail-closed (INVALID_VALUE), defaults never persisted (existing
  blob semantics).
- No per-event allocation in the push path (fixed frame buffers).

## Security considerations

### Threat model
- Malformed watch requests; watch-queue exhaustion; a foreign service
  spoofing pushed values to consumers.

### Security invariants (MUST hold)
- Consumers trust `OP_EVENT` only on the connection they opened to settingsd
  (kernel-routed reply path — no broadcast spoofing surface).
- Prefix bounded before matching; subscriber slots capped; `test_reject_*`
  for oversized prefix, subscriber overflow, and malformed event frames.
- Settings values are non-secret by charter — nothing sensitive may be added
  to this registry without a policyd gate (pinned in RFC-0078).

## Contract sources (single source of truth)

- **Wire contract**: nexus-wire settingsd goldens (existing + new watch ops).
- **Key schema**: RFC-0078 (names, defaults, validators, consumer map).
- **QEMU marker contract**: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`.

## Stop conditions (Definition of Done)

- **Proof (host)**: registry tests for all new keys/validators; watch unit
  tests (match, drop-oldest+resync, unsubscribe on disconnect); codec
  goldens + reject matrix for OP_WATCH/OP_EVENT.
- **Proof (QEMU)**:
  - `SELFTEST: settings watch ok` — set → watcher receives push
  - `inputd: keymap set de` — layout applied live
- **Proof (interactive)**: `just start` — pick DE layout in Settings →
  typing immediately yields z/y swap + umlauts (with TASK-0147 typing path).
- **Gates**: `just check`, `just test-all` green; RFC-0078 checklist ticked.

## Touched paths (allowlist)

- `source/services/settingsd/` (registry rows + watch spine, src/ + tests/)
- `source/libs/nexus-wire/src/settingsd.rs` (OP_WATCH/OP_EVENT) — **approval zone**
- `source/services/inputd/` (watch consumer)
- `userspace/apps/settings/` (General pane pickers)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` — **approval zone**
- `docs/rfcs/RFC-0078-*.md` (new seed) — **approval zone**
- `docs/**` settings pages, `CHANGELOG.md`

## Plan (small PRs)

1. RFC-0078 seed; new keys + validators + host tests.
2. OP_WATCH/OP_EVENT codec + settingsd spine + reject matrix + selftest.
3. inputd watch consumer + marker; Settings General pickers + interactive proof.

## Acceptance criteria (behavioral)

- Changing keyboard layout/country in Settings applies live (no reboot),
  persists across reboot via the prefs blob, and invalid values are rejected
  at the wire with INVALID_VALUE.
