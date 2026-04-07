<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Packaging artifact kinds

Open Nexus OS packaging needs more semantic distinction than "everything is just an app bundle".

This page describes the intended artifact kinds at the packaging and product level so install, update, lifecycle, and
developer-workstation stories can remain understandable as the ecosystem grows.

This page does **not** replace the container-format contract in `docs/packaging/nxb.md`.

## Why artifact kinds matter

Different installed things behave differently:

- an end-user app is visible and launchable,
- a runtime exists so other developer workflows can use it,
- a tool may be used from Console or Dev Studio,
- and a local service may have data, ports, logs, and lifecycle.

If the platform treats them all as undifferentiated bundles, the developer and policy story becomes muddy very quickly.

## Core kinds

### App

An **app** is an installable user-facing application.

Typical expectations:

- appears in launcher/search/navigation when appropriate,
- can be launched directly,
- may declare abilities/capabilities,
- and participates in normal install/update/remove flows.

### Runtime

A **runtime** is an installable execution foundation used by developer or app workflows.

Examples:

- Python
- Node.js

Typical expectations:

- versioned and installable,
- selectable by projects or environments,
- not necessarily a user-facing launcher app,
- and may underpin tools, builds, and package-manager flows.

### Tool

A **tool** is an installable utility, CLI, compiler, package manager, or developer helper.

Examples:

- npm
- pip
- git
- clang

Typical expectations:

- usable from Console and Dev Studio,
- may depend on a runtime,
- may belong to user or project scope,
- and should not imply unrestricted machine mutation.

### Service

A **service** is an installable local capability provider with lifecycle and often persistent state.

Examples:

- database
- local cache
- language server
- dev web server helper

Typical expectations:

- start/stop/restart semantics,
- logs and status,
- storage and possibly ports,
- and explicit visibility in developer workflows.

## Optional future kinds

Depending on how the platform evolves, other kinds may also make sense:

- **provider**
- **sdk**
- **toolset**

Those should be introduced only when they clarify lifecycle and developer understanding rather than just adding taxonomy.

## Relationship to packaging format

The current `.nxb` packaging page defines the container and manifest baseline:

- `docs/packaging/nxb.md`

This page is about the **meaning** of installed artifacts, not the byte layout alone.

That distinction matters because:

- two artifacts can share a container format,
- while still differing in lifecycle, visibility, trust posture, or developer expectations.

## Relationship to developer workflows

Artifact kinds help explain workflows such as:

- install Python as a runtime,
- install npm as a tool,
- install PostgreSQL as a local service,
- install an app for ordinary use or testing,
- and reason about which things should appear in launcher, Console, Dev Studio, or service-management surfaces.

## Best practice

- use artifact kinds to clarify lifecycle and intent,
- keep the set of kinds small and understandable,
- and avoid collapsing all developer-facing installs into "just another app".

## Avoid

- creating many near-duplicate artifact categories with unclear behavior,
- or using artifact kinds as a replacement for the actual security, capability, or lifecycle contracts.

## Related docs

- `docs/packaging/nxb.md`
- `docs/dev/ui/foundations/development/package-manager.md`
- `docs/dev/platform/developer-workstation.md`
- `docs/security/capabilities.md`
