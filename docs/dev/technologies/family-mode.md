<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Family Mode

Family Mode is the platform's household management offering for purchase approvals, child-safe restrictions, shared
entitlements, and simple multi-member device guidance.

It is meant to feel easy for ordinary users while still building on the same security and policy model that the rest of
Open Nexus OS uses.

## Primary track anchors

- `tasks/TRACK-SETTINGS-FAMILY-MODE.md`
- `tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md`
- `tasks/TASK-0221-store-v2_2a-host-licensed-ledger-parental-payments.md`
- `tasks/TASK-0222-store-v2_2b-os-purchase-flow-entitlements-guard.md`

## Good fit

Use Family Mode when your app or product flow needs to account for:

- purchase approval,
- shared household purchases,
- age-aware restrictions,
- guardian-controlled settings,
- or devices that belong to one household rather than one completely independent user.

Typical consumers:

- Store and purchase flows,
- apps that expose age-sensitive content,
- device setup and account-linking flows,
- and family-aware settings or account-management surfaces.

## What users experience

Users should experience:

- a simple family setup flow,
- one or more guardians who can manage family rules,
- clear approval or denial of purchases,
- shared access to eligible purchased items,
- and straightforward controls for child or teen profiles.

The household should not need to understand servers, policy trees, or enterprise administration terms.

## What it gives app developers

- a standard purchase-approval and family-sharing story,
- a clearer way to design child-safe or guardian-aware product behavior,
- a platform-level route to household restrictions instead of app-specific reinvention,
- and a path to multi-device household experiences that still feel coherent.

## Best practice

- treat Family Mode as a system-owned household layer, not as an in-app admin panel,
- keep approval and sharing flows explicit and reversible,
- let system settings and store surfaces own family governance where possible,
- and assume that family restrictions should compose with the broader policy model rather than bypass it.

## Avoid

- forcing families into enterprise-style management UX,
- hiding family restrictions behind product-specific jargon,
- or assuming that household sharing means broad ambient access to everything a family owns.

## Related docs

- `docs/dev/technologies/nexus-account.md`
- `docs/dev/technologies/managed-devices.md`
- `docs/dev/platform/group-and-device-management.md`
