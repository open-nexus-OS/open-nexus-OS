---
title: TASK-0221 Store v2.2a (host-first): offline licensing (NLT) + sandbox payments + purchase ledger + trials/refunds/revocations + parental controls + deterministic tests
status: Draft
owner: @platform
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Store v1 host baseline: tasks/TASK-0180-store-v1a-host-storefeedd-storemgrd-ratings.md
  - Supply-chain hardening v2 (sigchain/trust primitives): tasks/TASK-0197-supply-chain-v2a-host-sigchain-translog-sbom-provenance.md
  - Trust store unification: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

Store v1 is offline and deterministic and does not implement purchases/payments.
Store v2.2 adds **offline purchases and licensing**:

- Nexus License Tokens (NLT) signed by an offline issuer key,
- a purchase ledger and revocation list,
- trials/refunds,
- and parental controls (PIN + spend limits),

all without network access and with deterministic host proofs.

This task is host-first (formats + core logic + tests). OS/UI wiring is v2.2b.

## Goal

Deliver:

1. Schema and caps (host-first definitions):
   - `schemas/store_v2_2.schema.json` (trials/refunds/limits/parental defaults)
   - policy cap names and default stance (system-only by default)
2. `licensed` (license verification) core model:
   - NLT v1 fields:
     - appId/sku/type(full|trial)/device_fp/aud/iat/nbf/exp/meta + signature
   - canonical encoding rules:
     - **Cap'n Proto** is the canonical token encoding (deterministic, compact, versioned)
     - deterministic JSON is a derived/debug view only (`nx store nlt export --json`)
   - signature verification (Ed25519)
   - verify outcomes:
     - `ok` + computed entitlement, or deterministic deny reason
   - device binding and max device count rules
3. `storepaymentsd` (sandbox wallet):
   - deterministic catalog under `pkg://fixtures/store/catalog.json` (fixture/authoring; not canonical)
     - compiled artifact for runtime may be `pkg://fixtures/store/catalog.nxf` (Cap'n Proto; canonical) if/when needed
   - quote/purchase/refund actions that produce:
     - an NLT (purchase/trial) signed by the issuer key
     - ledger events
4. Purchase ledger + revocations:
   - libSQL-backed store **in host tests** (tempdir):
     - tx table (purchase/refund) + token hash
     - revocations table
   - licensed verification consults revocations (signature-valid token can still be revoked)
5. Parental controls:
   - `parentald` core logic:
     - required toggle
     - daily spend limit enforcement
     - PIN verification
   - pin hashing:
     - deterministic fixture mode for host tests only
     - production mode must not claim security without real entropy/secret salt (explicit red flag)
6. Deterministic host tests `tests/store_v2_2_host/`:
   - quote + trial token verify
   - purchase + install model verify
   - refund -> revocation denies prior token
   - parental gating: pin required, over-limit deterministic
   - replay/idempotency rules (installing same token twice is stable)

## Non-Goals

- Kernel changes.
- Real payments, accounts, networking.
- Cross-device cloud restore.

## Constraints / invariants (hard requirements)

- Determinism:
  - injected clock in tests (no wallclock)
  - stable ordering in lists and stable error reasons
  - canonical JSON for signatures and stable hashing
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (PIN salt / entropy)**:
  - deterministic salts are acceptable for CI fixtures only. Production must use a device secret or keystore-backed salt.
  - Until the entropy/keystore story is unblocked, the OS path must not claim “secure parental PIN”.

- **YELLOW (licensed vs supply-chain sigchain overlap)**:
  - licensing tokens are separate from bundle signatures. Keep responsibilities clear:
    - bundle signature = integrity/publisher trust
    - NLT = entitlement for paid SKUs

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p store_v2_2_host -- --nocapture`

## Touched paths (allowlist)

- `source/services/licensed/` (new, host-testable core)
- `source/services/storepaymentsd/` (new, host-testable core)
- `source/services/parentald/` (new, host-testable core)
- `userspace/libs/storeledger/` (new)
- fixtures under `pkg://fixtures/store/` and trust keys under `pkg://trust/store/` (test-only signer key material must be clearly labeled)
- `tests/store_v2_2_host/`
- docs may land in v2.2b

## Plan (small PRs)

1. NLT canonical encoding + verify + host tests
2. sandbox catalog + purchase/trial issuance + host tests
3. ledger + revocations + host tests
4. parental logic + host tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove trial/purchase/refund/revocation/parental behaviors and stable verification outcomes.
