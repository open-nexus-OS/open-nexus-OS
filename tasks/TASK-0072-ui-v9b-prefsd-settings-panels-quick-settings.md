---
title: TASK-0072 UI v9b: prefsd persistent store + Settings panels (stubs but functional UI) + Quick Settings wiring
status: Draft
owner: @ui
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v8b quick settings overlay baseline: tasks/TASK-0070-ui-v8b-wm-resize-move-shortcuts-settings-overlays.md
  - UI v5b theme tokens baseline (theme switch): tasks/TASK-0063-ui-v5b-virtualized-list-theme-tokens.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Config broker (bridge for selected keys): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy as Code (prefs writes): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - UI v9a searchd (settings routes registration): tasks/TASK-0071-ui-v9a-searchd-command-palette.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

We need a durable, policy-guarded preferences store to back:

- Settings UI (Network/Display/Accounts stubs but functional UI),
- Quick Settings toggles,
- and a bridge to configd for keys that require 2PC-safe reload (e.g., theme mode, refresh rate).

This task delivers prefs + Settings panels + Quick Settings wiring. Search/palette is v9a.

Update note (to avoid drift):

- The repo now has an explicit `settingsd` direction (typed keys, scopes, provider apply hooks).
- Settings v2 is tracked as `TASK-0225` (host-first typed settingsd) and `TASK-0226` (OS UI/deeplinks/search/guides).
- This task should be treated as **deprecated/superseded** for new work: do not introduce a new `prefsd`
  JSON store if `settingsd` is the chosen canonical substrate.

## Goal

Deliver:

1. `prefsd` service + `nexus-prefs` client:
   - Value store keyed by string (API stays typed/IDL-driven; storage is not JSON-as-contract)
   - subscribe by prefix
   - backed by `/state/prefs.nxs` (Cap'n Proto snapshot) with atomic write
   - (optional) `nx prefs export --json` emits deterministic `prefs.json` as a derived view for debugging/tooling
2. Selected config bridge:
   - for keys like `ui.theme.mode`, `ui.display.hz` (host-first; OS-gated)
3. Settings app panels:
   - Network/Display/Accounts routes (stubs but real UI)
   - applying changes writes prefs and triggers bridge where applicable
4. Quick Settings wiring:
   - toggles/sliders write prefs
   - markers for applied keys
5. Host tests and OS/QEMU markers.

## Non-Goals

- Kernel changes.
- Real Wi‑Fi/Bluetooth/brightness/volume backends (prefs only for now).
- A full account system.

## Constraints / invariants (hard requirements)

- Deterministic storage semantics:
  - atomic write (temp + rename),
  - corrupt temp ignored,
  - bounded file size and JSON depth.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Policy guardrails:
  - only Settings/SystemUI (focused) can write certain keys,
  - audit all writes.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_v9b_host/`:

- prefs set/get/subscribe roundtrip
- atomicity: crash between temp write and rename does not corrupt committed prefs
- quick settings wiring: applying key writes prefs and triggers bridge hook (mocked)

### Proof (OS/QEMU) — gated

UART markers (order tolerant):

- `prefsd: ready`
- `prefs: set (key=..., size=...)`
- `systemui: quick settings apply (key=..., val=...)`
- `settings: open (route=...)`
- `settings: apply (key=value)`
- `SELFTEST: ui v9 prefs ok`
- `SELFTEST: ui v9 quick ok`

## Touched paths (allowlist)

- `source/services/prefsd/` (new)
- `userspace/prefs/nexus-prefs/` (new)
- `source/apps/settings/` (functional stub UI)
- SystemUI quick settings overlay wiring
- `tests/ui_v9b_host/`
- `source/apps/selftest-client/`
- `tools/postflight-ui-v9b.sh` (delegates)
- `docs/platform/prefs.md` + `docs/dev/ui/settings.md`

## Plan (small PRs)

1. prefsd + client + atomic file semantics + markers
2. config bridge for selected keys (gated)
3. settings panels + route registration to searchd (gated on v9a)
4. quick settings wiring + markers
5. tests + OS selftest markers + docs + postflight
