<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Managed Devices

Managed Devices is the platform posture for devices that belong to a family, school, enterprise, fleet, kiosk setup, or
other controlled group instead of acting as completely standalone personal devices.

The key idea is that Open Nexus OS should be able to manage more than laptops and phones through the same mechanism:

- tablet,
- watch,
- auto,
- smart-home devices,
- kiosk screens,
- panel PCs and HMIs,
- and other purpose-shaped hardware.

## Primary track anchors

- `tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md`
- `tasks/TRACK-SETTINGS-FAMILY-MODE.md`

## Good fit

Use this concept when your app or platform feature needs to account for:

- device enrollment into a family or organization,
- managed app rollout or restrictions,
- group-based browser/network/settings posture,
- kiosk or dedicated-mode operation,
- or devices that should be controlled as part of a larger people-and-device graph.

Typical consumers:

- setup and enrollment flows,
- settings and policy surfaces,
- Store/install and rollout surfaces,
- device-class-aware apps,
- and group-aware account or entitlement flows.

## What users experience

Users should experience:

- one clear idea of "this device belongs to my family / school / company / fleet",
- restrictions or defaults that feel intentional instead of random,
- easier device onboarding,
- and consistent behavior across different device shapes.

For families, this should still stay simple and friendly.
For organizations, it should scale without changing the core model.

## What it gives app developers

- a clearer expectation that devices may operate under household or organization policy,
- a platform-level route for managed restrictions instead of app-by-app policy reinvention,
- a better way to think about multi-device products spanning more than one device class,
- and a stronger ecosystem story for schools, fleets, kiosks, hospitality, and industrial environments.

## Best practice

- design for managed and unmanaged devices as two valid postures of the same platform,
- expect roles, groups, and restrictions to come from system-owned layers,
- and avoid treating phone, tablet, watch, auto, or kiosk as completely separate management universes.

## Avoid

- assuming "managed device" only means enterprise laptop,
- or hardcoding one vertical's workflow so deeply that family, school, kiosk, and fleet cannot reuse the same substrate.

## Related docs

- `docs/dev/technologies/family-mode.md`
- `docs/dev/technologies/dev-mode.md`
- `docs/dev/platform/group-and-device-management.md`
- `docs/security/managed-policy-and-enrollment.md`
