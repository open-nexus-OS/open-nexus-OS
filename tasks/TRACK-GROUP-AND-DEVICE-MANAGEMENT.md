---
title: TRACK Group and Device Management (family + school + enterprise + fleet + kiosk): one policy/config/enrollment substrate across people and devices
status: Draft
owner: @platform @security @runtime
created: 2026-04-07
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy as Code: tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config & Schema v1: tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Policy capability matrix: tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Policy scoped grants v1.1: tasks/TASK-0167-policy-v1_1-host-scoped-grants-expiry-enumeration.md
  - NexusAccount (optional cloud/group bootstrap): tasks/TRACK-NEXUSACCOUNT.md
  - Store parental/licensing core: tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md
  - Store parental/licensing OS wiring: tasks/TASK-0222-store-v2_2b-os-purchase-flow-entitlements-guard.md
  - Accounts/Identity v1.2: tasks/TASK-0223-accounts-identity-v1_2a-host-multiuser-sessiond.md
  - Settings Family Mode UI: tasks/TRACK-SETTINGS-FAMILY-MODE.md
---

## Goal (track-level)

Deliver a shared management substrate that can power:

- **families**,
- **schools**,
- **enterprises**,
- **device fleets**,
- **kiosk / panel / HMI deployments**,
- and multi-device environments spanning phone, tablet, watch, auto, smart-home, and desktop-class devices,

without introducing a second authority stack or turning remote servers into the root of the OS.

The same core mechanism should support:

- people and device enrollment,
- groups and subgroups,
- roles and delegated administration,
- signable policy/config/profile delivery,
- app allow/deny and rollout posture,
- and managed settings, purchases, restrictions, and shared entitlements.

## Scope boundaries (anti-drift)

- This is **not** a kernel-management feature set.
- This is **not** "Active Directory in the kernel" or a direct clone of existing desktop MDM stacks.
- This is **not** a second policy authority beside `policyd`, or a second config authority beside `configd`.
- This is **not** a requirement that families self-host a server.

## Core product stance

The key product idea is:

- **one mechanism**,
- **many product postures**.

Examples:

- a family uses the same substrate to manage purchases, age rules, and developer restrictions,
- a school uses it to manage shared devices, classes, and allowed apps,
- an enterprise uses it for departments, browser profiles, shares, and rollout channels,
- a fleet operator uses it for kiosks, vehicles, smart displays, and industrial panels.

The UX should differ by context, but the underlying policy/config/enrollment model should stay shared.

## Architecture stance

Management should layer over existing authorities:

- `policyd` for allow/deny and grants,
- `configd` for typed configuration distribution and reload,
- `settingsd` for typed user-facing settings surfaces,
- `bundlemgrd` / `storemgrd` for install and rollout,
- `identityd` / `sessiond` for local user/session lifecycle,
- and optional account/cloud providers for remote coordination.

Remote systems should provide **signed or authenticated management inputs**.
Local authorities should still validate, store, explain, and enforce them.

## Shared model

The substrate should eventually support common objects such as:

- users,
- devices,
- groups,
- roles,
- memberships,
- grants,
- policy packs,
- config profiles,
- rollout channels,
- and shared entitlements.

This keeps "family", "class", "department", "fleet", "room", or "vehicle group" as product-level variants rather than
entirely different stacks.

## Enrollment stance

Two onboarding postures should both be supported:

- **no-server / household posture**:
  - simple account-based invites and device linking,
  - especially for Family Mode and small personal groups.
- **managed / organization posture**:
  - self-hosted or provider-backed enrollment,
  - suitable for schools, enterprises, fleets, kiosks, smart spaces, and industrial deployments.

## Capability / policy posture

The management substrate should be able to influence policies such as:

- allowed apps,
- restricted capabilities,
- browser and networking posture,
- developer-mode visibility and enablement,
- sideload and store rules,
- purchase approval and entitlement sharing,
- and device- or group-scoped restrictions.

But enforcement must remain local and explainable through the same policy/config stack already planned for the OS.

## Phase map

### Phase 0 - shared contract framing

- Define the common people/device/group/role model.
- Define enrollment and trust postures.
- Define how policy/config/grants relate to management inputs.

### Phase 1 - household and small-group viability

- Family and small-group use cases become real without requiring self-hosted infra.
- Shared purchases, restrictions, and multi-device membership are credible.

### Phase 2 - managed deployments

- School, enterprise, fleet, kiosk, vehicle, and smart-device postures build on the same substrate.
- Managed config/profile/rollout and delegated administration become first-class.

## Candidate subtasks (to be extracted into real TASK-XXXX)

- **CAND-GDM-000: Membership and role model v0 (users, devices, groups, delegated admins)**
- **CAND-GDM-010: Enrollment and trust contract v0 (household + managed postures)**
- **CAND-GDM-020: Policy/config profile delivery v0 (signed inputs to local authorities)**
- **CAND-GDM-030: Device and app rollout model v0 (groups, channels, allowlists, required apps)**
- **CAND-GDM-040: Shared entitlements and household/org reuse v0**
