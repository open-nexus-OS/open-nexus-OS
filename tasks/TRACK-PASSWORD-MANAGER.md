---
schema: 1
id: TRACK-PASSWORD-MANAGER
title: "Password Manager: vault + generator + policy/audit first (keystore-backed, no-secret-logs)"
status: draft
owner: ["@security", "@ui"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "keystored (secrets authority)"
    target: tasks/TRACK-KEYSTONE-GATES.md
  - label: "Policy foundations (cap matrix future)"
    target: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - label: "Zero-Copy App Platform (attachments/export)"
    target: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
---

## Goal (track-level)

Deliver a first-party **Password Manager** that proves:

- secure storage patterns (keystore-backed; device-bound),
- no-secret logging discipline,
- explicit policy/grants for sensitive operations,
- safe import/export with clear user intent.

## Scope boundaries (anti-drift)

- No “browser autofill everywhere” in v0 (that needs deeper UI integration and policy).
- No custom crypto schemes; reuse approved primitives and keystore.
- No cloud sync requirement in v0.

## Security invariants (hard)

- Never log: passwords, keys, recovery codes, raw vault blobs, auth headers.
- All exports are explicit user actions and are encrypted by default (format to be specified).
- Clipboard use is bounded and auto-cleared (policy-driven).

## Product scope (v0)

- vault list + item view (logins/notes/tokens)
- generator (password + passphrase)
- tagging/folders (optional)
- import/export:
  - start with a bounded CSV subset (import) and an encrypted export format

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-PASS-000: Vault model v0 (keystore-backed; audit hooks; no-secret logs)**
- **CAND-PASS-010: Password Manager app UI v0 (list/view/generate)**
- **CAND-PASS-020: Import/export v0 (bounded CSV import + encrypted export)**
