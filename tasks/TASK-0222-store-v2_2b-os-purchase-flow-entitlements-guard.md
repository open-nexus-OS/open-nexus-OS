---
title: TASK-0222 Store v2.2b (OS/QEMU): licensed enforcement + purchase UX + trials/refunds + parental controls UI + nx-store CLI + selftests/docs (offline)
status: Draft
owner: @platform
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Store v2.2 host core: tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md
  - Store v1 OS storefront baseline: tasks/TASK-0181-store-v1b-os-storefront-ui-selftests-policy-docs.md
  - Packages install authority (bundlemgrd): tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - Supply-chain enforcement hooks: tasks/TASK-0198-supply-chain-v2b-os-enforcement-store-updater-bundlemgrd.md
  - Trust store unification: tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Store v2.2a defines offline licensing/payments/parental logic host-first.
This task wires it into OS/QEMU:

- purchase flow and entitlement UI surfaces,
- installer guardrails for paid SKUs,
- and deterministic selftests/markers.

Everything is offline and deterministic; no network.

## Goal

Deliver:

1. OS services bring-up:
   - `licensed`, `storepaymentsd`, `parentald` are started in OS service graph
   - persistence under `/state`:
     - entitlements DB and ledger DB are only “real” if `/state` exists; otherwise explicit placeholder and no “persist ok” markers
2. Entitlement enforcement:
   - Storefront shows licensed/trial/expired/revoked state per SKU
   - Installer/Store install flow requires entitlement for paid SKUs:
     - deny with stable reason when missing/expired/revoked
   - keep responsibilities clear:
     - supply-chain verification happens via `TASK-0198` path (sigchain/trust)
     - entitlement is a separate check enforced by storemgrd/storefront before install/launch
3. Purchase UX (SystemUI/Storefront):
   - Product detail:
     - Quote price and trial info from storepaymentsd
     - Start Trial / Buy / Refund actions
     - PIN prompt when parental required
   - Manage licenses page:
     - list entitlements with state/expiry
     - revoke action (system-only)
     - restore purchases (rebuild entitlement view from ledger deterministically)
   - markers:
     - `ui: store trial start app=<id> sku=<sku>`
     - `ui: store buy app=<id> sku=<sku>`
     - `ui: store refund app=<id> sku=<sku>`
     - `ui: parental pin prompt`
4. CLI `nx-store` + `nx parental` (host tools):
   - quote/buy/trial/refund/entitlements/revoke/ledger
   - parental status/set-pin/require/limit
   - NOTE: QEMU selftests must not depend on running host tools inside QEMU
5. OS selftests (bounded):
   - `SELFTEST: store trial ok`
   - `SELFTEST: store buy+install ok`
   - `SELFTEST: store refund+revoke ok`
   - `SELFTEST: parental gating ok`
   - selftests must verify by calling services and inspecting entitlement state, not log greps
6. Docs:
   - NLT format + canonical JSON rules
   - ledger + revocations semantics
   - parental model and security caveats
   - testing/markers and fixture keys

## Non-Goals

- Kernel changes.
- Online payments/accounts.
- Claiming strong security if keystore/entropy prerequisites are not met (must be explicit).

## Constraints / invariants (hard requirements)

- Offline & deterministic:
  - all quotes/catalog are from `pkg://fixtures/store/catalog.json` (fixture/authoring; not canonical),
  - runtime may consume a compiled Cap'n Proto catalog (e.g. `catalog.nxf`) if/when needed for bounded parsing.
- `/state` gating: persistence is only real when `TASK-0009` exists.
- No fake success markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p store_v2_2_host -- --nocapture` (from v2.2a)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh`
  - Required markers:
    - `SELFTEST: store trial ok`
    - `SELFTEST: store buy+install ok`
    - `SELFTEST: store refund+revoke ok`
    - `SELFTEST: parental gating ok`

## Touched paths (allowlist)

- `source/services/licensed/`
- `source/services/storepaymentsd/`
- `source/services/parentald/`
- `userspace/apps/storefront/` (purchase flow + licenses page)
- `source/services/storemgrd/` (entitlement checks) and/or `bundlemgrd` (guard hook; decision documented)
- `tools/nx-store/` + `tools/nx-parental/` (or subcommands under nx-store)
- fixtures under `pkg://fixtures/store/` and `pkg://trust/store/`
- `source/apps/selftest-client/`
- `docs/store/` + `docs/tools/nx-store.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. service wiring + entitlement verification + stable errors
2. Storefront purchase/trial/refund UI + parental PIN prompts
3. install guard path + selftests
4. docs + marker contract updates + postflight wrapper (delegating)

## Acceptance criteria (behavioral)

- In QEMU, trial/buy/refund/parental flows are proven deterministically via selftests; paid SKU install is denied without entitlement and allowed with it.
