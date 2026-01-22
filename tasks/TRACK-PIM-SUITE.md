---
title: TRACK PIM Suite (Calendar + Contacts): local-first, account-aware, share-integrated reference apps
status: Draft
owner: @ui @runtime
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusAccount (identity + online grants): tasks/TRACK-NEXUSACCOUNT.md
  - NexusNet SDK (sync/providers): tasks/TRACK-NEXUSNET-SDK.md
  - Zero-Copy App Platform (content/grants/share): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Share v2 / Intents (registry + dispatch): tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Share v2 (targets + senders + selftests): tasks/TASK-0128-share-v2c-app-senders-selftests-postflight-docs.md
---

## Goal (track-level)

Deliver a first-party **PIM suite** (Personal Information Management) that makes the OS feel complete:

- **Calendar** (events, reminders, repeat rules)
- **Contacts** (people, groups, dedupe, share)

This suite is intentionally local-first, but becomes “account-aware” via NexusAccount/provider model.

## Scope boundaries (anti-drift)

- v0 works fully offline.
- No enterprise groupware parity in v0 (delegation, complex scheduling).
- Avoid leaking contact data into logs/telemetry.

## Shared primitives (required)

- **Canonical data models** for events/contacts with stable IDs.
- **Import/export**:
  - Contacts: vCard import/export subset (bounded)
  - Calendar: ICS import/export subset (bounded)
- **Share integration**:
  - “Add to Calendar” intent target
  - “Save contact” intent target

## Account + sync stance

- **NexusAccount** is optional: users can use local-only PIM.
- Sync is implemented via the **NexusNet provider model**:
  - CalDAV/CardDAV-like providers can be added later as installable providers/services.
- Apps never see raw refresh tokens; secrets remain in keystore via account grants.

## App 1: Calendar

### Scope

- day/week/month views
- create/edit event (title, location, notes, start/end)
- reminders/alerts (bounded)
- repeating rules (subset; deterministic)
- search (by title)

### Security / privacy

- do not infer location without explicit user action/capability
- reminders must be auditable and revocable

## App 2: Contacts

### Scope

- list + search
- contact details (phones/emails/addresses as fields)
- groups/tags (bounded)
- merge/dedupe helper (v1.1+)
- share contact as vCard via Share v2

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-PIM-000: Contacts app v0 (local store + search + vCard share)**
- **CAND-PIM-010: Calendar app v0 (views + edit + reminders + ICS share)**
- **CAND-PIM-020: PIM intents v0 (“Add to Calendar”, “Save Contact”)**
- **CAND-PIM-030: Provider sync hooks v0 (account-aware, but optional)**
