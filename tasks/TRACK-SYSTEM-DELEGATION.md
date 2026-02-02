---
title: TRACK System Delegation (System Surfaces): replace “super-app” with secure, capability-gated system APIs (chat/contacts/share/compose/maps) + defaults + chooser
status: Draft
owner: @runtime @ui @apps
created: 2026-01-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Ads Safety + Family Mode (track): tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - Service architecture: docs/adr/0017-service-architecture.md
  - Policy v1 capability matrix + audit + foreground: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - MIME defaults + content:// providers: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Open With / picker (delegation baseline): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Share v2 intents (registry/dispatch): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Share v2 chooser + targets: tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Share v2 senders + OS selftests: tasks/TASK-0128-share-v2c-app-senders-selftests-postflight-docs.md
  - Scoped URI grants: tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Policy scoped grants v1.1: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - Fediverse suite (NexusChat/NexusSocial): tasks/TRACK-NEXUSSOCIAL.md
  - Office suite (Open With + share/export surfaces): tasks/TRACK-OFFICE-SUITE.md
  - Game SDK (invites + chat delegation): tasks/TRACK-NEXUSGAME-SDK.md
  - PIM suite (Contacts/Calendar surfaces): tasks/TRACK-PIM-SUITE.md
  - Maps app: tasks/TRACK-MAPS-APP.md
  - Notes app (share target): tasks/TRACK-NOTES-APP.md
  - Mail app (compose + attachments): tasks/TRACK-MAIL-APP.md
  - Feeds app (share/open in browser): tasks/TRACK-FEEDS-APP.md
  - Podcasts app (share/open-with): tasks/TRACK-PODCASTS-APP.md
  - Media apps (share/edit/cast): tasks/TRACK-MEDIA-APPS.md
  - Video editor (edit-in + export/share): tasks/TRACK-VIDEO-EDITOR-APP.md
  - NexusFrame (edit-in + export/share): tasks/TRACK-NEXUSFRAME.md
  - NexusVideo (share/send via chat): tasks/TRACK-NEXUSVIDEO.md
  - Recipes (import target): tasks/TRACK-RECIPES-APP.md
  - Core utilities (voice memos share): tasks/TRACK-CORE-UTILITIES.md
  - App Store (avoid super-app anti-pattern): tasks/TRACK-APP-STORE.md
  - Creative apps (import/export/share primitives): tasks/TRACK-CREATIVE-APPS.md
  - DSoftBus share/discovery (optional targets later): tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md
  - Chat inline action cards v0 (track/confirm/open): tasks/TASK-0126D-chat-action-cards-v0-host-inline-track-confirm-open.md
---

## Goal (track-level)

Replace the “WeChat super-app” pattern with a **system-level delegation infrastructure**:
apps can request **system surfaces** (Chat / Compose / Contacts picker / Maps pick-location / Share)
via a stable intent API, while the OS enforces:

- **least privilege** (no ambient authority; capability-gated),
- **user-mediated sensitive actions** (explicit confirmation surfaces),
- **no data exfiltration by default** (content via `content://` + scoped grants only),
- **determinism + testability** (host-first proofs; QEMU markers only after real behavior).

Product outcome: third-party apps no longer need to ship their own chat/compose/contact-picker stacks.
We get “super-infrastructure”, not a “super-app”: safer, more maintainable, and composes over time.

## Problem statement (why this exists)

Today, each app tends to re-implement:
- “send a message to someone” flows,
- “pick a contact / pick a file / share something” flows,
- and (later) “compose a post” flows.

That creates duplicated security bugs, inconsistent UX, and redundant protocol/client code.
We want an OS-native delegation mechanism that is **explicit**, **auditable**, and **policy-driven**.

## Architecture stance (track contract)

- Delegation is expressed as **intents** routed by `intentsd` (control-plane), with:
  - `content://` URIs + scoped grant tokens for any cross-app data (data-plane).
- `policyd` remains the **single authority** for allow/deny decisions (no duplicated policy).
- The chooser and confirmation UI lives in **SystemUI** (user-mediated).
- Identity must be channel-bound (`sender_service_id`); never trust caller-provided app IDs.

## Eligibility rules (anti-monopoly guardrails)

Some system defaults are powerful. To avoid “a third-party app sets itself as default and monopolizes the user’s graph”
we define eligibility rules for becoming a default handler for certain **system surfaces**.

### Default Chat eligibility (required)

An app may only be set as the **default chat surface** (handles `chat.*` actions) if it implements:

- **Export/backup** of user-visible chat data in a portable form (bounded, deterministic),
- **Import/transfer** from another chat app export (bounded, deterministic),
- and the UI/OS wiring exposes these flows in a user-controlled way (Settings / migration UI).

Rationale:
- Without transfer, a default chat app can recreate a WeChat/WhatsApp-style lock-in: the “default” becomes monopoly.
- With mandatory transfer, users can switch defaults without losing their history.

Enforcement direction:
- SystemUI chooser / Settings must refuse “Set as default chat app” unless the app advertises transfer support.
- Policy/audit should record default changes.

## Scope boundaries (anti-drift)

- No kernel changes.
- No proprietary “mini-app runtime” (no app-in-app sandbox inside a system app).
- v1 focuses on **delegation semantics + routing**, not implementing every surface.
- Payment/ID verification are explicitly **out of scope** for this track until consent + audit + keystore
  flows are mature and threat-modeled as their own tasks.

## Phase map

### Phase 0 — Foundation reuse (already tracked; must land first)

Deliverables (by existing tasks):
- `intentsd` registry/dispatch + strict payload policy (`TASK-0126`)
- SystemUI chooser + first targets (`TASK-0127`)
- sender wiring + OS selftests (`TASK-0128`)
- `contentd` + `mimed` + Open With baseline (`TASK-0081`, `TASK-0083`)
- scoped grants + policy integration (`TASK-0084`, `TASK-0167`, `TASK-0136`)

Stop condition:
- Share v2 works end-to-end with deterministic host tests and QEMU markers.

### Phase 1 — “Action-based delegation” (new: System Surfaces v1)

Deliverables:
- Add an **action-based** intent routing mode alongside MIME-based share:
  - examples: `chat.compose`, `contacts.pick`, `social.compose`, `maps.pick_location`
- Add per-action defaults (similar to `mimed` defaults) and deterministic selection ordering.
- Host-first test suite proving:
  - policy denies without grants,
  - chooser ordering + defaults are deterministic,
  - payload bounds and allowed URI schemes are enforced.

Primary task:
- `TASK-0126B-system-delegation-v1a-intent-actions-defaults-policy-host.md` (added by this change-set).

### Phase 2 — First real system surfaces (targets) + proof apps

Deliverables:
- First-party targets (receivers) that implement the “system surface” UX:
  - NexusChat registers `chat.compose` (pre-fill + confirm; returns status only)
  - Contacts app registers `contacts.pick` (returns a single selected `content://` contact URI + scoped grant)
  - NexusSocial registers `social.compose` (pre-fill + confirm)
  - Maps app registers `maps.pick_location` (returns a single selected location object; no live tracking)
- OS selftests demonstrating at least 2 surfaces end-to-end with markers:
  - `SELFTEST: delegation v1 contacts.pick ok`
  - `SELFTEST: delegation v1 chat.compose ok`

### Phase 2.5 — External message ingress (SMS/MMS and similar)

Design stance:
- **SMS/MMS (and other “external transports”) should not create separate user-facing chat apps.**
- A dedicated transport/adapter service (future) can ingest SMS/MMS, but **delivery into the user inbox**
  must be mediated via the **default Chat app** (NexusChat) using delegation/intents.

Implications:
- The system chat surface becomes the single “messages inbox” for:
  - Matrix (NexusChat native),
  - SMS/MMS (ingress adapter),
  - and optionally other bounded ingress sources (future, explicit).
- Policy remains centralized (`policyd`), and user mediation applies for sending; receiving must be auditable.

Transport model note:
- The chat surface should treat recipients as **typed addresses** (e.g. Matrix user/room vs phone number),
  and route via a transport adapter/connector layer.
- This makes it possible to add new chat standards later as installable, policy-gated adapters,
  without each app building a new chat stack or compromising security boundaries.

#### Draft: Address scheme convention (directional, not an ABI contract)

To make “multi-transport chat” concrete without locking the ABI too early, use a simple textual scheme
for recipients that is easy to parse and validate with strict bounds:

- **Matrix user**: `matrix:@user:server.tld`
- **Matrix room**: `matrix:!roomid:server.tld`
- **Phone number (SMS/MMS)**: `tel:+491701234567` (E.164, digits only after `+`)

Rules (security + determinism):
- Parse with **strict length caps** (e.g. max 256 bytes total) and reject invalid UTF-8/characters deterministically.
- **Normalize**: for `tel:` accept only `+` + digits; no spaces, dashes, parentheses.
- No “guessing”: do not infer transport from free-form strings; the scheme selects the adapter.
- Display identity safely: UI should show “via SMS” / “via Matrix” and never let apps spoof the transport label.

### Phase 2.6 — Chat backup + transfer (default-eligibility gate)

Primary deliverable:
- A portable, deterministic **chat export/import** contract that enables switching the default chat app without lock-in.

This is tracked as a follow-up task (host-first) and wired into SystemUI/Settings later:
- `TASK-0126C-chat-transfer-v1a-host-export-import-eligibility.md` (to be added).

### Phase 2.7 — Inline chat action cards (stay in chat; no mini-app runtime)

Design stance:
- For low-risk flows, users should complete the action **inside the chat** via bounded inline cards,
  instead of constantly opening other apps.
- Providers supply **data only**; the chat app renders canonical UI (no embedded web/JS/runtime).

Chosen UX semantics:
- Cards persist as **snapshots** in the chat log.
- Refresh is explicit (user taps “Refresh”), not background polling.

Primary task:
- `TASK-0126D-chat-action-cards-v0-host-inline-track-confirm-open.md`

UX note (v0):
- For inline chat cards, we define canonical action names and a “stay in chat by default” fallback matrix in `TASK-0126D`.

### Phase 3 — Optional distributed targets (DSoftBus)

Deliverables:
- Optional “send to peer” targets implemented as intents (not app-embedded DSoftBus clients),
  gated by DSoftBus policy + consent (`TASK-0158`).

## Security checklist (track-level)

For any new delegation action:
- [ ] Explicit **threat model** in the task (phishing, exfiltration, spoofing)
- [ ] Deny-by-default in `policyd` (capability required)
- [ ] User-mediated confirmation where action is sensitive (chat send, post, location share)
- [ ] No secrets in logs; no token strings in app payloads
- [ ] Negative tests: `test_reject_*` for oversize payloads, missing grants, wrong scheme, not-foreground
- [ ] Deterministic allow/deny reasons and bounded timeouts

## Candidate subtasks (to be extracted later; not part of this patch)

- System Delegation v1b: SystemUI confirmation patterns + anti-phishing indicators (app identity badge).
- Delegation targets: NexusChat/NexusSocial/Contacts/Maps registrations + minimal result contracts.
