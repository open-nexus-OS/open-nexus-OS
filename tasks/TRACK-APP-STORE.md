---
schema: 1
id: TRACK-APP-STORE
title: "App Store (Storefront + publishing): discovery, install, updates, licensing, and review/policy (ecosystem keystone)"
status: draft
owner: ["@platform", "@ui", "@security"]
created: 2026-01-19
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Ads Safety + Family Mode (track)"
    target: tasks/TRACK-ADS-SAFETY-FAMILYMODE.md
  - label: "System Delegation / System Surfaces (no “store inside another app” anti-pattern)"
    target: tasks/TRACK-SYSTEM-DELEGATION.md
  - label: "DevX nx CLI (scaffold/pack/sign/inspect)"
    target: tasks/TASK-0045-devx-nx-cli-v1.md
  - label: "Packaging toolchain (bundle authoring/signing)"
    target: tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - label: "Packages install authority (bundlemgrd)"
    target: tasks/TASK-0130-packages-v1b-bundlemgrd-install-upgrade-uninstall-trust.md
  - label: "Store v1a (host core)"
    target: tasks/TASK-0180-store-v1a-host-storefeedd-storemgrd-ratings.md
  - label: "Store v1b (OS Storefront UI)"
    target: tasks/TASK-0181-store-v1b-os-storefront-ui-selftests-policy-docs.md
  - label: "Store v2.2 (licensing/payments core)"
    target: tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md
  - label: "Store v2.2 (purchase flow/entitlements guard)"
    target: tasks/TASK-0222-store-v2_2b-os-purchase-flow-entitlements-guard.md
  - label: "Supply chain enforcement (store/updater/bundlemgrd)"
    target: tasks/TASK-0198-supply-chain-v2b-os-enforcement-store-updater-bundlemgrd.md
  - label: "Updates/packaging (A/B skeleton)"
    target: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - label: "Updates v2 (offline feed delta/health/rollback)"
    target: tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md
  - label: "NexusAccount (optional account/grants)"
    target: tasks/TRACK-NEXUSACCOUNT.md
  - label: "Zero-Copy App Platform (intents/grants/open-with)"
    target: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
---

## Goal (track-level)

Close the ecosystem loop so that “we have a DSL and apps” becomes a sustainable platform:

- **Storefront**: browse/search/details/install/update/remove (first-party app).
- **Publishing**: a bounded, auditable submission pipeline for developers.
- **Trust**: signatures, provenance, SBOM, and policy enforcement.
- **Licensing (optional)**: purchases/entitlements and parental controls.

This track is a keystone gate: without it, future apps have no credible distribution path.

## System Delegation note

We explicitly avoid the “super-app store inside a chat app” pattern:
- Storefront is a first-party app/service with its own policy gates and auditability.
- Deep links into the Store should be routed via system navigation/delegation surfaces (not embedded web flows).

## Scope boundaries (anti-drift)

- v1 is offline-first and deterministic (feed + `pkg://` fixtures), matching `TASK-0180/0181`.
- “Online store” is a later phase and must remain capability-gated and auditable.
- Avoid introducing duplicate authorities (do not add `catalogd` unless explicitly justified).

## Architecture stance (OS-aligned)

### Authorities

- **Install authority**: `bundlemgrd` (verify/install/uninstall).
- **Store feed**: `storefeedd` (metadata/search).
- **Store orchestration**: `storemgrd` (planning + invokes bundlemgrd).
- **Trust store**: unified signature/trust model (see `TASK-0160`).
- **Policy**: `policyd` (caps like `store.install`, `store.remove`).
- **Licensing/entitlements**: store v2.2 tasks (`TASK-0221/0222`).

### Publishing pipeline (developer-to-store)

Publishing is treated as a **separate, auditable workflow**:

- build → package → sign → validate → submit → review → publish into a feed/channel
- do not mix “developer submission” with the runtime install authority

## Phase map

### Phase 0 — Offline store MVP (already planned)

- Store v1a core services + deterministic tests (`TASK-0180`)
- Store v1b OS Storefront UI + QEMU proofs (`TASK-0181`)

### Phase 1 — Update channel + health/rollback

- Offline delta feed + health + rollback (`TASK-0179`)
- A/B packaging skeleton (`TASK-0007`)

### Phase 2 — Licensing + entitlements (optional, but ecosystem-critical for paid apps)

- Licensing ledger + parental/payments core (`TASK-0221`)
- Purchase flow + entitlements guard (`TASK-0222`)

### Phase 3 — Publishing workflows (developer submission becomes real)

Deliver a developer-facing workflow that can exist even before “online store”:

- a **local publisher** tool that emits a valid feed entry + signed bundles for `pkg://store/`
- a **review gate** surface (static checks, policy checks, SBOM)
- optional: organization/private channels (enterprise)

## Done criteria (track-level)

- A developer can ship an app from source to installable store artifact using documented tooling.
- Storefront installs and updates only after real verification and bundlemgrd success.
- Supply chain checks are enforced (no fake success markers).
