---
title: TASK-0126C Chat Transfer v1a (host-first): portable export/import contract + deterministic tests + default-chat eligibility metadata
status: Draft
owner: @runtime @ui
created: 2026-01-28
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - System Delegation track (default-chat eligibility): tasks/TRACK-SYSTEM-DELEGATION.md
  - Intents (action routing baseline): tasks/TASK-0126B-system-delegation-v1a-intent-actions-defaults-policy-host.md
  - Share v2 (content:// + chooser pattern): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md, tasks/TASK-0127-share-v2b-chooser-ui-targets-grants.md
  - Scoped grants: tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - Backup bundles (NBK v1): tasks/TASK-0161-backup-restore-v1a-host-nbk-format-pack-verify-restore.md
---

## Context

If a third-party chat app can set itself as the default chat surface without supporting migration,
the platform recreates a WhatsApp/WeChat-style monopoly.

We need a portable, bounded, deterministic **chat export/import** contract so users can switch default chat apps
without losing their data, while preserving OS security invariants (no ambient authority, no secrets in logs).

This task is host-first: it defines the contract and proves determinism. UI wiring is a later task.

## Goal

Deliver:

1. Chat Transfer v1 format (portable):
   - deterministic, canonical representation of:
     - accounts/identities (redacted/hashed where appropriate; no secrets),
     - conversations (stable IDs),
     - messages (bounded types: text + attachments via `content://` references or exported blobs),
     - participants (typed addresses: `matrix:` / `tel:` per System Delegation draft)
   - deterministic ordering rules and stable timestamps handling (inject clock where needed)
2. Export API shape (host harness):
   - export produces a bundle artifact written to a destination URI (in tests: fixture filesystem)
   - recommended artifact direction:
     - either a dedicated “chat transfer bundle” spec, or
     - reuse NBK v1 as a container with a chat-specific subtree (preferred for ecosystem consistency)
3. Import API shape (host harness):
   - import validates and applies the transfer bundle into a target chat store abstraction
   - must handle partial import gracefully with deterministic error codes
4. Default eligibility metadata:
   - define a small declarative marker a chat app must advertise to be eligible as default:
     - supports export v1
     - supports import v1
     - supports required bounds (max bundle size, max attachment sizes)
5. Deterministic tests:
   - export output hash stable for the same inputs
   - import produces the same target state deterministically
   - `test_reject_*` for oversize bundles, invalid schemas, unsupported versions, and tampered checksums

## Non-Goals

- OS/QEMU UI wiring (Settings migration UI, chooser/“set default” enforcement UI).
- Live sync between chat apps.
- Export of secrets (Matrix private keys, SMS carrier secrets): secrets remain in keystore; exports may carry only
  device-bound wrapped blobs where appropriate and explicitly labeled.

## Constraints / invariants (hard requirements)

- Bounded sizes and deterministic behavior; no unbounded parsing.
- No secrets in logs or error strings.
- Attachments are either:
  - referenced as `content://` with scoped grants (when migrating on-device), or
  - exported as bounded blobs inside the transfer bundle (when doing portable export).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add `tests/chat_transfer_v1_host/`:

- deterministic export bundle bytes hash stable across runs
- verify detects tampering (if NBK-based: checksums; otherwise explicit hash list)
- deterministic import reproduces expected conversation/message state
- rejection tests:
  - `test_reject_oversize_bundle`
  - `test_reject_unknown_version`
  - `test_reject_bad_checksums`
  - `test_reject_invalid_address_scheme`

## Touched paths (allowlist)

- `tests/chat_transfer_v1_host/` (new)
- (future implementation tasks will touch chat app storage + intentsd metadata; not in this task)

## Follow-ups (expected)

- UI: Settings “Default chat app” page must enforce eligibility (cannot set default unless import/export supported).
- NexusChat: implement export/import adapters for Matrix conversations (bounded) and any SMS/MMS ingress store.
