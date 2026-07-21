---
title: TASK-0300 IME personalization store encryption-at-rest (seed)
status: Draft (seed — not scheduled)
owner: @ui
created: 2026-07-21
depends-on:
  - TASK-0204
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Store baseline: tasks/TASK-0204-ime-v2_1b-os-statefs-personal-dict-ui-cli-selftests.md
  - Encryption-at-rest host groundwork: tasks/TASK-0182-encryption-at-rest-v1a-host-secure-keys-io-format-tests.md
  - Superseded securefsd plan: tasks/TASK-0183-encryption-at-rest-v1b-os-securefsd-unlock-ui-migration-cli-selftests.md
---

## Context (seed)

TASK-0204 persists the IME personalization store (learned words, bigrams) as
plaintext NDJSON under `state:/ime/…` because no encrypted storage substrate
exists (TASK-0183 securefsd is Superseded). Learned vocabulary is
privacy-sensitive: it reflects what the user types. This seed reserves the
hardening follow-up and documents the interim threat posture.

## Interim threat note (valid while this seed is open)

- Protection today = platform storage isolation (statefsd is the only writer;
  apps have no raw block access). An attacker with offline access to the
  block image can read learned vocabulary.
- Password fields never train (proven in TASK-0204) — secrets should never
  enter the store by construction; encryption reduces residual exposure of
  non-secret but private vocabulary.

## Scope sketch (to be turned into a full ledger when scheduled)

- Encrypt `state:/ime/**` blobs at rest once an encryption-at-rest substrate
  lands (TASK-0182 line: keys via keystored, AEAD file format, no plaintext
  fallback writes after migration).
- One-shot migration: plaintext → encrypted on first unlock; delete plaintext
  after verified re-read.
- `test_reject_*`: tampered ciphertext → clean empty-store fallback, never a
  parse of attacker-controlled plaintext.

## Blocking prerequisites

- An accepted encryption-at-rest design (successor of TASK-0182/0183) with
  key custody in keystored — RFC required before implementation.
