---
title: TRACK Settings Family Mode (guardians + members + purchase approvals + restrictions): simple household management without server setup
status: Draft
owner: @ui @platform @security
created: 2026-04-07
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Group and Device Management substrate: tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md
  - NexusAccount (optional account bootstrap): tasks/TRACK-NEXUSACCOUNT.md
  - Settings typed registry: tasks/TASK-0225-settings-v2a-host-settingsd-typed-prefs-providers.md
  - Settings OS/UI wiring: tasks/TASK-0226-settings-v2b-os-settings-ui-deeplinks-search-guides.md
  - Policy capability matrix: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Policy scoped grants v1.1: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - Store parental/payments core: tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md
  - Store parental/payments OS wiring: tasks/TASK-0222-store-v2_2b-os-purchase-flow-entitlements-guard.md
---

## Goal (track-level)

Deliver a very simple **Family Mode** experience in Settings that lets a household manage:

- guardians,
- family members,
- purchases and approvals,
- shared entitlements,
- age- and safety-oriented restrictions,
- and selected device/developer controls,

without expecting a family to self-host a server or understand enterprise-style management tooling.

## Scope boundaries (anti-drift)

- This track is about the **family-facing Settings UX**, not the entire cross-domain management substrate.
- Do not turn Family Mode into an enterprise administration console.
- Do not require a server-based setup flow for ordinary household use.
- Keep the language and workflows simple enough for non-technical users.

## Product stance

The household story should feel:

- simple,
- trustworthy,
- guided,
- and reversible.

Examples of what a family should be able to do from Settings:

- add a child or another family member,
- allow both mother and father to act as guardians,
- approve or deny purchases,
- share purchased items within the family,
- disable Developer Mode for child profiles,
- set age-appropriate restrictions,
- and understand who is allowed to manage what.

## UX stance

Family Mode should prefer:

- a small number of clear roles,
- straightforward add-member flows,
- obvious purchase-approval surfaces,
- and calm explanations over admin jargon.

Where possible, Settings should present this as:

- household setup,
- guardian assignment,
- member permissions,
- purchase and sharing rules,
- and child/device safety defaults.

## Relationship to the broader management substrate

This track is a consumer of the broader management model:

- household membership and roles,
- local policy/config/grant enforcement,
- family-safe restrictions,
- shared household entitlements,
- and optional NexusAccount-based invites or linking.

The Family Mode Settings UI should **reuse** those capabilities rather than inventing a parallel family-only authority.

## Family-specific concerns

Examples of family-facing controls this track should account for:

- purchase approval,
- shared purchases,
- age/content restrictions,
- browser or app restrictions,
- Developer Mode and sideload posture,
- and device inclusion across phone, tablet, watch, auto, and smart-home surfaces when relevant.

## Phase map

### Phase 0 - household model + simple UX

- Define guardian/member roles suitable for non-technical users.
- Define add-member and invite posture.
- Define the minimum useful restriction and approval model.

### Phase 1 - purchases, sharing, and restrictions

- Purchase approvals and shared entitlements become coherent.
- Family-safe restrictions become manageable in one place.

### Phase 2 - multi-device household

- Family management naturally extends to watches, vehicles, tablets, smart displays, and similar household devices.
- The same Settings UX stays simple while the underlying management substrate broadens.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-FAMILY-000: Guardian/member role model v0 (multiple guardians, simple permissions)**
- **CAND-FAMILY-010: Settings family setup flow v0 (invite/add/link members)**
- **CAND-FAMILY-020: Purchase approvals and shared household entitlements v0**
- **CAND-FAMILY-030: Family restrictions UI v0 (age, dev-mode, sideload, app/store posture)**
- **CAND-FAMILY-040: Multi-device family membership UI v0**
