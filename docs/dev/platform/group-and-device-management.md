<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Group and Device Management

This page maps how Open Nexus OS can support family, school, enterprise, fleet, kiosk, smart-device, and vehicle
management through one shared substrate instead of many disconnected management systems.

Scope: this is a **platform map**, not a low-level implementation contract.

## Core stance

The intended direction is:

- one shared model for people, devices, roles, groups, and restrictions,
- different product postures for family vs school vs enterprise vs fleet,
- and local enforcement through existing authorities rather than remote root control.

## Main pillars

### Pillar 1: Enrollment and membership

The platform needs a common way to represent:

- users,
- devices,
- groups,
- roles,
- and memberships.

That same substrate should be able to represent:

- a family household,
- a school class or lab,
- a company department,
- a kiosk fleet,
- or a restaurant/device cluster.

### Pillar 2: Policy and config delivery

Managed behavior should flow through:

- `configd` for typed config/profile distribution,
- `policyd` for allow/deny and grants,
- and other local authorities for the final enforcement path.

For Config v1 this means typed authoring/validation in JSON, canonical runtime/persistence effective snapshots in Cap'n Proto,
and no parallel management-only config surface.

### Pillar 3: Install, rollout, and entitlement posture

Managed groups and devices often need:

- allowed / denied apps,
- required apps,
- purchase approvals,
- shared entitlements,
- rollout channels,
- and controlled settings or capabilities.

These should layer over the same store/install/policy systems rather than inventing a second device-management stack.

## Family vs organization posture

The shared substrate should support at least two different product feels:

### Family posture

- no server required,
- simple guardian/member UX,
- purchase approvals and family sharing,
- child-safe defaults and restrictions.

### Managed organization posture

- self-hosted or provider-backed enrollment,
- broader role and device-group models,
- managed app rollout and config profiles,
- and suitability for school, enterprise, kiosk, fleet, or industrial use.

## Device classes

The same management substrate should be able to scale across:

- phone,
- tablet,
- desktop/laptop,
- watch,
- vehicle/auto systems,
- smart-home devices,
- kiosks,
- and industrial or hospitality displays/panels.

This is a major platform differentiator if kept coherent and secure.

## Related docs

- `docs/dev/technologies/family-mode.md`
- `docs/dev/technologies/managed-devices.md`
- `docs/security/managed-policy-and-enrollment.md`
- `tasks/TRACK-GROUP-AND-DEVICE-MANAGEMENT.md`
