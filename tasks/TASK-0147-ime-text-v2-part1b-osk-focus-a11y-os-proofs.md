---
title: TASK-0147 IME v2 Part 1b (OS/QEMU): imed service real + typing lands in apps + OSK overlay app (ime-ui)
status: In Progress (Part 1 DONE + live-proven 2026-07-21; Part 2 OSK next)
owner: @ui
created: 2025-12-26
updated: 2026-07-21 (rewritten against repo reality; architecture per RFC-0075)
depends-on:
  - TASK-0146
  - TASK-0253
follow-up-tasks:
  - TASK-0149 / TASK-0150 (CJK + candidate UI)
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md
  - Host foundation: tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md
  - Live input chain (Done): tasks/TASK-0253-input-v1_0b-os-hidraw-touch-inputd-windowd-ime-nx.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Settings keymap key (consumer): tasks/TASK-0298-settings-spine-watch-region-keys.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

TASK-0146 delivers ime-core, the DSL focused-field model and the wire codecs —
all host-proven. This task makes typing real in QEMU, in two OS packages:

- **Part 1 (typing lands in apps)**: imed becomes a real service; the resolved
  characters inputd already computes (and currently drops) flow
  `inputd → imed → windowd → focused surface → app-host → focused DSL field`.
- **Part 2 (OSK)**: an on-screen keyboard as its **own DSL overlay app**
  (`ime-ui`) — windowd only composites and shows/hides it. The legacy shell
  keyboard-card visual is retired.

Authority cleanup: `source/services/ime/` (deprecated placeholder,
TRACK-AUTHORITY-NAMING) is deleted in Part 1.

## Goal

### Part 1 — typing lands in apps (first visible win)
1. `source/services/imed/` real: samgrd registration, `frames!` imed protocol
   serve loop, hosts ime-core, fixed buffers (no per-key allocation), src/ + tests/.
2. inputd: forward resolved `KeyOutput::{Text,Dead,Action}` to imed **only
   while text focus is active** (`OP_KEY`, source=hw). Pointer path untouched.
3. windowd: relay `OP_SURFACE_TEXT_FOCUS` (app → imed + inputd `set_text_focus`,
   which already exists) and route imed `OP_COMMIT`/`OP_PREEDIT` pushes to the
   focused surface as `OP_SURFACE_TEXT` — pure routing, no IME logic in windowd.
4. app-host: decode `OP_SURFACE_TEXT` → insert into the focused DSL field.
5. Delete `source/services/ime/`; imed joins the boot service list.

### Part 2 — OSK overlay app
6. `userspace/apps/ime-ui`: OSK as `WIN_LEVEL_OVERLAY` surface; show/hide
   driven by the existing ImeHook/`keyboard_visible` state; layout follows the
   `input.keymap` setting (TASK-0298); taps → `OP_KEY` (source=osk).
7. policyd: deny-by-default — only the ime-ui bundle may inject OSK keys.

## Non-Goals

- No candidate popup, no CJK layouts (TASK-0150).
- No a11y announcements (deferred to the a11y track, TASK-0114 direction).
- No OSK drawing or IME state machine inside windowd (hard boundary, RFC-0075).

## Constraints / invariants (hard requirements)

- Input-latency: imed sits only on the key/text branch; the 111 Hz pointer
  VisibleState push is untouched; existing chain-stats counters are the
  regression signal.
- OS services: fixed `[u8; N]` frames, buffer reuse, no per-key `format!`/Vec.
- Markers honest: `imed: ready` fires only after samgr registration + serve
  loop armed (upgrades the stub's marker semantics, same string).
- Marker changes land with `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`
  + docs in the same change.

## Security considerations

### Threat model
- Key injection: a malicious app posing as keyboard source.
- Typed-text leakage via logs/markers.
- Malformed frames from any peer.

### Security invariants (MUST hold)
- imed accepts `OP_KEY` only from inputd's `sender_service_id`; OSK-sourced
  keys only from the app-host instance hosting ime-ui, policyd-gated.
- `OP_SET_FOCUS` accepted only from windowd.
- `field_kind=password`: no preedit preview push, no downstream learning.
- Typed text never in logs/markers; selftests use fixed fixture strings.

### Security proof (required tests)
- `test_reject_key_from_foreign_sender` (imed unit + OS selftest negative path)
- `test_reject_focus_from_non_windowd`
- Reject matrices for all imed frames (inherited from TASK-0146 codecs).

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt`
- **Wire/composition contract**: RFC-0075 + nexus-wire golden tests

## Stop conditions (Definition of Done)

- **Proof (host)**: imed service-logic tests (focus bookkeeping, outcome
  routing, reject paths) green; `just test-host` green.
- **Proof (QEMU) — Part 1** (adjusted 2026-07-21: the deterministic CI lane
  proves ready + identity gate; the positive typing chain needs input
  injection, which only exists via QMP (`QEMU_INPUT_AUTOINJECT`) or the OSK
  (Part 2) — the honest CI markers are:)
  - `init: start imed` / `init: up imed` / `imed: ready` (boot ladder)
  - `SELFTEST: imed reject foreign ok` — foreign-identity `OP_KEY` DENIED
  - Positive chain markers `SELFTEST: ime v2 latin us ok` /
    `... deadkeys de ok` move to the injection lane / Part 2 OSK selftest;
    until then the interactive proof (typing in `just start`) is the gate.
- **Proof (QEMU) — Part 2**:
  - `SELFTEST: ime v2 osk ok` — programmatic OSK tap → commit at focused field
  - `imed: reject foreign key source` — negative selftest (hardening marker)
- **Proof (interactive)**: `just start` — typing into the greeter password
  field works; OSK usable via touch.
- **Gates**: `just check`, `just test-all` green; RFC-0075 checklist ticked.

## Touched paths (allowlist)

- `source/services/imed/` (real implementation, src/ + tests/)
- `source/services/ime/` (**delete**) + workspace/boot references scrubbed
- `source/services/inputd/` (forwarding seam), `source/services/windowd/` (routing only)
- `source/services/app-host/` (OP_SURFACE_TEXT insert)
- `userspace/apps/ime-ui/` (new, Part 2), policyd rules (Part 2)
- `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` — **approval zone**
- Boot service list (Makefile/scripts), `config/service-layout.allow` if needed — **approval zone**
- `source/apps/selftest-client/`, `docs/dev/ui/input/**`, `docs/rfcs/RFC-0075-*.md`, `CHANGELOG.md`

## Part 1 status: DONE (2026-07-21)

Deterministic lane green (`just ci-os-smp1`: `init: up imed`, `imed: ready`,
all four routes, `SELFTEST: imed reject foreign ok`) AND the interactive
positive chain PROVEN LIVE: QMP tap on the greeter secret field →
`apphost: text focus set` → `key a` → **`apphost: text commit applied`**
(one-shot count-only marker) — the full chain
hidrawd→inputd→imed(focus-gated)→windowd→app-host insert.

Landed on the way (debug findings):
- **windowd's server endpoint carries NO per-sender identity for app
  processes** (`sender_sid == 0`) — `OP_SURFACE_TEXT_FOCUS` therefore
  carries the app's own `surface_id` (same trust level + same recorded
  follow-up as `OP_SURFACE_CONTROL`); identity-derived sender resolution
  was removed.
- init's cap-table ceiling (128) broke runtime `@mint-pair` after the imed
  endpoint mints → init now closes its imed pair caps after wiring
  (mint→grant→close); app event channels restored (this was the
  "320x240 desktop / splash hang" regression).
- inputd needed `ipc.core` (`!route-deny: inputd → imed`).
- `trace_line` folds routine markers in interactive boots — success markers
  are invisible there by design; failure markers always print.

## Recorded follow-up (2026-07-21)

- **init cap-table headroom**: init's kernel cap table (`DEFAULT_CAP_SLOTS =
  128`) runs at its ceiling by the end of wiring — every late `cap_clone`
  (allocates init-side) fails NoSpace. The imed routes were switched to
  direct `cap_transfer` (allocates target-side only), but the NEXT service
  with late clone-based wiring will hit the same wall. Either raise the
  kernel constant (kernel-touch, ADR) or audit the remaining clone sites.

## Plan (small PRs)

1. imed real service + inputd forwarding + windowd routing + app-host insert
   (one package: typing works) + markers + selftests.
2. `source/services/ime/` deletion + TRACK-AUTHORITY-NAMING update.
3. ime-ui OSK app + policyd gate + OSK markers/selftests.

## Acceptance criteria (behavioral)

- Typing on the QEMU keyboard appears live in the greeter password field and
  any focused DSL TextField; DE dead keys compose correctly.
- OSK shows on text focus (tablet profile), taps commit through the same imed
  path as hardware keys; foreign-app injection is rejected (proven).
- RFC-0058 gains a pointer: stub contract superseded by RFC-0075.
