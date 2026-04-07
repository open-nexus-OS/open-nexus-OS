<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Managed policy and enrollment

Open Nexus OS should be able to support household, school, enterprise, fleet, kiosk, and other managed-device postures
without turning remote infrastructure into the root of the system.

This page captures the security stance for that direction.

## Core stance

Managed operation should follow this rule:

- remote systems may provide **management inputs**
- local system authorities still perform **validation, storage, explanation, and enforcement**

This preserves the platform's capability-first security model even when devices are part of a family or organization.

## What counts as a management input

Examples include:

- enrollment records,
- group membership,
- guardian or admin roles,
- policy bundles,
- config profiles,
- app rollout/channel intent,
- and shared entitlement or purchase-approval state.

These should not be treated as a blanket remote "do anything" channel.

## Local authorities still matter

Managed posture should still rely on local authorities such as:

- `identityd` / `sessiond`
- `policyd`
- `configd`
- `settingsd`
- `bundlemgrd` / `storemgrd`

That means:

- no remote bypass around policy,
- no second hidden install authority,
- and no "management mode" that quietly replaces the system's own trust boundaries.

## Household vs organization posture

Two trust postures should coexist:

### Household / no-server posture

- simple account-based invites,
- straightforward guardian/member setup,
- no self-hosting burden,
- and local enforcement of family restrictions and purchase rules.

### Managed organization posture

- self-hosted or provider-backed enrollment,
- larger-scale group and device control,
- signed/validated policy and config distribution,
- and stronger audit/admin expectations.

## Security expectations

Any managed-policy or enrollment path should preserve:

- local login staying local,
- no ambient authority,
- auditable policy and grant changes,
- explainable local deny/allow results,
- bounded and typed config delivery,
- and revocation/update flows that do not depend on vague hidden remote state.

## Relationship to capabilities and policy

Managed systems should influence policy through the same capability and grant models the OS already uses.

That means:

- restrictions should resolve into local policy/config/grants,
- app and device controls should remain explainable,
- and management inputs should not become a backdoor that skips capability checks.

## Relationship to packaging and rollout

Managed rollouts and restrictions often imply:

- allowed / denied apps,
- required apps,
- rollout channels,
- purchase approvals,
- and shared entitlements.

Those flows should still reuse the platform's store/install and entitlement systems rather than inventing a second admin
installer.

## Best practice

- think of managed posture as **policy/config/grant delivery plus local enforcement**,
- keep family/simple user experiences lightweight,
- keep enterprise/fleet posture scalable without changing the underlying trust model,
- and avoid remote-control shortcuts that weaken the local security model.

## Related docs

- `docs/security/capabilities.md`
- `docs/security/signing-and-policy.md`
- `docs/dev/platform/group-and-device-management.md`
- `tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md`
