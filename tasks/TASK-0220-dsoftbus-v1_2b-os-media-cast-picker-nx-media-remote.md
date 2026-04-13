---
title: TASK-0220 DSoftBus v1.2b (OS/QEMU): Media Remote cast/transfer/group UI (SystemUI) + nx-media remote CLI + selftests/docs (loopback default)
status: Draft
owner: @media
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Media apps product track (TV casting + system-wide controls): tasks/TRACK-MEDIA-APPS.md
  - DSoftBus v1.1 OS wiring (busdir/rpcmux/health): tasks/TASK-0212-dsoftbus-v1_1d-os-busdir-ui-selftests.md
  - DSoftBus v1.2 host core (media remote + mediacast): tasks/TASK-0219-dsoftbus-v1_2a-host-media-remote-proto-cast.md
  - Media UX v2.1 OS baseline (mini-player + nx-media): tasks/TASK-0218-media-v2_1b-os-focus-ducking-miniplayer-nx-media.md
  - DSoftBus UDP devnet gating (must remain gated): tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Integrate media remote cast UX into OS (SystemUI + `nx-media`) on loopback-first DSoftBus wiring.
- **Deliver**: Deterministic QEMU selftests for transfer/group/disconnect flows with policy/schema caps.
- **Out of scope**: Enabling UDP by default or delivering remote audio stream transport.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock cast UI contract, policy caps, and marker honesty rules.
- **Phase B (Host proof)**: requirement-named host reject/control suites are green.
- **Phase C (OS-gated proof)**: canonical QEMU cast marker ladder is green with real state assertions.
- **Phase D (Performance gate)**: deterministic cast/group responsiveness and drift budgets are met.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host: `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture`
- OS: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
- 2-VM (if distributed behavior is asserted): `cd /home/jenning/open-nexus-OS && RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

With busdir/rpcmux in OS (v1.1) and media remote semantics proven host-first (v1.2a),
we wire a “cast picker” UX and remote controls into SystemUI and `nx-media`, proven in QEMU loopback mode.

Audio remains local on each node; v1.2 is control-plane only.

## Goal

Deliver:

1. SystemUI cast picker + remote controls:
   - mini-player adds a Cast button
   - picker lists peers from `busdir.list("media.remote@1")` (or stable name mapping)
   - shows peer health (up/degraded/down)
   - actions:
     - Transfer
     - Group Solo
     - Group Party
     - Disconnect
   - show drift badge for party mode (green/yellow/red thresholds; deterministic mapping)
   - markers:
     - `ui: cast picker open`
     - `ui: cast transfer fp=<fp>`
     - `ui: cast group mode=<solo|party> fp=<fp>`
     - `ui: cast disconnect`
2. `nx-media` extensions (host tool):
   - `cast peers/transfer/group/disconnect`
   - `remote list/control`
   - NOTE: QEMU selftests must not require running host CLIs inside QEMU
3. OS selftests (bounded):
   - spawn loopback peer B exposing `media.remote@1`
   - transfer:
     - `SELFTEST: media cast transfer ok`
   - group solo:
     - `SELFTEST: media cast group solo ok`
   - group party:
     - `SELFTEST: media cast party sync ok`
   - disconnect:
     - `SELFTEST: media cast disconnect ok`
4. Policy/schema:
   - `schemas/dsoftbus_media_v1_2.schema.json`:
     - drift threshold, sync interval, transfer timeout, share enable
   - caps:
     - `media.remote.publish` for mediaremoted
     - `media.remote.control` for SystemUI and `nx-media`
     - reuse `share.send/recv` for file offer fallback
5. Docs:
   - protocol summary and determinism model
   - transfer vs group modes
   - drift model and bounds
   - nx-media remote usage

## Non-Goals

- Kernel changes.
- Enabling UDP mode by default (must remain devnet-gated).
- Remote audio streaming.

## Constraints / invariants (hard requirements)

- Loopback default; UDP path remains gated (aligned with `TASK-0196`).
- No fake success: selftests validate state via service queries and deterministic metrics, not log greps.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (ui-proof drift)**:
  - cast UI markers must be coupled to protocol/service state, not button-click events alone.
- **YELLOW (policy surface)**:
  - `media.remote.control` and share fallback caps must remain fail-closed for non-system callers.

## Security considerations

### Threat model

- Unauthorized cast control through UI or CLI command path.
- Health/drift spoofing that enables invalid transfer/group actions.
- Share fallback payload abuse in cast flow.

### Security invariants (MUST hold)

- cast control actions require authenticated peer context and capability checks.
- health state and drift badges derive from verified protocol data, not client-provided labels.
- share fallback obeys existing quota/integrity constraints.

### DON'T DO (explicit prohibitions)

- DON'T enable cast actions when peer health is down.
- DON'T treat UI confirmation as authorization.
- DON'T claim cast selftest success without protocol-level evidence.

### Attack surface impact

- Significant: OS UI path controlling remote media operations.

### Mitigations

- strict policy checks, deterministic health/drift evaluation, bounded share fallback validations.

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
- Required tests:
  - `test_reject_cast_control_without_cap`
  - `test_reject_group_action_when_peer_down`
  - `test_reject_share_fallback_without_integrity_proof`

### Hardening markers (QEMU, if applicable)

- `SELFTEST: media cast transfer ok`
- `SELFTEST: media cast group solo ok`
- `SELFTEST: media cast party sync ok`
- `SELFTEST: media cast disconnect ok`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p dsoftbus_media_remote_v1_2_host -- --nocapture` (from v1.2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=200s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: media cast transfer ok`
    - `SELFTEST: media cast group solo ok`
    - `SELFTEST: media cast party sync ok`
    - `SELFTEST: media cast disconnect ok`

## Touched paths (allowlist)

- `userspace/systemui/tray/mini_player/` (cast picker UI)
- `source/services/mediaremoted/` (OS bring-up)
- `userspace/libs/mediacast/` (OS wiring)
- `tools/nx-media/` (extend)
- `source/apps/selftest-client/`
- `schemas/dsoftbus_media_v1_2.schema.json`
- `docs/media/remote.md` + `docs/tools/nx-media.md` + `docs/dev/ui/foundations/quality/testing.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. OS service wiring: mediaremoted publish + busdir integration
2. mini-player cast picker + remote control surface
3. nx-media remote/cast commands
4. selftests + docs + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, Media Remote discovery/control/transfer/group/disconnect are proven deterministically in loopback mode via selftest markers.
