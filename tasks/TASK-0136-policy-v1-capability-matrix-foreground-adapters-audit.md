---
title: TASK-0136 Policy v1 (apps): capability matrix + foreground-only guards + enforcement adapters + audit events (host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - System Delegation / System Surfaces (track): tasks/TRACK-SYSTEM-DELEGATION.md
  - Policy authority + unification: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Security hardening baseline (nexus-sel + audit): tasks/TASK-0008-security-hardening-v1-nexus-sel-audit-device-keys.md
  - Permissions broker (runtime consent): tasks/TASK-0103-ui-v17a-permissions-privacyd.md
  - Storage error contract: tasks/TASK-0132-storage-errors-vfs-semantic-contract.md
  - Share v2 intents: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Notifications v2: tasks/TASK-0069-ui-v8a-notifications-v2-actions-inline-reply.md
  - Clipboard v3: tasks/TASK-0087-ui-v13a-clipboard-v3.md
---

## Context

We already have:

- a `policyd` service (single authority; must be extended, not duplicated),
- `permsd`/`privacyd` for camera/mic/screen runtime consent and indicators,
- Policy-as-Code work (`TASK-0047`) to unify policy islands.

This task introduces a **user-facing app capability matrix** (Android-like permissions) with a strict,
portable enforcement model:

- explicit per-app grants,
- foreground-only guards for sensitive caps,
- service-side adapters call `policyd.require(...)` and emit auditable denies,
- deterministic host tests and OS markers (gated).

System Delegation note:
- v1 starts with `intents.send` / `intents.receive` enforcement for `intentsd`.
- Follow-up “system surfaces” (chat/contacts/maps compose) may introduce finer-grained caps later, but must still route
  all allow/deny decisions through `policyd` (see `tasks/TRACK-SYSTEM-DELEGATION.md`).

Scope note:

- Policy v1.1 (scoped grants + expiry + enumerate/revoke) is tracked as `TASK-0167` (host-first semantics)
  and `TASK-0168` (OS runtime prompts + Privacy Dashboard + CLI).

## Goal

Deliver:

1. Capability catalog (v1) with stable names:
   - `content.read`, `content.write.state`
   - `clipboard.read`, `clipboard.write`
   - `camera`, `microphone`, `screen.capture`
   - `location.coarse`, `location.precise` (declared now; enforcement can remain stubbed until a location service exists)
   - `audio.output`
   - `notifications.post`
   - `intents.send`, `intents.receive`
   - `webview.net` **hard-denied in v1** (even if granted)
   - `storage.manage` (system/admin only)
2. Per-app grants:
   - persist effective grants under `/state` once available (gated on `TASK-0009`)
   - deterministic grant merge semantics (installer baseline + settings toggles)
3. Foreground-only guards (v1):
   - enforced centrally in `policyd` for:
     - `clipboard.read`, `camera`, `microphone`, `screen.capture`
   - foreground identity is set by `windowd`/`appmgrd` (OS-gated); host tests inject it directly
4. Enforcement adapters (service-side):
   - `contentd` (state provider): read/write gated by `content.read` / `content.write.state`
   - `clipboardd`: read gated by `clipboard.read` + foreground; write gated by `clipboard.write`
   - `intentsd`: dispatch gated by `intents.send`; registerTarget gated by `intents.receive`
   - `notifd`: post/update gated by `notifications.post`
   - camera/mic/screen paths: require both `policyd` capability **and** `permsd` runtime consent
   - location path (when present): require `location.coarse`/`location.precise` **and** `permsd` runtime consent,
     and feed `privacyd` indicators (no “fake location” UI)
   - webview: http(s) navigation remains denied with a stable `"policy"` reason
5. Manifest / declared permissions alignment (avoid drift):
   - runtime prompts must be denied if the app did not declare the requested cap in its package manifest
   - do **not** introduce a parallel `manifest.json` contract; reuse the repo’s existing package/bundle manifest direction
     (caps are part of install-time metadata and must be auditable)
6. Audit events:
   - `policyd.require()` emits a structured audit record for allow/deny
   - sink alignment:
     - preferred: emit via `logd` (TASK-0006)
     - fallback (bring-up): deterministic UART markers only, explicitly labeled
7. Stable errors:
   - denials map to `EPERM` via the shared storage error contract (`TASK-0132`) where applicable

Future note (out of scope for v1, but important for System Delegation):
- A unified “default chat surface” (NexusChat) may later support multiple transports (Matrix + SMS/MMS + others).
  When SMS/MMS exists, prefer stable capability names like:
  - `sms.send` / `sms.receive`
  - `mms.send` / `mms.receive`
  and keep enforcement centralized (telephony/sms authority service + `policyd`), with clear user-mediated sending UX
  and auditable receiving/ingress.

## Non-Goals

- Kernel changes.
- Full Policy-as-Code tree unification (that’s `TASK-0047`); this task is the **capability-matrix domain** and adapters.
- A new standalone `auditd` authority (audit sink is `logd` direction; any persistent audit store is a separate follow-up).

## Constraints / invariants (hard requirements)

- `policyd` remains the single authority.
- Channel-bound identity must be used (no trusting appId strings in untrusted payloads in OS builds).
- Deterministic allow/deny reasons and bounded tables.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

New deterministic host tests (suggested: `tests/policy_caps_host/`):

- grant/revoke/listGranted deterministic
- foreground-only gating denies when not foreground
- adapters deny without grants and allow with grants
- combined `policyd + permsd` gating for camera/mic/screen produces stable deny reasons
- webview http(s) is denied even if cap granted (v1 hard deny)
- audit events emitted to a test sink deterministically

### Proof (OS/QEMU) — gated

Once `policyd` + at least one adapter is real in QEMU:

- `policyd: ready`
- `policy: foreground app=<id>`
- `SELFTEST: policy v1 clipboard ok`
- `SELFTEST: policy v1 notif ok`
- `SELFTEST: policy v1 camera ok`
- `SELFTEST: policy v1 screen fg ok`

## Touched paths (allowlist)

- `source/services/policyd/` (extend: capability-matrix domain + foreground + audit)
- `source/services/contentd/` (adapter)
- `source/services/clipboardd/` (adapter)
- `source/services/intentsd/` (adapter; when it exists)
- `source/services/notifd/` (adapter)
- `source/services/permsd/` (consent integration where relevant)
- `tests/`
- `docs/security/policy-overview.md` (or `docs/security/policy-as-code.md` extension)

## Plan (small PRs)

1. policyd: capability catalog + grants store (host-first) + foreground tracking API + audit sink abstraction
2. adapters: wire 1–2 services first (clipboard + notif) to prove pattern, then extend
3. host tests + docs; OS markers once gated deps exist
