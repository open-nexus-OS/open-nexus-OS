<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Dev Mode

Dev Mode is the platform's developer-facing posture for turning Open Nexus OS from a calm default user system into a
serious local development machine when you actually need it.

It exists so the same device can feel:

- simple and focused for everyday users,
- capable and open for developers,
- and still consistent with one platform design language instead of splitting into a separate "developer OS".

## Primary track anchors

- `tasks/TRACK-DEVELOPER-EXPERIENCE-SURFACES.md`
- `tasks/TRACK-CONSOLE-AND-TOOLCHAINS.md`
- `tasks/TRACK-DEVSTUDIO-IDE.md`

## Good fit

Use Dev Mode when you want to:

- build and test apps locally,
- open a Console for project work or automation,
- install runtimes and tools such as Python, Node, npm, or package-manager-style developer tooling,
- run local development services such as a database or dev server,
- or use Dev Studio and related diagnostics on-device.

Typical users:

- students learning app development,
- independent app developers,
- teams testing local builds and sideload flows,
- and advanced users who want a real workstation posture without turning the whole system into legacy admin UX.

## What users experience

When Dev Mode is enabled, users should experience:

- a clear **Developer Features** area in Settings,
- developer surfaces such as **Console** and **Dev Studio** appearing intentionally,
- access to installable **runtimes**, **tools**, and **local services**,
- and a system that still feels designed rather than cluttered or hostile.

The default experience should remain calm:

- no giant always-on power-user shell by default,
- no accidental exposure of developer-only surfaces,
- and no requirement that ordinary users understand toolchains just to use the device.

## What it gives app developers

- a first-party path to local app development on the device itself,
- a way to install and manage app-facing tools without platform hacks,
- a cleaner route to local databases, dev servers, and automation flows,
- and a more open platform story than tightly locked-down mobile systems.

## Common examples

With Dev Mode, a developer should be able to:

- install Python for local scripts and tooling,
- install Node and npm for web or hybrid tooling,
- add a local database service for testing,
- open a project-scoped Console session,
- run package-manager or shell-based setup flows inside the platform's development model,
- and build, sideload, and test apps locally.

## Recommended posture

- treat Dev Mode as an explicit developer posture, not as something every user needs turned on,
- keep developer tools visible only when they add value,
- prefer the platform's managed developer flows over ad-hoc machine mutation,
- and expect the system to stay secure and policy-aware even in developer scenarios.

## Avoid

- treating Dev Mode as a hidden root/admin backdoor,
- assuming that "developer-friendly" means ambient authority everywhere,
- or designing developer flows that make the normal product feel noisy or intimidating.

## Related docs

- `docs/dev/ui/foundations/development/README.md`
- `docs/dev/ui/foundations/development/console.md`
- `docs/dev/ui/foundations/development/package-manager.md`
- `docs/dev/ui/foundations/development/dev-studio.md`
- `docs/dev/platform/developer-workstation.md`
