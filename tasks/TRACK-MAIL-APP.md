---
title: TRACK Mail app (IMAP/SMTP): offline-first, attachment-safe, account-grant based reference app
status: Draft
owner: @ui @security @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusAccount (identity + online grants): tasks/TRACK-NEXUSACCOUNT.md
  - NexusNet SDK (network/OAuth/providers): tasks/TRACK-NEXUSNET-SDK.md
  - Zero-Copy App Platform (content/grants/saveAs): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Files app (Open With / save/export): tasks/TASK-0086-ui-v12c-files-app-progress-dnd-share-openwith.md
  - Share v2 / Intents (registry + dispatch): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Share v2 (targets + senders + selftests): tasks/TASK-0128-share-v2c-app-senders-selftests-postflight-docs.md
---

## Goal (track-level)

Deliver a first-party **Mail** app that is good enough for daily use and proves:

- account-grant gated networking,
- correct handling of MIME + attachments,
- offline cache + deterministic sync behavior,
- safe “open/save attachment” flows with scoped grants.

## Scope boundaries (anti-drift)

- v0 targets IMAP + SMTP (basic auth mechanisms only if policy allows; prefer modern flows).
- No full enterprise feature set in v0 (shared mailboxes, complex rules).
- No “run arbitrary HTML email with ambient permissions”.

## Security invariants (hard)

- Never log secrets (credentials, tokens, headers).
- Strict bounds on:
  - message size,
  - attachment size,
  - MIME nesting depth,
  - decoded HTML size and external resource loads.
- HTML emails render in a sandboxed webview mode with **network blocked** by default; images are opt-in per message or sender policy.
- Attachments are opened only via `content://` + scoped grants (no raw paths).

## Product scope (v0)

- Inbox list + message view
- compose + send
- search (subject/sender; body indexing later)
- attachments:
  - preview safe types (text/plain, images) with strict limits
  - “Save to Files…” and “Open With…” flows
- multi-account support (basic)

## Architecture stance

- Local database for headers + state; bodies fetched on demand (bounded).
- Background sync is bounded and policy-aware (no infinite polling).
- Secrets are stored via keystore; app never keeps long-lived raw tokens in logs or crash dumps.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-MAIL-000: Mail core model v0 (MIME parse bounds + safe rendering policy)**
- **CAND-MAIL-010: Mail UI v0 (list/view/compose; deterministic host tests)**
- **CAND-MAIL-020: Attachment flows v0 (save/open-with + grants; selftests)**
- **CAND-MAIL-030: Offline cache + bounded sync v0 (FakeNet harness; no unbounded polling)**
