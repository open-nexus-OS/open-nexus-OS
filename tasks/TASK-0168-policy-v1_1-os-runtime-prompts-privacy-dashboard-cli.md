---
title: TASK-0168 Policy v1.1 (OS/QEMU): permsd runtime prompts + Privacy Dashboard (Settings) + audit viewer/export + nx-policy (+ selftests/docs)
status: Draft
owner: @ui
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Policy v1.1 core: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - Policy v1 capability matrix + adapters: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Security & Privacy UI v1 baseline: tasks/TASK-0137-ui-security-privacy-settings-permissions-audit.md
  - permsd baseline: tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - SystemUI→DSL Settings baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - Observability/audit sink direction (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Policy v1.1 core semantics are delivered host-first in `TASK-0167`.
This task wires the user-facing experience:

- runtime permission prompts (permsd + SystemUI sheet),
- a Privacy Dashboard in Settings to view/revoke/expire grants,
- an audit viewer/export surface aligned with the chosen audit sink (prefer logd),
- a CLI `nx policy` for listing/granting/revoking/require-dryrun.

This should extend (not replace) the existing Security & Privacy UI plan in `TASK-0137`.

## Goal

Deliver:

1. Runtime prompts (permsd + SystemUI):
   - `permsd.prompt(appId, cap, scope, rationale) -> decision`
   - decisions map to policyd grants:
     - allow once → `Mode::once` with no expiry
     - while using → `Mode::session`
     - deny → no grant
     - always allow → `Mode::persistent` (dev toggle / admin only if desired)
   - deterministic “test auto-decision” path for QEMU selftests (explicitly labeled)
   - markers (rate-limited):
     - `perms: prompt app=<a> cap=<c>`
     - `perms: decision=<deny|once|session|persistent>`
2. Privacy indicators + kill switches (SystemUI + privacyd):
   - indicators are the **live** “in use” signal (camera/microphone/screen.capture) and must not be faked
   - SystemUI status area shows aggregated indicators (counts + app list tooltip)
   - kill switches (system-only):
     - global toggle per category (camera/microphone/screen.capture)
     - when enabled, guarded services must deny with stable reason `killswitch`
   - markers (rate-limited):
     - `ui: privacy indicators mic=<on|off> cam=<on|off> screen=<on|off>`
     - `privacy: kill <cap>=<true|false>`
2. Privacy Dashboard (Settings, DSL):
   - per-app list of effective grants (cap + scope + mode + expiry)
   - revoke and set-expiry actions
   - history tab (audit trail):
     - reads from logd query if available; otherwise explicit “audit unavailable” stub UI (no fake data)
   - export audit slice:
     - deterministic export format (e.g. JSONL or a small zip) written under `/state` when available
   - markers:
     - `privacy: revoke app=<a> cap=<c>`
     - `privacy: export audit bytes=<n>`
3. CLI `nx policy`:
   - list/grant/revoke/require/devmode
   - stable output lines for tests
4. OS selftests (bounded, QEMU-safe):
   - wait for `policyd: ready v1.1` and `permsd: ready`
   - trigger a runtime prompt via a known service operation; auto-decision “allow once” → marker
   - verify `require()` behavior changes after revoke
   - required markers:
     - `SELFTEST: policy v1.1 prompt ok`
     - `SELFTEST: policy v1.1 require ok`
     - `SELFTEST: policy v1.1 revoke ok`
5. Docs:
   - `docs/policy/overview.md` (scopes/modes/expiry/devmode)
   - `docs/policy/runtime-prompts.md`
   - `docs/policy/privacy-dashboard.md`
   - `docs/policy/audit.md` (sink alignment + export)

## Non-Goals

- Kernel changes.
- Introducing a new `auditd` authority if logd is the chosen sink (keep a single audit source of truth).
- Full admin/auth model; v1 may treat “system UI” as privileged (must be explicit).

Follow-up note (Privacy Dashboard v2):

- A richer Privacy Dashboard with **usage timeline/stats**, deterministic **NDAP** export, and a dedicated
  aggregator service (`privacytelemd`) is tracked as `TASK-0191`/`TASK-0192`.

## Constraints / invariants (hard requirements)

- Determinism: bounded timeouts; injected clocks for expiry in tests; stable UI mocks.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success:
  - audit history must not show “fake logs” if audit sink is unavailable
  - runtime prompt auto-decisions must be explicitly marked as test-only

## Red flags / decision points (track explicitly)

- **RED (requires `/state` for persistence/export)**:
  - persistent grants and audit export require `/state` (`TASK-0009`).
  - without `/state`, the UI must be explicit about non-persistence and exports must be disabled.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p policy_v1_1_host -- --nocapture` (from `TASK-0167`)
  - host UI snapshots/interactions are part of `TASK-0137` style tests (can be extended here if needed)

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=185s ./scripts/qemu-test.sh`
  - Required markers (to be added to `scripts/qemu-test.sh` expected list):
    - `policyd: ready v1.1`
    - `SELFTEST: policy v1.1 prompt ok`
    - `SELFTEST: policy v1.1 require ok`
    - `SELFTEST: policy v1.1 revoke ok`

## Touched paths (allowlist)

- `source/services/policyd/` (OS wiring)
- `source/services/permsd/` + SystemUI prompt overlay
- `source/services/privacyd/` (indicators + kill switches)
- `userspace/systemui/dsl/pages/settings/` (privacy dashboard page)
- `tools/nx-policy/` (new)
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`
- `docs/policy/`

## Plan (small PRs)

1. permsd prompt API + SystemUI consent sheet + policyd grant wiring
2. indicators UI + privacyd bridge (live view + kill switches)
3. privacy dashboard UI + bridge adapters (list/revoke/expiry + audit viewer/export)
4. nx-policy CLI
5. selftests + marker contract + docs

## Acceptance criteria (behavioral)

- In QEMU, runtime prompts and revoke flows are proven by selftest markers without fake audit data.
